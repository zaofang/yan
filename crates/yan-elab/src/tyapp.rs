//! Type-function applications in any type position (10.4, RFC 0051 D4). Each application of a
//! user type function outside `type X = f(...)` becomes a hidden alias of that form, placed
//! before the item that uses it, so [`crate::reify::expand`] evaluates it once on the stage-0
//! interpreter and every use resolves to the reified type.
//!
//! The types inside expressions are visited too, so a cast inside a call is an application.
//! `session.protocol(...)` hoists the same way, into the alias form its expansion reads.

use yan_hir as j;

use crate::reify;

pub(crate) fn hoist(m: &mut j::Module) {
    let mut at = 0;
    let mut n = 0;
    while at < m.items.len() {
        let mut item = m.items[at].clone();
        let from = m.items[at].clone();
        let mut found = Vec::new();
        {
            let mut visit = |t: &mut j::Type| {
                let j::Type::Call { path, .. } = t else {
                    return;
                };
                if reify::type_fn(m, path).is_none() {
                    return;
                }
                n += 1;
                let name = format!("__app_{n}");
                found.push(alias(&name, t.clone(), &from));
                *t = j::Type::Named {
                    path: vec![name],
                    args: Vec::new(),
                };
            };
            item_types(&mut item.kind, &mut visit);
        }
        if found.is_empty() {
            at += 1;
        } else {
            m.items[at] = item;
            let k = found.len();
            m.items.splice(at..at, found);
            at += k + 1;
        }
    }
}

fn alias(name: &str, call: j::Type, from: &j::Item) -> j::Item {
    j::Item {
        decorators: Vec::new(),
        is_pub: false,
        kind: j::ItemKind::Type(j::TypeDef {
            name: name.to_owned(),
            tparams: Vec::new(),
            rhs: j::TypeRhs::Alias(vec![call]),
            where_: None,
        }),
        span: from.span,
        name_span: from.name_span,
    }
}

type V<'a> = &'a mut dyn FnMut(&mut j::Type);

fn ty(t: &mut j::Type, v: V) {
    match t {
        j::Type::Named { args, .. } => {
            for a in args {
                if let j::TArg::Type(t) = a {
                    ty(t, v);
                }
            }
        }
        j::Type::Callable { params, ret, .. } => {
            for p in params {
                ty(p, v);
            }
            ty(ret, v);
        }
        j::Type::Call { .. } | j::Type::None => {}
    }
    v(t);
}

fn item_types(k: &mut j::ItemKind, v: V) {
    match k {
        j::ItemKind::Func(f) | j::ItemKind::Lemma(f) => func(f, v),
        j::ItemKind::Record(r) => {
            tparams(&mut r.tparams, v);
            fields(&mut r.fields, v);
        }
        j::ItemKind::Type(d) => {
            tparams(&mut d.tparams, v);
            match &mut d.rhs {
                j::TypeRhs::Sum(vs) => vs.iter_mut().for_each(|x| fields(&mut x.fields, v)),
                // `type X = f(...)` is the application itself, not a use of one.
                j::TypeRhs::Alias(ts) if matches!(ts.as_slice(), [j::Type::Call { .. }]) => {}
                j::TypeRhs::Alias(ts) => ts.iter_mut().for_each(|t| ty(t, v)),
                j::TypeRhs::Newtype(t) => ty(t, v),
                j::TypeRhs::Caps(_) => {}
            }
        }
        j::ItemKind::Trait(t) => {
            tparams(&mut t.tparams, v);
            t.sigs.iter_mut().for_each(|(_, f)| func(f, v));
        }
        j::ItemKind::Impl(i) => {
            i.args.iter_mut().for_each(|t| ty(t, v));
            ty(&mut i.for_ty, v);
            i.assoc.iter_mut().for_each(|(_, t)| ty(t, v));
            i.fns.iter_mut().for_each(|f| func(f, v));
        }
        j::ItemKind::Const { ty: t, .. } => ty(t, v),
        j::ItemKind::Extern(_) | j::ItemKind::Verbatim(_) => {}
    }
}

fn tparams(ps: &mut [j::TParam], v: V) {
    for p in ps {
        if let j::TParam::Value { ty: t, .. } = p {
            ty(t, v);
        }
    }
}

fn fields(fs: &mut [j::Field], v: V) {
    fs.iter_mut().for_each(|f| ty(&mut f.ty, v));
}

fn func(f: &mut j::FuncDef, v: V) {
    tparams(&mut f.tparams, v);
    for p in &mut f.params {
        match p {
            j::Param::Value { ty: t, .. } | j::Param::Pack { ty: t, .. } => ty(t, v),
            j::Param::Slash => {}
        }
    }
    if let Some(r) = f.ret.as_mut().and_then(|r| r.ty.as_mut()) {
        ty(r, v);
    }
    if let Some(b) = &mut f.body {
        stmts(&mut b.stmts, v);
    }
}

