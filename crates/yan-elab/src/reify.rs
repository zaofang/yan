//! User type functions (10.4, RFC 0051 D3, D5): `type X = f(args)` where `f` is a function in
//! this module returning `Type`. The application runs on the stage-0 interpreter against the
//! module's own items, and the type it reifies is declared like a written one.

use std::collections::{HashMap, HashSet};

use yan_core::ast as y;
use yan_diag::Span;
use yan_hir as j;
use yan_interp::Stage0;

use crate::{Cx, R, stage0, unsupported};

/// The function an alias applies, when it is a function of the module returning `Type`.
pub(crate) fn type_fn<'a>(m: &'a j::Module, path: &[String]) -> Option<&'a j::Item> {
    let [name] = path else { return None };
    m.items.iter().find(|it| {
        matches!(&it.kind, j::ItemKind::Func(f) if &f.name == name
            && matches!(f.ret.as_ref().and_then(|r| r.ty.as_ref()),
                Some(j::Type::Named { path, args }) if *path == ["Type"] && args.is_empty()))
    })
}

/// The alias `type X = f(args)` at `it`, if `f` is a type function of `m`.
fn application<'a>(
    m: &j::Module,
    it: &'a j::Item,
) -> Option<(&'a str, &'a [String], &'a [j::Arg])> {
    let j::ItemKind::Type(j::TypeDef {
        name,
        rhs: j::TypeRhs::Alias(ts),
        ..
    }) = &it.kind
    else {
        return None;
    };
    let [j::Type::Call { path, args }] = ts.as_slice() else {
        return None;
    };
    type_fn(m, path)?;
    Some((name, path, args))
}

/// An argument as the memo key spells it: a literal, or a type name.
fn spell(e: &j::Expr) -> Option<String> {
    Some(match &e.kind {
        j::ExprKind::Name(n) => n.clone(),
        j::ExprKind::Index(b, k) => format!("{}[{}]", spell(b)?, spell(k)?),
        j::ExprKind::Tuple(xs) => xs.iter().map(spell).collect::<Option<Vec<_>>>()?.join(", "),
        _ => match crate::const_val(e) {
            Ok(v) => format!("{v:?}"),
            // A stage-0 constant or call: its spelling keys the run, and equal results unify
            // by the name the function gives the type.
            Err(_) => format!("{:?}", e.kind),
        },
    })
}

fn key(path: &[String], args: &[j::Arg]) -> R<String> {
    let mut parts = Vec::new();
    for a in args {
        match (&a.name, &a.kind) {
            (None, j::ArgKind::Expr(e)) => parts.push(
                spell(e)
                    .ok_or(())
                    .or_else(|()| unsupported("a type-function argument of this form"))?,
            ),
            _ => return unsupported("a named or `&` type-function argument"),
        }
    }
    Ok(format!("{}({})", path.join("."), parts.join(",")))
}

fn words(s: &str) -> impl Iterator<Item = &str> {
    s.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
}

/// The items the application at `at` can reach by name, through the type function: what the
/// stage-0 run needs, and none of the aliases still waiting to be applied.
fn reachable(m: &j::Module, at: usize) -> Vec<j::Item> {
    let by_name: HashMap<&str, Vec<usize>> = {
        let mut h: HashMap<&str, Vec<usize>> = HashMap::new();
        for (i, it) in m.items.iter().enumerate() {
            if i != at && application(m, it).is_none() && !is_pending(it) {
                h.entry(it.kind.name()).or_default().push(i);
            }
        }
        h
    };
    let mut seen: HashSet<usize> = HashSet::new();
    let mut todo = vec![format!("{:?}", m.items[at])];
    while let Some(text) = todo.pop() {
        for w in words(&text) {
            for &i in by_name.get(w).into_iter().flatten() {
                if seen.insert(i) {
                    todo.push(format!("{:?}", m.items[i]));
                }
            }
        }
    }
    let mut ix: Vec<usize> = seen.into_iter().collect();
    ix.sort_unstable();
    ix.into_iter().map(|i| m.items[i].clone()).collect()
}

fn is_pending(it: &j::Item) -> bool {
    matches!(&it.kind, j::ItemKind::Type(j::TypeDef { rhs: j::TypeRhs::Alias(ts), .. })
        if matches!(ts.as_slice(), [j::Type::Call { .. }]))
}

