//! The decorators the elaborator applies: `@derive` (10.4), `@expect` (18.2), `@deprecated`
//! (20.5), `@messages` (17.5), `@export` and `@layout(c)` (12), `@forbid(alloc)` (5.7),
//! `@limit` (10.4) and `@inline`.

use yan_diag::Span;
use yan_hir as j;

use crate::stdlib::STD;
use crate::ty::Ty;
use crate::{Cx, Globals, R, ffi, lower, unsupported};

/// std traits with a `derived` body (10.4); `From` derives on sums (R432).
const DERIVABLE: [&str; 9] = [
    "Eq", "Ord", "Hash", "Show", "Default", "Wire", "Num", "From", "Cli",
];
/// Derives whose fields must implement the trait too (E282).
pub(crate) const FIELD_TRAITS: [&str; 5] = ["Eq", "Ord", "Hash", "Show", "Wire"];
/// The compiler derives these itself (6.2).
const AUTO: [&str; 3] = ["Send", "Sync", "Freeze"];

/// Whether `it` has `@derive(name)`.
pub(crate) fn derives(it: &j::Item, name: &str) -> bool {
    it.decorators.iter().any(|d| {
        d.path == ["derive"]
            && d.args.as_deref().unwrap_or_default().iter().any(|a| {
                matches!(&a.kind, j::ArgKind::Expr(j::Expr { kind: j::ExprKind::Name(n), .. }) if n == name)
            })
    })
}

/// Checks `it`'s decorators after the item was lowered; its diagnostics start at `start`.
pub(crate) fn check(g: &Globals, it: &j::Item, start: usize, cx: &mut Cx) -> R<()> {
    let item = it.kind.name().to_owned();
    deny(it, &item, start, cx);
    for (k, d) in it.decorators.iter().enumerate() {
        let args = d.args.as_deref().unwrap_or_default();
        match d.path.join(".").as_str() {
            "derive" => {
                for a in args {
                    let j::ArgKind::Expr(j::Expr {
                        kind: j::ExprKind::Name(t),
                        ..
                    }) = &a.kind
                    else {
                        return unsupported("a `@derive` argument that is not a trait name");
                    };
                    let why = if AUTO.contains(&t.as_str()) {
                        "is an auto trait; the compiler derives it"
                    } else if t == "Cli" {
                        if let Err(msg) = crate::cli::schema(g, &item) {
                            cx.err("E269", a.span, &item, msg);
                        }
                        continue;
                    } else if DERIVABLE.contains(&t.as_str()) {
                        if FIELD_TRAITS.contains(&t.as_str()) {
                            fields(g, it, &item, t, cx);
                        }
                        continue;
                    } else {
                        "has no `derived` body"
                    };
                    cx.err("E269", a.span, &item, format!("`{t}` {why}"));
                }
            }
            "expect" => {
                let Some(j::ArgKind::Expr(j::Expr {
                    kind: j::ExprKind::Name(code),
                    ..
                })) = args.first().map(|a| &a.kind)
                else {
                    return unsupported("an `@expect` without a code");
                };
                let before = cx.diags.len();
                let mut k0 = 0;
                cx.diags.retain(|d| {
                    k0 += 1;
                    k0 <= start || d.code != *code
                });
                if cx.diags.len() == before {
                    cx.warn(
                        "W810",
                        decorator_span(it, k),
                        &item,
                        format!("`@expect({code})` suppresses nothing"),
                    );
                }
            }
            "messages" => {
                let j::ItemKind::Record(c) = &it.kind else {
                    return unsupported("`@messages` on an item that is not a class");
                };
                for f in &c.fields {
                    if let Some(
                        e @ j::Expr {
                            kind: j::ExprKind::Lit(j::Lit::Str { value, .. }),
                            ..
                        },
                    ) = &f.default
                        && let Some((code, msg)) = message_error(value)
                    {
                        cx.err(code, e.span, &item, format!("`{}`: {msg}", f.name));
                    }
                }
            }
            "export" => ffi::export(g, it, cx)?,
            "forbid" => {
                for a in args {
                    match &a.kind {
                        j::ArgKind::Expr(j::Expr {
                            kind: j::ExprKind::Name(n),
                            ..
                        }) if n == "alloc" => forbid_alloc(it, cx),
                        _ => return unsupported("`@forbid` other than `alloc`"),
                    }
                }
            }
            "layout" if g.layout_c.contains(&item) => {}
            "layout" => return unsupported("`@layout` other than `@layout(c)` on a record"),
            "reentrant" => reentrant(it, cx),
            "deny" => {}
            "deprecated" | "inline" | "limit" | "test_support" => {}
            "capability" if matches!(it.kind, j::ItemKind::Trait(_)) => {}
            // Read with `@derive(Cli)` (RFC 0019 D14).
            "help" if g.clis.contains_key(&item) => {}
            "help" => cx.err(
                "E269",
                decorator_span(it, k),
                &item,
                "`@help` needs `@derive(Cli)`".into(),
            ),
            other => return unsupported(format!("decorator `@{other}`")),
        }
    }
    Ok(())
}

