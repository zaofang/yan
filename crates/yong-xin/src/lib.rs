//! Xin: lexer (INDENT/DEDENT), parser, syntactic lenient normalizations and canonical printer.

pub use yong_hir as ast;
mod lexer;
mod parser;
mod printer;
mod resugar;

pub use printer::{expr as print_expr, print};
pub use resugar::resugar;
use yong_diag::{Diagnostic, Severity};

/// Parse a `.xin` file. `strict` disables the lenient reader (14.4): its deviations become
/// errors where 13 names one, and its notes are not reported.
pub fn parse(file: &str, src: &str, strict: bool) -> (Option<ast::Module>, Vec<Diagnostic>) {
    let (m, issues) = parser::parse(src, strict);
    let diags = issues
        .into_iter()
        .filter(|i| !(strict && i.severity == Severity::Note))
        .map(|i| {
            let line = src
                .lines()
                .nth(i.span.line.saturating_sub(1) as usize)
                .unwrap_or("");
            let from = (i.span.col.saturating_sub(1) as usize).min(line.len());
            let to = if i.span.end_line == i.span.line {
                (i.span.end_col.saturating_sub(1) as usize).clamp(from, line.len())
            } else {
                line.len()
            };
            let mut d = Diagnostic::error(
                i.code,
                file,
                i.span,
                line.get(from..to).unwrap_or(""),
                i.message,
            );
            d.severity = i.severity;
            d
        })
        .collect();
    (m, diags)
}

#[cfg(test)]
mod selftest {
    use super::*;

    fn roundtrip(src: &str) -> String {
        let (m, d) = parse("t.xin", src, false);
        let errors: Vec<_> = d
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.render_line(None))
            .collect();
        assert!(errors.is_empty(), "{errors:#?}");
        let once = print(&m.unwrap());
        let (m2, d2) = parse("t.xin", &once, false);
        let errors: Vec<_> = d2
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.render_line(None))
            .collect();
        assert!(errors.is_empty(), "reparse failed:\n{once}\n{errors:?}");
        assert_eq!(print(&m2.unwrap()), once);
        once
    }

    const SAMPLE: &str = r#"# header
"""Module doc."""

class Point:
    x: int
    y: int = 0

type Shape = Circle(r: float) | Dot()

type Err = IoErr | Timeout

type Meters = newtype[float] where value >= 0.0

trait Show[T]:
    def show(x: T) -> str: ...

impl Show[int] for Point:
    def show(x: Point) -> str:
        return f"{x.x}, {{literal}} {x.y:>4}"

limit: int = 10

@inline
pub def f[T: Eq + Hash, n: int](xs: list[int], var acc: int, take s: str, /, **kw: Expr[int]) -> int fail IoErr | Timeout uses io, Kv as cache:
    """Doc."""
    pre len(xs) > 0
    post out >= 0
    ex f([1], &a, "s") == Ok(1) with io = fake_io()
    decreases len(xs)
    a, b = 1, 2
    total: dict[str, tuple[float, int]] = {}
    acc += -x ** 2 if a else (b if c else d)
    y = d.get(k) else err -> 0
    z = q else return
    w = (a and b) or not c
    v = [x * 2 for x in xs if x > 0]
    u = par [g(x)? for x in xs]
    s2 = {k: v for k, v in d.items()}
    t = {1, 2}
    sl = xs[1:2:3]
    lam = lambda a, b: a + b
    g(dst=&acc, src=xs[0])
    if a: return 1
    elif b:
        pass
    else:
        fail IoErr()
    while i < n:
        invariant i >= 0
        i += 1
    for k, v in d.items():
        continue
    match s:
        case Circle(r=r) if r > 0.0:
            return 1
        case Dot() as d:
            return 2
        case (1, _):
            return 3
    with io = fake_io():
        io.print("x")
    with deadline(10 * ms):
        pass
    par:
        a1 = h(1)
        b1 = h(2)
    par concurrent(limit=4) for x in xs:
        h(x)
    assert a is not None
    ghost g1: int = 1
    become f(xs, &acc, s)

def opt(x: int | None, f: Callable[[int], int, fail(E), uses(io)]) -> int | None:
    return x