fn core_type(t: &y::Type) -> R<j::Type> {
    let arg = |a: &y::TArg| {
        Ok(match a {
            y::TArg::Type(t) => j::TArg::Type(core_type(t)?),
            y::TArg::Nat(n) => j::TArg::Int(n.clone()),
        })
    };
    match t {
        y::Type::Named(n) => Ok(j::Type::Named {
            path: n.path.clone(),
            args: n.args.iter().map(arg).collect::<R<_>>()?,
        }),
        y::Type::Tuple(xs) => Ok(j::Type::Named {
            path: vec!["tuple".into()],
            args: xs
                .iter()
                .map(|x| Ok(j::TArg::Type(core_type(x)?)))
                .collect::<R<_>>()?,
        }),
        _ => unsupported("a reified field of this type"),
    }
}

fn hir_fields(fs: &[y::Field], at: Span) -> R<Vec<j::Field>> {
    fs.iter()
        .map(|f| {
            Ok(j::Field {
                decorators: f
                    .attrs
                    .iter()
                    .map(|a| j::Decorator {
                        path: vec![a.name.clone()],
                        args: None,
                    })
                    .collect(),
                name: f.name.clone(),
                ty: core_type(&f.ty)?,
                ty_span: at,
                default: None,
            })
        })
        .collect()
}

/// A reified declaration as the item a written one would be.
fn hir_item(d: &y::Item, from: &j::Item) -> R<j::Item> {
    let at = from.span;
    let kind = match &d.kind {
        y::ItemKind::Record(r) => j::ItemKind::Record(j::Record {
            name: r.path.join("."),
            tparams: Vec::new(),
            doc: None,
            fields: hir_fields(&r.fields, at)?,
        }),
        y::ItemKind::Sum(s) => {
            let variants = s
                .variants
                .iter()
                .map(|v| {
                    Ok(j::Variant {
                        decorators: Vec::new(),
                        name: v.name.clone(),
                        fields: hir_fields(v.fields.as_deref().unwrap_or(&[]), at)?,
                    })
                })
                .collect::<R<_>>()?;
            j::ItemKind::Type(j::TypeDef {
                name: s.path.join("."),
                tparams: Vec::new(),
                rhs: j::TypeRhs::Sum(variants),
                where_: None,
            })
        }
        // Desugar has run: a newtype is a record with the one field `value`.
        y::ItemKind::Newtype(n) => j::ItemKind::Record(j::Record {
            name: n.path.join("."),
            tparams: Vec::new(),
            doc: None,
            fields: vec![j::Field {
                decorators: Vec::new(),
                name: "value".into(),
                ty: core_type(&n.ty)?,
                ty_span: at,
                default: None,
            }],
        }),
        _ => return unsupported("a reified declaration of this kind"),
    };
    let mut decorators: Vec<j::Decorator> = d
        .attrs
        .iter()
        .map(|a| j::Decorator {
            path: vec![a.name.clone()],
            args: None,
        })
        .collect();
    if matches!(d.kind, y::ItemKind::Newtype(_)) {
        decorators.push(crate::desugar::newtype_mark());
    }
    Ok(j::Item {
        decorators,
        is_pub: from.is_pub,
        kind,
        span: at,
        name_span: from.name_span,
    })
}

fn names(it: &j::Item) -> Vec<String> {
    let mut ns = vec![it.kind.name().to_owned()];
    if let j::ItemKind::Type(j::TypeDef {
        rhs: j::TypeRhs::Sum(vs),
        ..
    }) = &it.kind
    {
        ns.extend(vs.iter().map(|v| v.name.clone()));
    }
    ns
}

