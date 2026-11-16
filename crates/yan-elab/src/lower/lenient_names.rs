//! N52 and N53: free calls the lenient reader reads as `math.f(x)` or as a method or operator.

use super::*;

const COLLECTIONS: [&str; 4] = ["deque", "heap", "sorted_dict", "sorted_set"];

pub(crate) const MATH_FREE: [&str; 6] = ["sqrt", "sin", "cos", "tan", "exp", "log"];

impl Lower<'_, '_, '_> {
    /// `n(args)` for an unbound `n` that N52 or N53 names, lowered as its replacement.
    pub(super) fn lenient_free(
        &mut self,
        n: &str,
        args: &[j::Arg],
        at: Span,
        want: Option<&Ty>,
    ) -> R<Option<(Rhs, Ty)>> {
        if self.cx.strict {
            return Ok(None);
        }
        let xs: Vec<&j::Expr> = args
            .iter()
            .map_while(|a| match (&a.name, &a.kind) {
                (None, j::ArgKind::Expr(e)) => Some(e),
                _ => None,
            })
            .collect();
        if xs.len() != args.len() {
            return Ok(None);
        }
        let ex = |kind| j::Expr {
            kind,
            span: at,
            paren: false,
        };
        let name = |s: &str| ex(j::ExprKind::Name(s.to_owned()));
        let boxed = |e: &j::Expr| Box::new(e.clone());
        let method = |recv: &j::Expr, m: &str| {
            ex(j::ExprKind::Call(
                Box::new(ex(j::ExprKind::Attr(boxed(recv), m.to_owned(), at))),
                Vec::new(),
            ))
        };
        let call = |f: &str| ex(j::ExprKind::Call(Box::new(name(f)), args.to_vec()));
        let (code, e, to) = match (n, xs.as_slice()) {
            (f, _) if MATH_FREE.contains(&f) && !args.is_empty() => {
                let attr = ex(j::ExprKind::Attr(Box::new(name("math")), f.to_owned(), at));
                let e = ex(j::ExprKind::Call(Box::new(attr), args.to_vec()));
                ("N52", e, format!("math.{f}(...)"))
            }
            ("char_count" | "parse_int", [s]) => ("N53", method(s, n), format!("s.{n}()")),
            ("to_string", [_]) => ("N53", call("str"), "str(x)".into()),
            ("to_int", [_]) => ("N53", call("int"), "int(s)".into()),
            ("slice", [xs, a, b]) => {
                let bounds = [Some(boxed(a)), Some(boxed(b)), None];
                let e = ex(j::ExprKind::Slice(boxed(xs), bounds, false));
                ("N53", e, "xs[a:b]".into())
            }
            ("byte_at", [s, i]) => {
                let bytes = method(s, "bytes");
                let e = ex(j::ExprKind::Index(Box::new(bytes), boxed(i)));
                ("N53", e, "s.bytes()[i]".into())
            }
            _ => return Ok(None),
        };
        self.lenient(code, at, &format!("`{n}(...)` normalized to `{to}`"));
        self.mark_call(at, yan_surface::Mark::Lenient(code.into()));
        self.rhs(&e, want).map(Some)
    }

    /// N54: `collections.deque(xs)` and `std.len(xs)` are the prelude call; a name the module
    /// lacks, as `collections.Counter`, is E200 with the nearest std names as a choice.
    pub(super) fn lenient_module_call(
        &mut self,
        module: &str,
        m: &str,
        args: &[j::Arg],
        at: Span,
        name_at: Span,
        want: Option<&Ty>,
    ) -> R<Option<(Rhs, Ty)>> {
        if self.cx.strict || self.locals.contains_key(module) {
            return Ok(None);
        }
        let mut names: Vec<&str> = match module {
            "collections" => COLLECTIONS.to_vec(),
            "std" => BUILTINS.to_vec(),
            _ => return Ok(None),
        };
        if module == "std" {
            names.extend(crate::stdlib::members("prelude"));
        }
        if names.contains(&m) {
            self.lenient(
                "N54",
                at,
                &format!("`{module}.{m}(...)` normalized to `{m}(...)`"),
            );
            let f = j::Expr {
                kind: j::ExprKind::Name(m.to_owned()),
                span: name_at,
                paren: false,
            };
            let e = j::Expr {
                kind: j::ExprKind::Call(Box::new(f), args.to_vec()),
                span: at,
                paren: false,
            };
            return self.rhs(&e, want).map(Some);
        }
        self.err_near(
            "E200",
            name_at,
            format!("`std.{module}` has no `{m}`"),
            m,
            names,
        );
        for a in args {
            if let j::ArgKind::Expr(e) | j::ArgKind::Ref(e) = &a.kind {
                self.atom(e, None)?;
            }
        }
        Ok(Some((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err)))
    }
}