fn stmts(ss: &mut [j::Stmt], v: V) {
    for s in ss {
        match &mut s.kind {
            j::StmtKind::Assign {
                target,
                ty: t,
                value,
            } => {
                t.iter_mut().for_each(|t| ty(t, v));
                expr(target, v);
                expr(value, v);
            }
            j::StmtKind::Ghost { ty: t, value, .. } | j::StmtKind::Bind { ty: t, value, .. } => {
                t.iter_mut().for_each(|t| ty(t, v));
                expr(value, v);
            }
            j::StmtKind::AugAssign { target, value, .. } => {
                expr(target, v);
                expr(value, v);
            }
            j::StmtKind::Expr(e)
            | j::StmtKind::Become(e)
            | j::StmtKind::Fail(e)
            | j::StmtKind::Assert(e)
            | j::StmtKind::Return(Some(e)) => expr(e, v),
            j::StmtKind::Unsafe(b) => stmts(b, v),
            j::StmtKind::StageFor { iter, body, .. } | j::StmtKind::For { iter, body, .. } => {
                expr(iter, v);
                stmts(body, v);
            }
            j::StmtKind::While { cond, body, .. } => {
                expr(cond, v);
                stmts(body, v);
            }
            j::StmtKind::StageIf { cond, then, els } => {
                expr(cond, v);
                stmts(then, v);
                if let Some(e) = els {
                    stmts(e, v);
                }
            }
            j::StmtKind::If { arms, els } => {
                for (c, b) in arms {
                    expr(c, v);
                    stmts(&mut b.stmts, v);
                }
                if let Some(b) = els {
                    stmts(&mut b.stmts, v);
                }
            }
            j::StmtKind::Match { scrutinee, cases } => {
                expr(scrutinee, v);
                for c in cases {
                    c.guard.iter_mut().for_each(|g| expr(g, v));
                    stmts(&mut c.body.stmts, v);
                }
            }
            j::StmtKind::With { body, .. } => stmts(&mut body.stmts, v),
            j::StmtKind::Par { body, .. } => match body {
                j::ParBody::Block(b) | j::ParBody::For { body: b, .. } => stmts(b, v),
            },
            _ => {}
        }
    }
}

/// The types inside an expression: the target of each cast.
fn expr(e: &mut j::Expr, v: V) {
    use j::ExprKind as K;
    match &mut e.kind {
        K::Cast(x, t) => {
            expr(x, v);
            ty(t, v);
        }
        K::FStr { pieces, .. } => {
            for p in pieces {
                if let j::FPiece::Expr(x, _) = p {
                    expr(x, v);
                }
            }
        }
        K::Tuple(xs) | K::List(xs) | K::Set(xs) => xs.iter_mut().for_each(|x| expr(x, v)),
        K::Dict(kvs) => kvs.iter_mut().for_each(|(k, x)| {
            expr(k, v);
            expr(x, v);
        }),
        K::Comp {
            elem,
            value,
            iter,
            cond,
            ..
        } => {
            expr(elem, v);
            value.iter_mut().for_each(|x| expr(x, v));
            expr(iter, v);
            cond.iter_mut().for_each(|x| expr(x, v));
        }
        K::Call(f, args) => {
            expr(f, v);
            for a in args {
                match &mut a.kind {
                    j::ArgKind::Expr(x) | j::ArgKind::Ref(x) | j::ArgKind::Become(x) => expr(x, v),
                }
            }
        }
        K::Index(a, b) | K::Binary(_, a, b) | K::Compare(_, a, b) => {
            expr(a, v);
            expr(b, v);
        }
        K::Slice(x, parts, _) => {
            expr(x, v);
            parts.iter_mut().flatten().for_each(|p| expr(p, v));
        }
        K::Attr(x, ..)
        | K::Try(x)
        | K::Unary(_, x)
        | K::Not(x)
        | K::Lambda(_, x)
        | K::Reify(x)
        | K::Stage(x) => expr(x, v),
        K::Cond { then, cond, els } => {
            expr(then, v);
            expr(cond, v);
            expr(els, v);
        }
        K::Fallback(x, fb) => {
            expr(x, v);
            match &mut **fb {
                j::Fallback::Value(y) | j::Fallback::Bind(_, y) | j::Fallback::Fail(y) => {
                    expr(y, v)
                }
                j::Fallback::Return(y) => y.iter_mut().for_each(|y| expr(y, v)),
                j::Fallback::Block(_, b) => stmts(b, v),
                j::Fallback::Break | j::Fallback::Continue => {}
            }
        }
        K::Name(_) | K::Lit(_) | K::Hole | K::Wild => {}
    }
}
