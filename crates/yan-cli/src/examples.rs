//! `ex` lines as tests (R309) and as spec-gate checks (21.1). Each `ex E` of `def f` becomes a
//! function appended to the module text, `def ex_of_f_k(): assert E`, so the checker types it and
//! the interpreter runs it without either learning about `ex`. With `pre`, the spec gate also
//! gets `def pre_of_f_k(): p = arg ... assert pre`, which holds when the example's arguments
//! satisfy the precondition. Diagnostics in the appended text are moved to the `ex` line.

use yan_diag::{Diagnostic, Severity, Span};
use yan_yag::ast::{ArgKind, Contract, ExprKind, FuncDef, ItemKind, Param};

pub enum Kind {
    /// The example holds.
    Holds,
    /// The example's arguments satisfy `pre`.
    Pre,
}

/// One appended function.
pub struct Example {
    pub kind: Kind,
    /// The function it belongs to.
    pub func: String,
    /// 1 for the first `ex` line of `func`.
    pub k: usize,
    /// The appended function's name.
    pub name: String,
    /// Where the `ex` expression starts in the written text.
    pub span: Span,
    /// The `ex` expression, printed.
    pub text: String,
    first: u32,
    last: u32,
}

impl Example {
    /// The test id `yan test` reports: `module.f#ex1`.
    pub fn id(&self, module: &str) -> String {
        format!("{module}.{}#ex{}", self.func, self.k)
    }
}

/// `src` with every `ex` line appended as a function, and the appended functions. `pre` adds
/// the precondition checks. A module that does not parse comes back unchanged.
pub fn augment(file: &str, src: &str, pre: bool) -> (String, Vec<Example>) {
    let (Some(m), _) = yan_yag::parse(file, src, false) else {
        return (src.to_owned(), Vec::new());
    };
    let text = |e: &yan_yag::ast::Expr| yan_yag::print_expr(e, 0);
    let mut out = src.trim_end().to_owned();
    out.push('\n');
    let mut next = out.lines().count() as u32 + 1;
    let mut found = Vec::new();
    let mut push = |out: &mut String, kind, f: &str, k, e: &yan_yag::ast::Expr, body: String| {
        let name = match kind {
            Kind::Holds => format!("ex_of_{f}_{k}"),
            Kind::Pre => format!("pre_of_{f}_{k}"),
        };
        let item = format!("\ndef {name}():\n{body}");
        let first = next + 1;
        next += item.lines().count() as u32;
        out.push_str(&item);
        found.push(Example {
            kind,
            func: f.to_owned(),
            k,
            name,
            span: e.span,
            text: yan_yag::print_expr(e, 0),
            first,
            last: next - 1,
        });
    };
    for it in &m.items {
        let ItemKind::Func(f) = &it.kind else {
            continue;
        };
        let Some(body) = &f.body else { continue };
        let pres: Vec<String> = body
            .contracts
            .iter()
            .filter_map(|c| match c {
                Contract::Pre(e) => Some(format!("({})", text(e))),
                _ => None,
            })
            .collect();
        let exs = body.contracts.iter().filter_map(|c| match c {
            Contract::Ex { expr, with } => Some((expr, with)),
            _ => None,
        });
        for (k, (e, with)) in exs.enumerate() {
            let k = k + 1;
            let mut indent = "    ".to_owned();
            let mut held = String::new();
            for (n, h) in with {
                held.push_str(&format!("{indent}with {n} = {}:\n", text(h)));
                indent.push_str("    ");
            }
            held.push_str(&format!("{indent}assert {}\n", text(e)));
            push(&mut out, Kind::Holds, &f.name, k, e, held);
            if pre
                && !pres.is_empty()
                && let Some(binds) = bindings(f, e, &text)
            {
                let mut body: String = binds
                    .iter()
                    .map(|(n, v)| format!("    {n} = {v}\n"))
                    .collect();
                body.push_str(&format!("    assert {}\n", pres.join(" and ")));
                push(&mut out, Kind::Pre, &f.name, k, e, body);
            }
        }
    }
    (out, found)
}

