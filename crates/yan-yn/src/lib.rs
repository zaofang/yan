//! Yan, the human surface (spec 15): lexer, parser and syntax diagnostics.

pub mod ast;
mod grammar;
mod lexer;
mod lower;
mod parser;
mod printer;

pub use grammar::grammar;
pub use lower::{Unsupported, lower};
pub use printer::print;

use lexer::Tok;
use yan_diag::Diagnostic;

/// Parse a `.yn` file. The module is returned whenever parsing recovered, with every lexical
/// and syntax diagnostic; a module with error diagnostics must not be lowered.
pub fn parse(file: &str, src: &str) -> (ast::Module, Vec<Diagnostic>) {
    let (lexed, mut diags) = lexer::lex(file, src);
    let mut p = parser::Parser::new(&lexed.tokens, file, src);
    let mut module = p.module();
    module.cst = syntax(src, &lexed, &p.nodes);
    diags.append(&mut p.diags);
    (module, diags)
}

/// The lossless tree over `src` (R995): the lexer's tokens and comments and the parser's nodes.
fn syntax(
    src: &str,
    lexed: &lexer::Lexed,
    nodes: &[(yan_cst::Kind, usize, usize)],
) -> yan_cst::Cst {
    let code =
        |t: &&lexer::Token| !matches!(t.tok, Tok::Newline | Tok::Indent | Tok::Dedent | Tok::Eof);
    let mut b = yan_cst::Builder::new(src);
    for t in lexed.tokens.iter().filter(code) {
        b.token(t.start, t.end);
    }
    for &(lo, hi, doc) in &lexed.comments {
        b.comment(lo, hi, doc);
    }
    for &(kind, from, to) in nodes {
        let mut toks = lexed.tokens[from..to].iter().filter(code);
        if let Some(first) = toks.next() {
            let (lo, hi) = toks.fold((first.start, first.end), |(lo, hi), t| {
                (lo.min(t.start), hi.max(t.end))
            });
            b.node(kind, lo, hi);
        }
    }
    b.finish()
}

#[cfg(test)]
mod selftest {
    use std::path::{Path, PathBuf};

    use crate::ast::*;

    /// Codes of the conformance cases decided by the parser alone.
    const SYNTAX_CODES: [&str; 13] = [
        "E100", "E101", "E102", "E104", "E105", "E110", "E112", "E132", "E133", "E134", "E135",
        "E141", "E143",
    ];