"#;

    #[test]
    fn every_production_round_trips() {
        let printed = roundtrip(SAMPLE);
        assert!(printed.starts_with("# header\n\n\"\"\"Module doc.\"\"\"\n"));
        assert_eq!(roundtrip(&printed), printed);
    }

    fn codes(src: &str, strict: bool) -> Vec<(String, u32, u32)> {
        parse("t.xin", src, strict)
            .1
            .into_iter()
            .map(|d| (d.code, d.span.line, d.span.col))
            .collect()
    }

    #[test]
    fn errors_recover_at_items() {
        let src = "def f() -> int:\n    return (1 2)\n\ndef g() -> int:\n    return [1 2]\n";
        assert_eq!(
            codes(src, false),
            [("E100".into(), 2, 15), ("E100".into(), 5, 15)]
        );
    }

    #[test]
    fn types_are_spelled_as_yong_in_hir() {
        let src = "def f(x: int, y: list[str] | None, z: sorted_dict[int8, bytes]) -> dict[str, float]:\n    return {}\n";
        let m = parse("t.xin", src, true).0.unwrap();
        let ast::ItemKind::Func(f) = &m.items[0].kind else {
            panic!("a function")
        };
        let names: Vec<String> = f
            .params
            .iter()
            .map(|p| match p {
                ast::Param::Value { ty, .. } => format!("{ty:?}"),
                _ => String::new(),
            })
            .collect();
        assert!(names[0].contains("\"i64\"") && !names[0].contains("\"int\""));
        assert!(
            names[1].contains("\"Option\"")
                && names[1].contains("\"List\"")
                && names[1].contains("\"Str\"")
        );
        assert!(
            names[2].contains("\"SortedMap\"")
                && names[2].contains("\"i8\"")
                && names[2].contains("\"Bytes\"")
        );
        assert_eq!(print(&m), src);
    }

    #[test]
    fn yong_spellings_are_not_xin_types() {
        let src = "def f(x: i64, y: Option[int], z: List[int]) -> Map[str, int]:\n    return {}\n";
        let got: Vec<_> = codes(src, true).into_iter().map(|c| (c.0, c.2)).collect();
        assert_eq!(
            got,
            [
                ("E200".into(), 10),
                ("E200".into(), 18),
                ("E200".into(), 34),
                ("E200".into(), 48)
            ]
        );
        // The lenient reader normalizes `List` (N04) before it reaches HIR.
        assert!(
            !codes("def f(z: List[int]) -> int:\n    return 0\n", false)
                .iter()
                .any(|c| c.0 == "E200")
        );
    }

    #[test]
    fn python_classes_normalize() {
        let src = "@dataclass\nclass P:\n    x: \"P\"\n\n    def m(self, o: P) -> int:\n        return 1\n\nclass E(Exception):\n    pass\n\nShape = P | E\n";
        let got: Vec<_> = codes(src, false).into_iter().map(|c| c.0).collect();
        assert_eq!(got, ["N08", "N19", "N09", "N20", "N20"]);
        let out = print(&parse("t.xin", src, false).0.unwrap());
        assert!(out.contains("def m(self: P, o: P) -> int:"), "{out}");
        assert!(
            out.contains("class E:") && out.contains("type Shape = P | E"),
            "{out}"
        );
        assert!(!codes(src, true).iter().all(|c| c.0.starts_with('N')));
    }

    #[test]
    fn loose_ex_moves_into_the_function_above() {
        let src =
            "def f(n: int) -> int:\n    \"\"\"Doc.\"\"\"\n    ...\n\nex f(1) == 1\nex f(2) == 2\n";
        assert_eq!(
            codes(src, false),
            [("N32".into(), 5, 1), ("N32".into(), 6, 1)]
        );
        let printed = roundtrip(src);
        assert!(printed.contains("    ex f(1) == 1\n    ex f(2) == 2\n"));
        assert_eq!(
            codes(src, true),
            [("E100".into(), 5, 1), ("E100".into(), 6, 1)]
        );
        assert_eq!(codes("ex f(1) == 1\n", false)[0].0, "E100");
    }

    #[test]
    fn layout_errors() {
        assert_eq!(
            codes("def f() -> int:\n    a = 1; b = 2\n    return a\n", false),
            [("E103".into(), 2, 10)]
        );
        assert_eq!(
            codes("def f() -> int:\n  return 1\n x = 2\n", false)[0].0,
            "E100"
        );
    }

    #[test]
    fn statement_after_multiline_brackets() {
        let src = "def f() -> int:\n    xs = [\n        1,\n    ]\n    return len(xs)\n";
        assert_eq!(codes(src, true), []);
    }

    #[test]
    fn prefixes_never_panic() {
        for (i, _) in SAMPLE.char_indices() {
            parse("t.xin", &SAMPLE[..i], false);
        }
    }

    /// Every pass, run and golden Xin case parses and round-trips through the printer.
    #[test]
    fn conformance_xin_files_round_trip() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance");
        let mut stack = vec![root.join("xin"), root.join("golden")];
        let mut n = 0;
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "xin")
                    && !p.components().any(|c| c.as_os_str() == "fail")
                {
                    roundtrip(&std::fs::read_to_string(&p).unwrap());
                    n += 1;
                }
            }
        }
        assert!(n == 0 || n > 50, "found {n} files");
    }
}
