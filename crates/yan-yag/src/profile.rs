//! The data profile (17.3, R826, R827): which Yan Agent files are plain data. Checking is syntactic
//! and linear; no name is resolved, so tools read a data file without the elaborator.

use ast::{ArgKind, Expr, ExprKind, ItemKind, Lit, Module};

use yan_diag::Span;

use crate::ast;

/// Every place `m` leaves the profile, as `(span, reason)` pairs (E144).
pub fn violations(m: &Module) -> Vec<(Span, String)> {
    let mut out = Vec::new();
    if !m.attrs.is_empty() {
        out.push((Span::default(), "a module decorator is not data".into()));
    }
    for it in &m.items {
        match &it.kind {
            ItemKind::Const { value, .. } if it.decorators.is_empty() && !it.is_pub => {
                value_in(value, &mut out)
            }
            ItemKind::Const { .. } => {
                out.push((it.span, "a data constant has no decorator or `pub`".into()))
            }
            _ => out.push((
                it.span,
                "only `NAME: Type = value` constants are data".into(),
            )),
        }
    }
    out
}

fn value_in(e: &Expr, out: &mut Vec<(Span, String)>) {
    match &e.kind {
        ExprKind::Lit(Lit::Int(_) | Lit::Float(_) | Lit::Bool(_) | Lit::None) => {}
        ExprKind::Lit(Lit::Str { prefix, .. }) if matches!(prefix.as_str(), "" | "b") => {}
        ExprKind::Unary("-", x) if matches!(x.kind, ExprKind::Lit(Lit::Int(_) | Lit::Float(_))) => {
        }
        ExprKind::Tuple(xs) | ExprKind::List(xs) | ExprKind::Set(xs) => {
            xs.iter().for_each(|x| value_in(x, out))
        }
        ExprKind::Dict(kv) => kv.iter().for_each(|(k, v)| {
            value_in(k, out);
            value_in(v, out);
        }),
        ExprKind::Call(f, args) if constructor_path(f) => {
            for a in args {
                match (&a.name, &a.kind) {
                    (Some(_), ArgKind::Expr(x)) => value_in(x, out),
                    _ => out.push((a.span, "a constructor argument must be named".into())),
                }
            }
        }
        ExprKind::Call(..) => out.push((e.span, "a call is not data".into())),
        ExprKind::Binary(..) | ExprKind::Unary(..) | ExprKind::Not(_) | ExprKind::Compare(..) => {
            out.push((e.span, "an operator is not data".into()))
        }
        ExprKind::Name(n) => out.push((
            e.span,
            format!("`{n}` refers to a name; data holds literals only"),
        )),
        _ => out.push((e.span, "this expression is not a literal".into())),
    }
}

/// `Name` or `Type.Variant`: a path of plain names, resolved against the declared type later.
fn constructor_path(f: &Expr) -> bool {
    match &f.kind {
        ExprKind::Name(_) => true,
        ExprKind::Attr(base, ..) => constructor_path(base),
        _ => false,
    }
}

#[cfg(test)]
mod selftest {
    use super::*;

    fn reasons(src: &str) -> Vec<String> {
        let (m, _) = crate::parse("data.yag", src, false);
        violations(&m.expect("parses"))
            .into_iter()
            .map(|v| v.1)
            .collect()
    }

    #[test]
    fn literals_and_constructors_are_data() {
        let ok = "\"\"\"doc\"\"\"\n# c\nA: int = -1\nB: Shape = Shape.Circle(r=1.5, tags=[\"a\", b\"b\"], m={1: (2, 3)}, n=None, t=True)\n";
        assert_eq!(reasons(ok), Vec::<String>::new());
    }

    #[test]
    fn everything_else_is_e144() {
        assert_eq!(reasons("A: int = 1 + 2\n").len(), 1);
        assert_eq!(reasons("A: str = str(1)\n").len(), 1);
        assert_eq!(reasons("A: int = 1\nB: int = A\n").len(), 1);
        assert_eq!(reasons("A: P = P(1)\n").len(), 1);
        assert_eq!(reasons("def f() -> int:\n    return 1\n").len(), 1);
        assert_eq!(reasons("A: str = f\"{1}\"\n").len(), 1);
        assert_eq!(reasons("A: list[int] = [x for x in y]\n").len(), 1);
    }
}