    fn files(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                files(&p, out);
            } else if p.extension().is_some_and(|x| x == "yn") {
                out.push(p);
            }
        }
    }

    fn expected(src: &str) -> Vec<(String, u32, u32)> {
        let mut out: Vec<_> = src
            .lines()
            .filter_map(|l| l.strip_prefix("-- expect: "))
            .map(|e| {
                let (code, at) = e.split_once(" at ").unwrap();
                let (l, c) = at.trim().split_once(':').unwrap();
                (code.to_owned(), l.parse().unwrap(), c.parse().unwrap())
            })
            .collect();
        out.sort();
        out
    }

    /// The syntax tree of every `.yn` file under conformance/ prints back to its source byte for
    /// byte, and holds one item node per item of a clean parse (R995).
    #[test]
    fn syntax_tree_lossless() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance");
        let mut paths = Vec::new();
        files(&root, &mut paths);
        assert!(paths.len() > 100);
        for p in paths {
            let src = std::fs::read_to_string(&p).unwrap();
            let (m, diags) = crate::parse(&p.to_string_lossy(), &src);
            assert_eq!(m.cst.to_string(), src, "{}", p.display());
            if diags.is_empty() {
                let tree = m.cst.syntax();
                let items = tree.children().filter(|n| n.kind() == yan_cst::Kind::Item);
                assert_eq!(items.count(), m.items.len(), "{}", p.display());
                let skipped = tree
                    .descendants_with_tokens()
                    .any(|t| t.kind() == yan_cst::Kind::Skipped);
                assert!(!skipped, "{}", p.display());
            }
        }
    }

    /// Every Yan conformance case parses; cases whose expected codes are all syntax codes
    /// report exactly those, and every other case reports nothing.
    #[test]
    fn conformance_cases_parse() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/yn");
        let mut paths = Vec::new();
        files(&root, &mut paths);
        paths.sort();
        assert!(!paths.is_empty());
        let mut failures = Vec::new();
        for path in &paths {
            let src = std::fs::read_to_string(path).unwrap();
            let want: Vec<_> = expected(&src)
                .into_iter()
                .filter(|(c, ..)| SYNTAX_CODES.contains(&c.as_str()))
                .collect();
            let (_, diags) = super::parse("case.yn", &src);
            let mut got: Vec<_> = diags
                .iter()
                .map(|d| (d.code.clone(), d.span.line, d.span.col))
                .collect();
            got.sort();
            if got != want {
                let msgs: Vec<_> = diags.iter().map(|d| d.render_line(None)).collect();
                failures.push(format!(
                    "{}: want {want:?}\n  {}",
                    path.display(),
                    msgs.join("\n  ")
                ));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    fn code_lines(src: &str) -> Vec<&str> {
        src.lines()
            .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with("--"))
            .map(|l| l.split(" --").next().unwrap().trim_end())
            .collect()
    }

    /// Cases without syntax errors print back to their own code lines, up to redundant
    /// parentheses the formatter drops, and printing is idempotent.
    #[test]
    fn conformance_cases_round_trip() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/yn");
        let mut paths = Vec::new();
        files(&root, &mut paths);
        paths.sort();
        let mut failures = Vec::new();
        for path in &paths {
            let src = std::fs::read_to_string(path).unwrap();
            let (m, diags) = super::parse("case.yn", &src);
            if !diags.is_empty() {
                continue;
            }
            let printed = super::print(&m);
            let (again, _) = super::parse("case.yn", &printed);
            if super::print(&again) != printed {
                failures.push(format!("{}: printing is not idempotent", path.display()));
            }
            let (want, got) = (code_lines(&src), code_lines(&printed));
            let bare = |l: &&str| l.replace(['(', ')'], "");
            let same = want.len() == got.len()
                && want
                    .iter()
                    .zip(&got)
                    .all(|(a, b)| a == b || bare(a) == bare(b));
            if !same {
                let at = want
                    .iter()
                    .zip(&got)
                    .position(|(a, b)| a != b)
                    .unwrap_or(want.len().min(got.len()));
                failures.push(format!(
                    "{}:\n  want {:?}\n  got  {:?}",
                    path.display(),
                    want.get(at),
                    got.get(at)
                ));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    fn reprint(src: &str) -> String {
        let (m, diags) = super::parse("t.yn", src);
        let msgs: Vec<_> = diags.iter().map(|d| d.render_line(None)).collect();
        assert!(diags.is_empty(), "{src}: {msgs:?}");
        super::print(&m)
    }

    #[test]
    fn minimal_and_required_parentheses() {
        let cases = [
            ("(a + b) * c", "(a + b) * c"),
            ("a + (b * c)", "a + b * c"),
            ("a - (b - c)", "a - (b - c)"),
            ("(a - b) - c", "a - b - c"),
            ("(a ** b) ** c", "(a ** b) ** c"),
            ("a ** (b ** c)", "a ** b ** c"),
            ("-(x ** 2)", "-(x ** 2)"),
            ("(-x) ** 2", "(-x) ** 2"),
            ("(a and b) or c", "(a and b) or c"),
            ("(a | b) & c", "(a | b) & c"),
            ("(a & b) | c", "(a & b) | c"),
            ("a | (b + c)", "a | (b + c)"),
            ("(a & b) == c", "(a & b) == c"),
            ("(a < b) == c", "(a < b) == c"),
            ("a % (b * c)", "a % (b * c)"),
            ("(a * b) * c", "a * b * c"),
            ("(x else y) + 1", "(x else y) + 1"),
            ("x else (0 + 1)", "x else (0 + 1)"),
            ("(if a then b else c) else d", "(if a then b else c) else d"),
            (
                "if a then b else if c then d else e",
                "if a then b else if c then d else e",
            ),
            ("not (a and b)", "not (a and b)"),
            ("(not a) and b", "not a and b"),
            ("(a as int) + 1", "a as int + 1"),
            ("(a + 1) as int", "(a + 1) as int"),
            ("(xs[0]).len()", "xs[0].len()"),
            ("(-a).abs()", "(-a).abs()"),
            ("(x -> x + 1)(2)", "(x -> x + 1)(2)"),
            ("f(x -> x.a.b)", "f(.a.b)"),
            ("f(x -> y.a)", "f(x -> y.a)"),
            ("(a,)", "(a,)"),
            ("[1 -2; 3 4]", "[1 -2; 3 4]"),
            ("[a (b)]", "[a (b)]"),
            ("[a(b)]", "[a(b)]"),
            ("xs[i -1]", "xs[i - 1]"),
            ("\"a \\(b) \\(c:>4)\"", "\"a \\(b) \\(c:>4)\""),
            ("t\"n = {n} {{x}}\"", "t\"n = {n} {{x}}\""),
            ("x is not None and y", "x is not None and y"),
            ("d.get(k) else err -> 1", "d.get(k) else err -> 1"),
            ("150ms + 2.5bp", "150ms + 2.5bp"),
            (
                "2026-10-30T09:00[America/Chicago]",
                "2026-10-30T09:00[America/Chicago]",
            ),
        ];
        for (input, want) in cases {
            let got = reprint(&format!("f() = {input}\n"));
            assert_eq!(got, format!("f() = {want}\n"), "input {input}");
        }
    }

    #[test]
    fn signature_layout() {
        let src = "sync_prices(src: Url, dst: Path, since: PlainDate) -> Report\n    fail SyncError\n    uses net, fs, time =\n  let rows = fetch(src, since)?\n  write(dst, rows)\n";
        let short = reprint(src);
        assert_eq!(
            short,
            "sync_prices(src: Url, dst: Path, since: PlainDate) -> Report fail SyncError uses net, fs, time =\n  let rows = fetch(src, since)?\n  write(dst, rows)\n"
        );
        let long = "synchronize_all_prices(source_url: Url, destination: Path, since: PlainDate) -> Report\n    fail SyncError\n    uses net, fs, time =\n  1\n";
        assert_eq!(reprint(long), long);
        let longer = "synchronize_all_prices(\n    source_url: Url,\n    destination_directory: Path,\n    since_date: PlainDate,\n) -> Report\n    fail SyncError =\n  1\n";
        assert_eq!(reprint(longer), longer);
    }

    #[test]
    fn lookahead_points() {
        // A `(` in a type starts a function type exactly when `) ->` closes it.
        let src = "f(g: (int) -> int uses (io, net), t: (int, str)) -> (int) -> int fail E = g\n";
        assert_eq!(reprint(src), src);
        // Clause form: the first body line is an arm when `->` follows it at top level.
        let src = "f(n: int) -> int =\n  pre n >= 0\n  0 -> 1\n  _ -> n\n";
        let (m, _) = super::parse("t.yn", src);
        let ItemKind::Fn(f) = &m.items[0].kind else {
            panic!()
        };
        assert!(
            matches!(&f.body, Body::Clauses { arms, contracts } if arms.len() == 2 && contracts.len() == 1)
        );
        // A lambda statement after the first line is not an arm.
        let src = "f() -> (int) -> int =\n  let t = 1\n  x -> x + t\n";
        assert_eq!(reprint(src), src);
        // The else after a let belongs to the let.
        let src =
            "f(d: Map[str, int]) -> int =\n  let Some(v) = d.get(\"a\") else\n    return 0\n  v\n";
        assert_eq!(reprint(src), src);
    }

    #[test]
    fn cost_bounds_round_trip_on_one_line() {
        let src = "f(xs: List[int]) -> int =\n  pre len(xs) > 0\n  cost steps <= 2 * len(xs), alloc <= len(xs)\n  0\n";
        assert_eq!(reprint(src), src);
        let src = "f(xs: List[int]) -> int =\n  cost alloc <= 3\n  0\n";
        assert_eq!(reprint(src), src);
    }

    #[test]
    fn spec_example_15_6() {
        let src = r#"--- Fetch quotes concurrently and value a portfolio.

type Position = { sym: str, qty: int }
type Quote = { sym: str, px: float }
type PortErr = MissingQuote(sym: str) | Fetch(msg: str)

impl From[http.HttpError] for PortErr
  convert(e: http.HttpError) -> PortErr = Fetch(msg: copy e.msg)

fetch_quote(sym: str) -> Quote =
  let body = http.get_text("https://px.example/\(sym)")?
  let px = json.parse_float(body) else fail Fetch(msg: "bad price: \(sym)")
  Quote(sym: copy sym, px:)

pub value(ps: List[Position], qs: Map[str, Quote]) -> float fail PortErr =
  pre ps.len() > 0
  ex try value([Position(sym: "A", qty: 2)], {"A": Quote(sym: "A", px: 10.0)}) == Ok(20.0)
  var total = 0.0
  for p in ps
    let q = qs.get(p.sym) else fail MissingQuote(sym: copy p.sym)
    total += float(p.qty) * q.px
  total

pub main() fail PortErr uses io, net =
  let ps = [Position(sym: "SPX", qty: 10), Position(sym: "NDX", qty: 5)]
  let quotes = par for p in ps
    fetch_quote(p.sym)?
  let qs = quotes.index_by(.sym)
  io.print("portfolio value: \(value(ps, qs)?)")

frame() =
  let r: Array[PlainDate, str, float] = returns(bars)
  let vol = r.std(over: PlainDate) * sqrt(252.0)
  let demeaned = r - r.mean(over: PlainDate)    -- broadcasts along PlainDate
  let spx = r[str: "SPX"]
  let fee = 2.5bp
  let w = [0.6 0.4]
"#;
        let printed = reprint(src);
        assert_eq!(code_lines(&printed), code_lines(src));
    }

    #[test]
    fn comments_and_author_blank_lines_survive_in_bodies() {
        let src = "-- top
f(x: int) -> int =
  -- lead

  let y = x + 1  -- trail


  let z = y * 2
  if z > 1
    z
    -- inner
  -- outer
  z
-- tail
";
        let want = "-- top
f(x: int) -> int =
  -- lead

  let y = x + 1  -- trail

  let z = y * 2
  if z > 1
    z
    -- inner
  -- outer
  z

-- tail
";
        let printed = reprint(src);
        assert_eq!(printed, want);
        assert_eq!(reprint(&printed), printed);
    }

    #[test]
    fn long_lines_wrap_inside_brackets_and_chains() {
        let src = "f(xs: List[int]) -> int =
  let a = compute_the_total_of_items(xs, another_long_argument_name, yet_another_argument_name_here, more)
  let b = xs.map(first_transformation).filter(second_predicate_function).take(10).collect_into_list()
  a
";
        let want = "f(xs: List[int]) -> int =
  let a = compute_the_total_of_items(
      xs,
      another_long_argument_name,
      yet_another_argument_name_here,
      more,
  )
  let b = xs
      .map(first_transformation)
      .filter(second_predicate_function)
      .take(10)
      .collect_into_list()
  a
";
        let printed = reprint(src);
        assert_eq!(printed, want);
        assert_eq!(reprint(&printed), printed);
    }

    #[test]
    fn keywords_name_methods_after_a_dot() {
        let src = "f(xs: List[int]) -> List[int] = xs.take(2)\n";
        assert_eq!(reprint(src), src);
    }

    #[test]
    fn fail_is_inferred_on_non_pub_functions() {
        let src = "type E = { msg: str }\n\n\
            parse(s: str) -> int fail E = s.parse_int() else fail E(msg: copy s)\n\n\
            mid(s: str) -> int = parse(s)?\n\n\
            top(s: str) -> int = mid(s)?\n\n\
            pub run(s: str) -> int = mid(s)?\n";
        let (m, diags) = super::parse("t.yn", src);
        assert!(diags.is_empty());
        let (h, _) = super::lower("t.yn", src, &m).unwrap();
        let fail = |name: &str| {
            h.items.iter().find_map(|i| match &i.kind {
                yan_hir::ItemKind::Func(f) if f.name == name => {
                    Some(f.ret.as_ref().map(|r| r.fail.clone()).unwrap_or_default())
                }
                _ => None,
            })
        };
        assert_eq!(fail("mid"), Some(vec![vec!["E".to_owned()]]));
        assert_eq!(fail("top"), Some(vec![vec!["E".to_owned()]]));
        assert_eq!(fail("run"), Some(vec![]));
    }

    #[test]
    fn a_cancel_scope_infers_timeout() {
        let src =
            "wait(d: Duration) -> int uses time =\n  with timeout(d)\n    time.sleep(d)?\n  0\n";
        let (m, diags) = super::parse("t.yn", src);
        assert!(diags.is_empty(), "{diags:?}");
        let (h, _) = super::lower("t.yn", src, &m).unwrap();
        let fail = h.items.iter().find_map(|i| match &i.kind {
            yan_hir::ItemKind::Func(f) if f.name == "wait" => {
                Some(f.ret.as_ref().map(|r| r.fail.clone()).unwrap_or_default())
            }
            _ => None,
        });
        assert_eq!(fail, Some(vec![vec!["Timeout".to_owned()]]));
    }
}