/// Run `path(args)` on the stage-0 interpreter against the items `items` and give the type it
/// reifies, or none after reporting why there is none.
fn run(
    m: &j::Module,
    at: usize,
    path: &[String],
    args: &[j::Arg],
    cx: &mut Cx,
) -> R<Option<yan_interp::Reified>> {
    let it = &m.items[at];
    let name = it.kind.name().to_owned();
    if cx.untrusted {
        cx.err(
            "E261",
            it.span,
            &name,
            "stage 0 has no budget in a repository that is not trusted; run `yan trust`".into(),
        );
        return Ok(None);
    }
    let call = j::Expr {
        kind: j::ExprKind::Call(
            Box::new(j::Expr {
                kind: j::ExprKind::Name(path[0].clone()),
                span: it.span,
                paren: false,
            }),
            args.to_vec(),
        ),
        span: it.span,
        paren: false,
    };
    let fname = "__stage0_type_application".to_owned();
    let driver = j::FuncDef {
        name: fname.clone(),
        name_span: it.span,
        tparams: Vec::new(),
        params: Vec::new(),
        ret: Some(j::RetType {
            ty: Some(j::Type::Named {
                path: vec!["Type".into()],
                args: Vec::new(),
            }),
            fail: Vec::new(),
            from: Vec::new(),
        }),
        uses: None,
        body: Some(j::Body {
            doc: None,
            contracts: Vec::new(),
            stmts: vec![j::Stmt {
                kind: j::StmtKind::Return(Some(call)),
                span: it.span,
            }],
        }),
    };
    let mut pre = j::Module {
        header: Vec::new(),
        attrs: m.attrs.clone(),
        doc: None,
        items: reachable(m, at),
    };
    if let Some(what) = crate::s0caps::violation(&pre.items) {
        cx.err(
            "E268",
            it.span,
            &name,
            format!("stage 0 does not allow this: {what}; a type function may use only `alloc`, `embed` and `diag`"),
        );
        return Ok(None);
    }
    pre.items.push(j::Item {
        decorators: Vec::new(),
        is_pub: false,
        kind: j::ItemKind::Func(driver),
        span: it.span,
        name_span: it.span,
    });
    let opts = crate::Options {
        strict: cx.strict,
        release: cx.release,
        yn: cx.yn,
        ffi: cx.ffi,
        untrusted: cx.untrusted,
    };
    let (core, _, diags) = crate::elaborate_in(cx.file, cx.src, &pre, opts, None)?;
    if diags
        .iter()
        .any(|d| d.severity == yan_diag::Severity::Error)
    {
        // The function's own errors are reported where it is defined.
        return Ok(None);
    }
    let (code, msg) = match yan_interp::stage0(&core, &fname, stage0::limits(it)?) {
        Ok(Stage0::Type(r)) => return Ok(Some(r)),
        Ok(Stage0::Value(_) | Stage0::NotMaterializable(_)) => (
            "E262",
            format!("`{}` did not give a `Type`", path.join(".")),
        ),
        Err(yan_interp::Stop::Panic(p)) if p.kind == "compile_error" => {
            cx.err("E270", it.span, &name, p.message.clone());
            let notes = cx.diags.last_mut().map(|d| &mut d.notes).expect("pushed");
            notes.push(format!("raised in type function `{}`", path.join(".")));
            notes.extend(p.trace.iter().filter(|f| f.item != fname).map(|f| {
                format!(
                    "stage-0 backtrace: at {} {}:{}",
                    f.item, f.at.line, f.at.col
                )
            }));
            return Ok(None);
        }
        Err(yan_interp::Stop::Panic(p)) => (
            "E260",
            format!("stage-0 evaluation panicked: {}: {}", p.kind, p.message),
        ),
        Err(yan_interp::Stop::Limit(what)) => (
            "E261",
            format!("stage-0 evaluation exceeded its {what} limit; raise it with `@limit`"),
        ),
        Err(yan_interp::Stop::Unsupported(what)) => {
            return unsupported(format!("at stage 0: {what}"));
        }
    };
    cx.err(code, it.span, &name, msg);
    Ok(None)
}