/// The parameter bindings of the call to `f` that `ex` makes, as `f(args)` or `f(args) == r`;
/// none when the example is another shape, passes `&x`, or leaves out a parameter that has no
/// default.
fn bindings(
    f: &FuncDef,
    ex: &yan_yag::ast::Expr,
    text: &dyn Fn(&yan_yag::ast::Expr) -> String,
) -> Option<Vec<(String, String)>> {
    let call = match &ex.kind {
        ExprKind::Compare("==", l, _) => l,
        _ => ex,
    };
    let ExprKind::Call(callee, args) = &call.kind else {
        return None;
    };
    if !matches!(&callee.kind, ExprKind::Name(n) if *n == f.name) {
        return None;
    }
    let params: Vec<(&str, Option<&yan_yag::ast::Expr>)> = f
        .params
        .iter()
        .filter_map(|p| match p {
            Param::Value { name, default, .. } => Some((name.as_str(), default.as_ref())),
            _ => None,
        })
        .collect();
    let mut given: Vec<Option<String>> = vec![None; params.len()];
    let mut pos = 0;
    for a in args {
        let ArgKind::Expr(e) = &a.kind else {
            return None;
        };
        let k = match &a.name {
            Some(n) => params.iter().position(|(p, _)| p == n)?,
            None => {
                pos += 1;
                pos - 1
            }
        };
        *given.get_mut(k)? = Some(text(e));
    }
    // An empty literal has no type of its own as a local; the parameter's type gives it one.
    let types = f.params.iter().filter_map(|p| match p {
        Param::Value { ty, .. } => Some(yan_yag::print_type(ty)),
        _ => None,
    });
    params
        .iter()
        .zip(given)
        .zip(types)
        .map(|(((n, d), g), t)| {
            let v = g.or_else(|| d.map(text))?;
            let empty = matches!(v.as_str(), "[]" | "{}") && f.tparams.is_empty();
            let n = if empty {
                format!("{n}: {t}")
            } else {
                (*n).to_owned()
            };
            Some((n, v))
        })
        .collect()
}

/// Diagnostics of the augmented text, as the written text reports them: errors in an appended
/// function move to its `ex` line under the item of the function, and its other diagnostics go.
pub fn remap(diags: Vec<Diagnostic>, found: &[Example]) -> Vec<Diagnostic> {
    diags
        .into_iter()
        .filter_map(|mut d| {
            let Some(x) = found
                .iter()
                .find(|x| (x.first..=x.last).contains(&d.span.line) || d.item == x.name)
            else {
                return Some(d);
            };
            if d.severity != Severity::Error {
                return None;
            }
            d.span = x.span;
            d.item.clone_from(&x.func);
            let what = match x.kind {
                Kind::Holds => "`ex`",
                Kind::Pre => "`pre` for an `ex` line",
            };
            d.notes.push(format!("in {what} of `{}`", x.func));
            Some(d)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::augment;
    use crate::test;

    #[test]
    fn pre_check_types_empty_literals_by_parameter() {
        let src = "def f(xs: list[int], d: dict[str, int]) -> int:\n    pre len(xs) >= 0\n    ex f([], {}) == 0\n    return 0\n";
        let (text, _) = augment("m.yag", src, true);
        assert!(text.contains("    xs: list[int] = []\n    d: dict[str, int] = {}\n"));
    }

    #[test]
    fn ex_lines_run_and_compare_outcomes() {
        let dir = std::env::temp_dir().join(format!("yan-ex-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = "class P:\n    x: int\n\ndef f(n: int) -> P fail str:\n    ex f(0) == Ok(P(0))\n    ex Err(\"no\") == f(1)\n    ex f(1) == Ok(P(1))\n    if n > 0:\n        fail \"no\"\n    return P(n)\n";
        std::fs::write(dir.join("m.yag"), src).unwrap();
        let r = test::run(&dir, None, false, None, None).unwrap();
        let v: Vec<(&str, bool)> = r
            .tests
            .iter()
            .map(|t| (t.id.as_str(), t.verdict == test::Verdict::Pass))
            .collect();
        assert_eq!(
            v,
            [("m.f#ex1", true), ("m.f#ex2", true), ("m.f#ex3", false)]
        );
    }
}