/// The code named by decorator argument `a`, if it is a bare name.
fn code_of(a: &j::Arg) -> Option<&str> {
    match &a.kind {
        j::ArgKind::Expr(j::Expr {
            kind: j::ExprKind::Name(n),
            ..
        }) => Some(n),
        _ => None,
    }
}

/// Whether `code` is a W8xx lint code.
fn is_lint(code: &str) -> bool {
    code.len() == 4 && code.starts_with("W8") && code[2..].bytes().all(|b| b.is_ascii_digit())
}

/// `@deny(code)` (18.2, R797): the lint is an error in the item; E277 for an `@expect` of the
/// same code, E278 for a code that is not a W8xx lint.
fn deny(it: &j::Item, item: &str, start: usize, cx: &mut Cx) {
    for (k, d) in it.decorators.iter().enumerate() {
        if d.path != ["deny"] {
            continue;
        }
        for code in d
            .args
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter_map(code_of)
        {
            if !is_lint(code) {
                let at = decorator_span(it, k);
                cx.err(
                    "E278",
                    at,
                    item,
                    format!("`@deny({code})` names a code that is not a W8xx lint"),
                );
                continue;
            }
            for (j, e) in it.decorators.iter().enumerate() {
                let expects = e.path == ["expect"]
                    && e.args
                        .as_deref()
                        .unwrap_or_default()
                        .first()
                        .and_then(code_of)
                        == Some(code);
                if expects {
                    let at = decorator_span(it, j);
                    cx.err(
                        "E277",
                        at,
                        item,
                        format!("`@expect({code})` inside an item that `@deny({code})`"),
                    );
                }
            }
            for dg in cx.diags.iter_mut().skip(start) {
                if dg.code == code && dg.severity == yan_diag::Severity::Warning {
                    dg.severity = yan_diag::Severity::Error;
                }
            }
        }
    }
}

/// E900: every expression that allocates in a function marked `@forbid(alloc)` (5.7, R447).
fn forbid_alloc(it: &j::Item, cx: &mut Cx) {
    let j::ItemKind::Func(f) = &it.kind else {
        return;
    };
    let mut found = Vec::new();
    let mut stack: Vec<&[j::Stmt]> = f.body.iter().map(|b| b.stmts.as_slice()).collect();
    while let Some(stmts) = stack.pop() {
        for s in stmts {
            for e in lower::stmt_exprs(s) {
                lower::walk(e, &mut |x| {
                    use j::ExprKind as K;
                    if matches!(
                        x.kind,
                        K::List(_) | K::Set(_) | K::Dict(_) | K::Comp { .. } | K::FStr { .. }
                    ) {
                        found.push(x.span);
                    }
                });
            }
            stack.extend(lower::sub_blocks(s));
        }
    }
    found.sort_by_key(|s| (s.line, s.col));
    for at in found {
        cx.err(
            "E900",
            at,
            &f.name,
            format!("`{}` is `@forbid(alloc)` and this allocates", f.name),
        );
    }
}