/// Apply each `type X = f(args)` of `m` whose `f` is one of its type functions, in source
/// order: the reified declarations join the module, and `X` becomes an alias of the type
/// (E265 when a name is taken). A failed alias is dropped, so its uses are E200. Returns the failed aliases.
pub(crate) fn expand(m: &mut j::Module, cx: &mut Cx) -> R<Vec<String>> {
    let mut broken = order(m, cx);
    let mut memo: HashMap<String, String> = HashMap::new();
    let mut at = 0;
    while at < m.items.len() {
        let Some((alias, path, args)) = application(m, &m.items[at]) else {
            at += 1;
            continue;
        };
        let (alias, path, args) = (alias.to_owned(), path.to_vec(), args.to_vec());
        let span = m.items[at].span;
        let k = key(&path, &args).map_err(|u| u.at(span))?;
        let target = match memo.get(&k) {
            Some(t) => t.clone(),
            None => {
                let Some(r) = run(m, at, &path, &args, cx)? else {
                    broken.push(alias);
                    m.items.remove(at);
                    continue;
                };
                let y::Type::Named(top) = &r.ty else {
                    return unsupported("a reified type that is not nominal")
                        .map_err(|u| u.at(span));
                };
                let from = m.items[at].clone();
                let mut generated = Vec::new();
                for d in &r.decls {
                    generated.push(hir_item(d, &from).map_err(|u| u.at(span))?);
                }
                let taken: HashSet<String> = m
                    .items
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != at)
                    .flat_map(|(_, it)| names(it))
                    .collect();
                let clash = generated
                    .iter()
                    .flat_map(names)
                    .find(|n| taken.contains(n) && !generated.iter().any(|g| m.items.contains(g)));
                if let Some(n) = clash {
                    cx.err(
                        "E265",
                        span,
                        &alias,
                        format!(
                            "`{}` reifies `{n}`, a name already taken; rename it",
                            path.join(".")
                        ),
                    );
                    broken.push(alias);
                    m.items.remove(at);
                    continue;
                }
                let new: Vec<j::Item> = generated
                    .into_iter()
                    .filter(|g| !m.items.contains(g))
                    .collect();
                m.items.extend(new);
                let t = top.path.join(".");
                memo.insert(k, t.clone());
                t
            }
        };
        if alias == target {
            m.items.remove(at);
        } else {
            m.items[at].kind = j::ItemKind::Type(j::TypeDef {
                name: alias,
                tparams: Vec::new(),
                rhs: j::TypeRhs::Alias(vec![j::Type::Named {
                    path: vec![target],
                    args: Vec::new(),
                }]),
                where_: None,
            });
            at += 1;
        }
    }
    Ok(broken)
}

/// Put the applications in dependency order (an argument that names another application's alias
/// needs it applied first), and drop the ones that depend on their own result with E261.
fn order(m: &mut j::Module, cx: &mut Cx) -> Vec<String> {
    let slots: Vec<usize> = (0..m.items.len())
        .filter(|&i| application(m, &m.items[i]).is_some())
        .collect();
    let names: Vec<String> = slots
        .iter()
        .map(|&i| m.items[i].kind.name().to_owned())
        .collect();
    let deps: Vec<Vec<usize>> = slots
        .iter()
        .map(|&i| {
            let (_, _, args) = application(m, &m.items[i]).expect("an application");
            let text = format!("{args:?}");
            let used: HashSet<&str> = words(&text).collect();
            (0..names.len())
                .filter(|&k| used.contains(names[k].as_str()))
                .collect()
        })
        .collect();
    let mut done: Vec<usize> = Vec::new();
    while done.len() < slots.len() {
        let next = (0..slots.len())
            .find(|k| !done.contains(k) && deps[*k].iter().all(|d| done.contains(d)));
        match next {
            Some(k) => done.push(k),
            None => break,
        }
    }
    let cyclic: Vec<usize> = (0..slots.len()).filter(|k| !done.contains(k)).collect();
    let items: Vec<j::Item> = slots.iter().map(|&i| m.items[i].clone()).collect();
    for (pos, &k) in done.iter().chain(&cyclic).enumerate() {
        m.items[slots[pos]] = items[k].clone();
    }
    let mut broken = Vec::new();
    for &k in &cyclic {
        cx.err(
            "E261",
            items[k].span,
            &names[k],
            format!(
                "`{}` depends on its own result; stage-0 evaluation cannot start",
                names[k]
            ),
        );
        broken.push(names[k].clone());
    }
    for &i in slots[done.len()..].iter().rev() {
        m.items.remove(i);
    }
    broken
}

/// D5: a function returning `Type` is called only at stage 0, from another one or under `#`.
pub(crate) fn stage0_only(g: &crate::Globals, item: &str, callee: &str, at: Span, cx: &mut Cx) {
    let stage0 =
        item.starts_with("__stage0_") || g.fns.get(item).is_some_and(|s| s.ret == crate::Ty::Type);
    if !stage0 {
        cx.err(
            "E262",
            at,
            item,
            format!("`{callee}` returns a `Type`, which exists only at stage 0; call it in a type position or under `#`"),
        );
    }
}

/// D6: Yan Agent applies type functions but does not define one (14, L5).
pub(crate) fn surface(m: &j::Module, cx: &mut Cx) {
    if cx.yn {
        return;
    }
    for it in &m.items {
        if let j::ItemKind::Func(f) = &it.kind
            && type_fn(m, std::slice::from_ref(&f.name)).is_some()
        {
            cx.err(
                "E283",
                it.name_span,
                &f.name,
                format!("`{}` returns a `Type`; Yan Agent applies type functions but does not define them, so define it in Yan", f.name),
            );
        }
    }
}
