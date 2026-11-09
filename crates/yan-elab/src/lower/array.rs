//! `Array` values with literal sizes (11.2), stored flat and row-major as a list, and
//! `gpu.launch` of a `@kernel` function (17.1, RFC 0055 stage 3, RFC 0058).

use super::*;
use crate::kernel::Step;

impl Lower<'_, '_, '_> {
    /// `a[i, j]` on an `Array` as a place: the element at the row-major offset, which
    /// `std.array.offset2` folds axis by axis and sets to -1 when an index leaves its axis.
    pub(super) fn array_place(&mut self, i: &j::Expr, mut p: Place, t: &Ty) -> R<(Place, Ty)> {
        let Ty::Array(_, et) = t else {
            unreachable!("an array")
        };
        let Some(dims) = t.dims() else {
            return unsupported("an element of an `Array` whose sizes are not literals");
        };
        let parts: Vec<&j::Expr> = match &i.kind {
            j::ExprKind::Tuple(xs) => xs.iter().collect(),
            _ => vec![i],
        };
        if parts.len() != dims.len() {
            self.err(
                "E201",
                i.span,
                format!(
                    "a `{}` takes {} indices, found {}",
                    self.ty_text(t),
                    dims.len(),
                    parts.len()
                ),
            );
            return Ok((p, Ty::Err));
        }
        if let [x] = parts.as_slice() {
            let (a, at) = self.atom(x, Some(&Ty::Int))?;
            self.expect(&at, &Ty::Int, x.span);
            p.projs.push(Proj::Index(a));
            return Ok((p, (**et).clone()));
        }
        let int = |n: i64, at| Atom::Lit(Literal::Int(n.to_string()), at);
        let mut off = int(0, i.span);
        let mut extent = 1;
        for (x, d) in parts.into_iter().zip(dims) {
            let (a, at) = self.atom(x, Some(&Ty::Int))?;
            self.expect(&at, &Ty::Int, x.span);
            let c = call(
                "std.array.offset2",
                vec![],
                vec![
                    let_arg(off),
                    let_arg(a),
                    let_arg(int(extent, x.span)),
                    let_arg(int(d, x.span)),
                ],
                x.span,
            );
            off = self.tmp(&Ty::Int, Rhs::Call(c), x.span);
            extent *= d;
        }
        p.projs.push(Proj::Index(off));
        Ok((p, (**et).clone()))
    }

    /// A list literal, or a grid (nested lists by row), where an `Array` is expected: its
    /// nesting must match the sizes, and the elements are stored flat.
    pub(super) fn array_literal(&mut self, xs: &[j::Expr], want: &Ty, at: Span) -> R<(Rhs, Ty)> {
        let Ty::Array(_, et) = want else {
            unreachable!("an array")
        };
        let Some(dims) = want.dims() else {
            return unsupported("an `Array` literal whose sizes are not literals");
        };
        let mut flat = Vec::new();
        if let Err(span) = flatten(xs, &dims, at, &mut flat) {
            self.err(
                "E201",
                span,
                format!(
                    "this literal does not have the shape of `{}`",
                    self.ty_text(want)
                ),
            );
            return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err));
        }
        let list = j::Expr {
            kind: j::ExprKind::List(flat),
            span: at,
            paren: false,
        };
        let (a, _) = self.atom(&list, Some(&Ty::List(et.clone())))?;
        Ok((Rhs::Atom(a), want.clone()))
    }

    /// `gpu.launch(k, args...)` (RFC 0058): one call of the kernel per program instance, each
    /// writing its disjoint part of a zeroed `out`, which is the result.
    pub(super) fn launch(&mut self, args: &[j::Arg], at: Span) -> R<(Rhs, Ty, Option<Ty>)> {
        let Some((k, rest)) = args.split_first() else {
            return unsupported("`gpu.launch` without a `@kernel` function");
        };
        let kernel = match &k.kind {
            j::ArgKind::Expr(j::Expr {
                kind: j::ExprKind::Name(n),
                ..
            }) if k.name.is_none() => self.g.kernels.get(n).map(|g| (n.clone(), g.clone())),
            _ => None,
        };
        let Some((name, grid)) = kernel else {
            return unsupported("`gpu.launch` of anything but a `@kernel` function by name");
        };
        let out_ty = self.g.fns[&name]
            .params
            .last()
            .map(|p| p.ty.clone())
            .expect("a kernel has `out`");
        let (Some(dims), Ty::Array(_, et)) = (out_ty.dims(), &out_ty) else {
            return unsupported("a `@kernel` result whose sizes are not literals");
        };
        let zero = match **et {
            Ty::Int => Literal::Int("0".into()),
            Ty::Float => Literal::Float("0.0".into()),
            _ => return unsupported("a `@kernel` result of other than `int` or `float` elements"),
        };
        let steps = match grid.steps(dims.len()) {
            Ok(s) => s,
            Err(why) => return unsupported(&why),
        };
        for (a, (d, s)) in dims.iter().zip(&steps).enumerate() {
            if let Step::By(s) = s
                && d % s != 0
            {
                self.err(
                    "E521",
                    at,
                    format!(
                        "axis {a} of `{name}`'s result has {d} elements, not a whole number of \
                         {s}-element tiles"
                    ),
                );
                return Ok((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None));
            }
        }

        let out = self.fresh("launch");
        let total: i64 = dims.iter().product();
        let filled = call(
            "std.array.filled",
            vec![et.targ()],
            vec![
                let_arg(Atom::Lit(zero, at)),
                let_arg(Atom::Lit(Literal::Int(total.to_string()), at)),
            ],
            at,
        );
        self.bind(&out, true, &out_ty, Rhs::Call(filled), at, at);
        self.mutable.insert(out.clone());

        let x = |kind| j::Expr {
            kind,
            span: at,
            paren: false,
        };
        let name_e = |n: &str| x(j::ExprKind::Name(n.to_owned()));
        let int_e = |n: i64| x(j::ExprKind::Lit(j::Lit::Int(n.to_string())));
        let arg = |name: Option<&str>, kind| j::Arg {
            name: name.map(str::to_owned),
            kind,
            span: at,
        };
        let origin: Vec<String> = (0..2).map(|_| self.fresh("t")).collect();
        let fields = (0..2).map(|a| {
            let v = if a < dims.len() {
                name_e(&origin[a])
            } else {
                int_e(0)
            };
            arg(Some(&format!("i{a}")), j::ArgKind::Expr(v))
        });
        let ctor = x(j::ExprKind::Call(
            Box::new(name_e("TileOrigin")),
            fields.collect(),
        ));
        // By keyword, as the launch already matched them to the kernel's parameters (E210).
        let params = &self.g.fns[&name].params;
        let mut call_args = vec![arg(Some(&params[0].name), j::ArgKind::Expr(ctor))];
        call_args.extend(rest.iter().zip(&params[1..]).map(|(a, p)| j::Arg {
            name: Some(a.name.clone().unwrap_or_else(|| p.name.clone())),
            ..a.clone()
        }));
        call_args.push(arg(Some("out"), j::ArgKind::Ref(name_e(&out))));
        let mut body = vec![j::Stmt {
            kind: j::StmtKind::Expr(x(j::ExprKind::Call(Box::new(name_e(&name)), call_args))),
            span: at,
        }];
        for a in (0..dims.len()).rev() {
            let step = match steps[a] {
                Step::By(s) => s,
                Step::Once => dims[a],
            };
            let range = x(j::ExprKind::Call(
                Box::new(name_e("range")),
                [0, dims[a], step]
                    .map(|n| arg(None, j::ArgKind::Expr(int_e(n))))
                    .into(),
            ));
            body = vec![j::Stmt {
                kind: j::StmtKind::For {
                    label: None,
                    target: vec![origin[a].clone()],
                    iter: range,
                    contracts: Vec::new(),
                    body,
                },
                span: at,
            }];
        }
        let was = std::mem::replace(&mut self.in_launch, true);
        let r = self.stmts(&body);
        self.in_launch = was;
        r?;
        Ok((Rhs::Atom(Atom::Name(out, at)), out_ty, None))
    }
}

/// The elements of `xs` nested to `dims`, row-major, or the span of the first part that does not
/// fit.
fn flatten(xs: &[j::Expr], dims: &[i64], at: Span, out: &mut Vec<j::Expr>) -> Result<(), Span> {
    let Some((&d, inner)) = dims.split_first() else {
        unreachable!("an array has an axis")
    };
    if xs.len() as i64 != d {
        return Err(at);
    }
    for x in xs {
        match (&x.kind, inner.is_empty()) {
            (_, true) => out.push(x.clone()),
            (j::ExprKind::List(ys), false) => flatten(ys, inner, x.span, out)?,
            _ => return Err(x.span),
        }
    }
    Ok(())
}