/// E715: in a `@reentrant` handler, a local bound from actor state and used after a suspension
/// (9.6, R397). Only top-level statements are followed.
fn reentrant(it: &j::Item, cx: &mut Cx) {
    let j::ItemKind::Func(f) = &it.kind else {
        return;
    };
    let Some(j::Param::Value { name: state, .. }) = f.params.first() else {
        return;
    };
    let mut reads: Vec<(String, Span)> = Vec::new();
    let mut suspended = false;
    let mut flagged = Vec::new();
    for s in f.body.iter().flat_map(|b| &b.stmts) {
        let mut used = Vec::new();
        let mut suspends = false;
        let mut state_read = None;
        for e in lower::stmt_exprs(s) {
            lower::walk(e, &mut |x| match &x.kind {
                j::ExprKind::Name(n) => used.push(n.clone()),
                j::ExprKind::Attr(b, _, _) if matches!(&b.kind, j::ExprKind::Name(n) if n == state) =>
                {
                    state_read.get_or_insert(x.span);
                }
                j::ExprKind::Call(c, _) => {
                    if let j::ExprKind::Attr(b, m, _) = &c.kind {
                        suspends |=
                            m == "sleep" && matches!(&b.kind, j::ExprKind::Name(n) if n == "time");
                    }
                }
                _ => {}
            });
        }
        if suspended {
            for (n, at) in &reads {
                if used.contains(n) && !flagged.contains(at) {
                    flagged.push(*at);
                }
            }
        }
        suspended |= suspends;
        if let (j::StmtKind::Assign { target, .. }, Some(at)) = (&s.kind, state_read)
            && let j::ExprKind::Name(n) = &target.kind
        {
            reads.push((n.clone(), at));
        }
    }
    for at in flagged {
        cx.err(
            "E715",
            at,
            &f.name,
            format!(
                "`{}` is `@reentrant` and keeps a value read from state across a suspension",
                f.name
            ),
        );
    }
}

/// Decorators sit on the lines just above their item, one per line.
fn decorator_span(it: &j::Item, k: usize) -> Span {
    let line = it.span.line - (it.decorators.len() - k) as u32;
    Span {
        line,
        col: 1,
        end_line: line,
        end_col: 2,
    }
}

