//! The decorators Jian may apply: `@derive` (10.4), `@expect` (18.2), `@deprecated` (20.5),
//! `@messages` (17.5) and `@inline`.

use yong_diag::Span;
use yong_hir as j;

use crate::{Cx, R, unsupported};

/// std traits with a `derived` body (10.4); `From` derives on sums (R432).
const DERIVABLE: [&str; 8] = [
    "Eq", "Ord", "Hash", "Show", "Default", "Wire", "Num", "From",
];
/// The compiler derives these itself (6.2).
const AUTO: [&str; 3] = ["Send", "Sync", "Freeze"];

/// Checks `it`'s decorators after the item was lowered; its diagnostics start at `start`.
pub(crate) fn check(it: &j::Item, start: usize, cx: &mut Cx) -> R<()> {
    let item = it.kind.name().to_owned();
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
                    } else if DERIVABLE.contains(&t.as_str()) {
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
                let j::ItemKind::Class(c) = &it.kind else {
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
            "deprecated" | "inline" => {}
            other => return unsupported(format!("decorator `@{other}`")),
        }
    }
    Ok(())
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
