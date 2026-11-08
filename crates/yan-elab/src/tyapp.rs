//! Type-function applications in any type position (10.4, RFC 0051 D4). Each application of a
//! user type function outside `type X = f(...)` becomes a hidden alias of that form, placed
//! before the item that uses it, so [`crate::reify::expand`] evaluates it once on the stage-0
//! interpreter and every use resolves to the reified type.
//!
//! Types inside expressions other than statement annotations (a cast in a larger expression)
//! are not visited.

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
            j::StmtKind::Assign { ty: Some(t), .. }
            | j::StmtKind::Ghost { ty: Some(t), .. }
            | j::StmtKind::Bind { ty: Some(t), .. } => ty(t, v),
            j::StmtKind::Unsafe(b)
            | j::StmtKind::StageFor { body: b, .. }
            | j::StmtKind::While { body: b, .. }
            | j::StmtKind::For { body: b, .. } => stmts(b, v),
            j::StmtKind::StageIf { then, els, .. } => {
                stmts(then, v);
                if let Some(e) = els {
                    stmts(e, v);
                }
            }
            j::StmtKind::If { arms, els } => {
                arms.iter_mut().for_each(|(_, b)| stmts(&mut b.stmts, v));
                if let Some(b) = els {
                    stmts(&mut b.stmts, v);
                }
            }
            j::StmtKind::Match { cases, .. } => {
                cases.iter_mut().for_each(|c| stmts(&mut c.body.stmts, v));
            }
            j::StmtKind::With { body, .. } => stmts(&mut body.stmts, v),
            j::StmtKind::Par { body, .. } => match body {
                j::ParBody::Block(b) | j::ParBody::For { body: b, .. } => stmts(b, v),
            },
            _ => {}
        }
    }
}