/// The first MessageFormat 2 problem in `m` that 17.5 names: unbalanced braces (E551),
/// unbalanced markup (E555), or a `.match` with no `*` arm that misses a plural category of
/// the source locale (E554). The source locale is English, whose categories are one and other.
fn message_error(m: &str) -> Option<(&'static str, &'static str)> {
    let mut depth = 0i32;
    let mut tags: Vec<String> = Vec::new();
    let mut chars = m.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '{' => {
                depth += 1;
                let rest = &m[i + 1..];
                let body = &rest[..rest.find('}').unwrap_or(rest.len())];
                if let Some(t) = body.strip_prefix('#') {
                    if !t.trim_end().ends_with('/') {
                        tags.push(name_of(t));
                    }
                } else if let Some(t) = body.strip_prefix('/')
                    && tags.pop() != Some(name_of(t))
                {
                    return Some(("E555", "markup closes a tag it did not open"));
                }
            }
            '}' => {
                depth -= 1;
                if depth < 0 {
                    return Some(("E551", "`}` closes nothing"));
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Some(("E551", "a `{` is never closed"));
    }
    if !tags.is_empty() {
        return Some(("E555", "markup opens a tag it never closes"));
    }
    if m.trim_start().starts_with('.') && m.contains(".match") {
        let keys: Vec<&str> = m
            .lines()
            .skip_while(|l| !l.trim_start().starts_with(".match"))
            .skip(1)
            .filter_map(|l| l.split("{{").next())
            .flat_map(str::split_whitespace)
            .collect();
        let covered = keys.contains(&"*") || (keys.contains(&"one") && keys.contains(&"other"));
        if !covered {
            return Some((
                "E554",
                "the `.match` misses a plural category and has no `*` arm",
            ));
        }
    }
    None
}

fn name_of(t: &str) -> String {
    t.split(|c: char| c.is_whitespace() || c == '/')
        .next()
        .unwrap_or("")
        .to_owned()
}

/// E282 once per field of `it` whose type does not implement the derived trait `tr`, at the
/// field's type annotation (R872, R873).
fn fields(g: &Globals, it: &j::Item, item: &str, tr: &str, cx: &mut Cx) {
    let owned: Vec<(String, &[crate::Field])> = match &it.kind {
        j::ItemKind::Record(c) => g
            .records
            .get(&c.name)
            .map(|fs| vec![(c.name.clone(), fs.as_slice())])
            .unwrap_or_default(),
        j::ItemKind::Type(t) => g
            .sums
            .get(&t.name)
            .map(|vs| {
                vs.iter()
                    .map(|(v, fs)| (v.clone(), fs.as_slice()))
                    .collect()
            })
            .unwrap_or_default(),
        _ => return,
    };
    for (owner, fs) in owned {
        for f in fs {
            if tr == "Wire" {
                if matches!(&f.ty, Ty::Option(e) if matches!(**e, Ty::Option(_))) {
                    cx.err(
                        "E330",
                        f.ty_span,
                        item,
                        format!(
                            "`{owner}.{}` is a nested optional, which json cannot represent",
                            f.name
                        ),
                    );
                } else if f.name == "tag" && matches!(&it.kind, j::ItemKind::Type(_)) {
                    cx.err(
                        "E330",
                        f.ty_span,
                        item,
                        format!("`{owner}.tag` clashes with the json tag of `{item}`"),
                    );
                }
            }
            let Err(msg) = field_ok(g, &owner, f, tr) else {
                continue;
            };
            cx.err("E282", f.ty_span, item, msg);
            if tr == "Wire"
                && has_capability(&f.ty)
                && let Some(d) = cx.diags.last_mut()
            {
                d.notes.push("capabilities are never `Wire` (17.4)".into());
            }
        }
    }
}

fn field_ok(g: &Globals, owner: &str, f: &crate::Field, tr: &str) -> Result<(), String> {
    if tr == "Wire" {
        return crate::wire::schema(g, owner, f, &f.ty, &mut Vec::new()).map(|_| ());
    }
    match offender(g, &f.ty, tr) {
        None => Ok(()),
        Some(t) => Err(format!(
            "`{owner}.{}` has type `{t}`, which does not {} `{tr}`",
            f.name,
            if matches!(t, Ty::Data(n) if !STD.caps.contains(n)) {
                "derive"
            } else {
                "implement"
            }
        )),
    }
}

/// The first type inside `t` that cannot implement `tr`. Std types not listed here are assumed to.
fn offender<'a>(g: &Globals, t: &'a Ty, tr: &str) -> Option<&'a Ty> {
    let all = |ts: &'a [Ty]| ts.iter().find_map(|t| offender(g, t, tr));
    match t {
        Ty::Fn(..) => Some(t),
        Ty::Data(n) if STD.caps.contains(n) => Some(t),
        Ty::Data(n) if g.std_records.contains(n) || g.std_sums.contains(n) => None,
        Ty::Data(n) => (!g.derived.contains(&(tr.to_owned(), n.clone()))
            && !g.impls.contains(&(tr.to_owned(), n.clone())))
        .then_some(t),
        Ty::Float | Ty::Fixed("f32" | "f64") if tr == "Hash" => Some(t),
        Ty::Set(_) | Ty::Dict(..) if matches!(tr, "Ord" | "Hash") => Some(t),
        Ty::List(e) | Ty::Set(e) | Ty::Option(e) => offender(g, e, tr),
        Ty::Dict(k, v) => offender(g, k, tr).or_else(|| offender(g, v, tr)),
        Ty::Tuple(ts) => all(ts),
        _ => None,
    }
}

fn has_capability(t: &Ty) -> bool {
    match t {
        Ty::Data(n) => STD.caps.contains(n),
        Ty::List(e) | Ty::Set(e) | Ty::Option(e) => has_capability(e),
        Ty::Dict(k, v) => has_capability(k) || has_capability(v),
        Ty::Tuple(ts) => ts.iter().any(has_capability),
        _ => false,
    }
}
