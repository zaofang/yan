use super::*;

fn lower(src: &str) -> (y::Module, Vec<Diagnostic>) {
    let (m, d) = yong_xin::parse("t.xin", src, false);
    assert!(
        d.iter().all(|d| d.severity != yong_diag::Severity::Error),
        "{d:?}"
    );
    elaborate("t.xin", src, &m.expect("parses"), Options::default()).expect("supported")
}

#[test]
fn hello_lowers_to_checked_yong() {
    let src = "pub def main() uses io:\n    x = 1 + 2\n    io.print(f\"{x}\")\n";
    let (m, d) = lower(src);
    assert!(d.is_empty(), "{d:?}");
    let checked = yong_check::check("t.xin", src, &m);
    assert!(checked.is_empty(), "{checked:?}");
    let text = yong_core::print(&m);
    assert!(text.contains("add_checked_i64"), "{text}");
}

fn run(src: &str) -> String {
    let (m, d) = lower(src);
    assert!(d.is_empty(), "{d:?}");
    let checked = yong_check::check("t.xin", src, &m);
    assert!(checked.is_empty(), "{checked:?}\n{}", yong_core::print(&m));
    let mut out = Vec::new();
    yong_interp::run(&m, &mut out).expect("runs");
    String::from_utf8(out).expect("utf-8")
}

#[test]
fn joins_continue_the_block() {
    let src = "class Bad:\n    code: int\n\n\
               def check(x: int) -> int fail Bad:\n    if x < 0:\n        fail Bad(code=x)\n    return x\n\n\
               pub def main() uses io:\n    a = 1 if 2 > 1 else 0\n    b = a > 0 and a < 5\n\
               \x20   c = check(-3) else err -> err.code\n    io.print(f\"{a} {b} {c}\")\n";
    assert_eq!(run(src), "1 True -3\n");
}

#[test]
fn tuple_assignment_reads_before_writing() {
    let src =
        "pub def main() uses io:\n    a, b = 1, 2\n    a, b = b, a\n    io.print(f\"{a} {b}\")\n";
    assert_eq!(run(src), "2 1\n");
}

#[test]
fn generic_calls_dispatch_to_impls() {
    let src = "trait Size:\n    def size(self: Self) -> int: ...\n\nclass Bag:\n    items: list[int]\n\nimpl Size for Bag:\n    def size(self: Bag) -> int:\n        return len(self.items)\n\ndef total[T: Size](xs: list[T]) -> int:\n    n = 0\n    for x in xs:\n        n += x.size()\n    return n\n\ndef first[T](xs: list[T], d: T) -> T:\n    if len(xs) < 1:\n        return d\n    return xs[0]\n\npub def main() uses io:\n    bs = [Bag(items=[1, 2]), Bag(items=[3])]\n    io.print(f\"{total(bs)} {bs[0].size()} {first([5, 6], 0)}\")\n";
    assert_eq!(run(src), "3 2 5\n");
}

#[test]
fn lambdas_capture_and_take_types_from_the_callee() {
    let src = "def twice[T](f: Callable[[T], T], x: T) -> T:\n    return f(f(x))\n\n\
               def total(xs: list[int], f: Callable[[int], int]) -> int:\n    t = 0\n    for x in xs:\n        t += f(x)\n    return t\n\n\
               pub def main() uses io:\n    m = 3\n    a = total([1, 2, 3], lambda v: v * m)\n\
               \x20   add: Callable[[int], Callable[[int], int]] = lambda p: lambda q: p + q\n\
               \x20   inc = add(1)\n    s = twice(lambda w: w + \"!\", \"hi\")\n    io.print(f\"{a} {inc(41)} {s}\")\n";
    assert_eq!(run(src), "18 42 hi!!\n");
}

#[test]
fn unannotated_lambdas_take_types_from_their_first_call() {
    let src = "pub def main() uses io:\n    m = 5\n    k = lambda x, y: x * y + m\n\
               \x20   s = lambda t: t + \"?\"\n    io.print(f\"{k(2, 3)} {k(4, 1)} {s('ok')}\")\n";
    assert_eq!(run(src), "11 9 ok?\n");
}

/// Law 1 (16.5) through the shared raise: Xin to Yong and back prints the same text.
fn raised(src: &str) -> String {
    let (y, d) = lower(src);
    assert!(
        d.iter().all(|d| d.severity != yong_diag::Severity::Error),
        "{d:?}"
    );
    let h = yong_hir::raise::raise(&y, &StdInfo).expect("raises");
    yong_xin::print(&yong_xin::resugar(h).expect("in the Xin subset"))
}

#[test]
fn raise_restores_loops_fallbacks_and_fstrings() {
    let src = "\
pub def count_words(text: str) -> dict[str, int]:
    counts: dict[str, int] = {}
    for w in text.split(\" \"):
        counts[w.copy()] = (counts.get(w) else 0) + 1
    return counts

pub def main() uses io:
    xs = [1, 2, 3]
    n = 0
    while n < 10:
        n += len(xs)
    for i in range(3):
        io.print(f\"{i}: {n}\")
";
    assert_eq!(raised(src), src);
}

#[test]
fn question_mark_needs_a_function_that_can_fail() {
    let g = "def g() -> int fail str:\n    fail \"x\"\n\n";
    let (_, d) = lower(&format!("{g}def f() -> int:\n    return g()?\n"));
    assert_eq!(
        d.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["E310"]
    );
    let (_, d) = lower(&format!("{g}def f() -> int fail str:\n    return g()?\n"));
    assert!(d.is_empty(), "{d:?}");
}

#[test]
fn unsupported_construct_is_e017_and_the_next_item_is_checked() {
    let src =
        "def a() -> int:\n    s = {1, 2}\n    return 1\n\ndef b() -> int:\n    return missing\n";
    let (_, d) = lower(src);
    let got: Vec<_> = d
        .iter()
        .map(|d| (d.code.as_str(), d.span.line, d.span.col, d.item.as_str()))
        .collect();
    assert_eq!(got, [("E017", 2, 9, "a"), ("E200", 6, 12, "b")], "{d:?}");
}

#[test]
fn a_value_function_must_not_fall_off_its_end() {
    let codes = |src: &str| {
        let (_, d) = lower(src);
        d.into_iter().map(|d| d.code).collect::<Vec<_>>()
    };
    let src = "def f(x: int) -> int:\n    if x > 0:\n        return 1\n";
    assert_eq!(codes(src), ["E201"]);
    let ok = "def f(x: int) -> int:\n    if x > 0:\n        return 1\n    else:\n        return 0\n\n\
              def g(x: int) -> int:\n    while True:\n        x += 1\n        if x > 9:\n            return x\n";
    assert!(codes(ok).is_empty());
}

#[test]
fn a_misspelt_use_reports_only_the_unresolved_name() {
    let (_, d) = lower("def f(count: int) -> int:\n    return cout + 1\n");
    assert_eq!(
        d.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["E200"]
    );
}

#[test]
fn messages_name_the_real_problem() {
    let (_, d) = lower("def f() -> int:\n    y = 2\n    return 1\n");
    assert!(
        d[0].message
            .ends_with("remove it or write `_` in its place"),
        "{d:?}"
    );
    let (_, d) = lower("def f(d: dict[str, int]) -> int:\n    return d.get(\"a\") + 1\n");
    assert!(d[0].message.contains("may be `None`"), "{d:?}");
}

#[test]
fn a_literal_argument_binds_a_type_parameter() {
    let src = "def first[T](xs: list[T]) -> T | None:\n    return xs.get(0)\n\n\
               pub def main() uses io:\n    s = first([\"a\"]) else \"z\"\n    n = first([1]) else 0\n\
               \x20   io.print(f\"{s} {n + 1}\")\n";
    assert_eq!(run(src), "a 2\n");
}

#[test]
fn lenient_methods_and_string_loops_run() {
    let src = "\
class Word:
    text: str

    def count(self, x: char) -> int:
        n = 0
        for c in self.text:
            if c == x:
                n += 1
        return n

pub def main() uses io:
    w = Word(\"hehh\")
    io.print(str(w.count(c'h')))
";
    let (m, d) = lower(src);
    let codes: Vec<_> = d.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(codes, ["N21"]);
    let checked = yong_check::check("t.xin", src, &m);
    assert!(checked.is_empty(), "{checked:?}");
    let mut out = Vec::new();
    yong_interp::run(&m, &mut out).expect("runs");
    assert_eq!(String::from_utf8(out).unwrap(), "3\n");
}

#[test]
fn a_failed_assert_reports_its_line_and_sides() {
    let src = "pub def main():\n    x = 1 + 1\n    assert x == 3\n";
    let (m, _) = lower(src);
    let r = yong_interp::run(&m, &mut Vec::new());
    let Err(yong_interp::Stop::Panic(p)) = r else {
        panic!("expected a panic")
    };
    assert_eq!(
        (p.kind.as_str(), p.message.as_str()),
        ("assert", "line 3: left is 2, right is 3")
    );
}

#[test]
fn str_advice_names_the_receiver() {
    let src = "def first(name: str) -> str:\n    return name[0]\n";
    let (_, d) = lower(src);
    assert_eq!(
        d[0].message, "`str` has no indexing; use `name.chars()`",
        "{d:?}"
    );
}

#[test]
fn wen_spells_types_its_own_way() {
    let t = Ty::Fn(
        false,
        vec![Ty::Dict(Box::new(Ty::Str), Box::new(Ty::Int))],
        Box::new(Ty::Option(Box::new(Ty::List(Box::new(Ty::BigInt))))),
    );
    assert_eq!(t.wen(), "(Map[str, int]) -> List[BigInt]?");
    assert_eq!(
        t.to_string(),
        "Callable[[dict[str, int]], list[bigint] | None]"
    );
    assert_eq!(Ty::Unit.wen(), "()");
}

#[test]
fn unknown_names_carry_near_names_as_maybe_fixes() {
    let src = "class P:\n    name: str\n\ndef f(p: P, count: int) -> str:\n    _ = cout\n    return p.nmae\n";
    let (_, d) = lower(src);
    let fixes: Vec<_> = d
        .iter()
        .filter(|d| d.code == "E200" || d.code == "E203")
        .map(|d| {
            (
                d.code.as_str(),
                d.fixes[0]["edits"][0]["text"].as_str().unwrap_or(""),
            )
        })
        .collect();
    assert_eq!(fixes, [("E200", "count"), ("E203", "name")], "{d:?}");
}

#[test]
fn second_sites_are_labels() {
    let src = "def f() -> int:\n    return 1\n\ndef f() -> int:\n    return 2\n\n\
               def g(xs: list[int]) -> list[int]:\n    ys = xs\n    return xs\n";
    let (_, d) = lower(src);
    let labels: Vec<_> = d
        .iter()
        .flat_map(|d| {
            d.labels
                .iter()
                .map(|l| (d.code.as_str(), l.text.as_str(), l.span.line))
        })
        .collect();
    assert_eq!(
        labels,
        [("E250", "first defined", 1), ("E413", "used again", 9)],
        "{d:?}"
    );
}

/// Applies every fix's single edit to `src`, last edit first so earlier spans stay valid.
fn apply_fixes(src: &str, d: &[Diagnostic]) -> String {
    let mut lines: Vec<String> = src.lines().map(str::to_owned).collect();
    let mut edits: Vec<_> = d
        .iter()
        .flat_map(|d| &d.fixes)
        .map(|f| &f["edits"][0])
        .collect();
    edits.sort_by_key(|e| (e["span"]["line"].as_u64(), e["span"]["col"].as_u64()));
    for e in edits.into_iter().rev() {
        let line = &mut lines[e["span"]["line"].as_u64().unwrap() as usize - 1];
        let from = e["span"]["col"].as_u64().unwrap() as usize - 1;
        let to = e["span"]["end_col"].as_u64().unwrap() as usize - 1;
        assert_eq!(&line[from..to], e["search"].as_str().unwrap());
        line.replace_range(from..to, e["text"].as_str().unwrap());
    }
    lines.join("\n") + "\n"
}

#[test]
fn mechanical_fixes_remove_their_diagnostics() {
    let src = "def bump(var n: int) -> None:\n    n += 1\n\n\
               def show(n: int) -> int:\n    return n\n\n\
               def keep(xs: list[int]) -> list[int]:\n    return xs\n\n\
               def f() -> int:\n    a = 1\n    bump(a)\n    b = show(&a)\n    c: int = 2\n    unused = 3\n\
               \x20   xs = [1]\n    ys = xs\n    return a + b + c + len(xs) + len(ys)\n";
    let (_, d) = lower(src);
    let mut codes: Vec<_> = d.iter().map(|d| d.code.as_str()).collect();
    codes.sort();
    assert_eq!(codes, ["E404", "E405", "E413", "W812", "W823"], "{d:?}");
    let fixed = apply_fixes(src, &d);
    let (_, d) = lower(&fixed);
    assert!(d.is_empty(), "{fixed}\n{d:?}");
}

#[test]
fn early_return_guards_prove_indices() {
    let codes = |src: &str| -> Vec<String> { lower(src).1.into_iter().map(|d| d.code).collect() };
    let guarded = "\
def first(xs: list[int]) -> int:
    if len(xs) == 0:
        return 0
    return xs[0]

def last(xs: list[int]) -> int:
    if not xs:
        return 0
    return xs[len(xs) - 1]

def inner(xs: list[int], i: int) -> int:
    if i != 0 and i >= 0 and i < len(xs):
        return xs[i - 1]
    return 0
";
    assert_eq!(codes(guarded), ["N15"]);
    let open =
        "def f(xs: list[int]) -> int:\n    if len(xs) == 1:\n        return 0\n    return xs[0]\n";
    assert_eq!(codes(open), ["W820"]);
}

#[test]
fn binary_search_and_counting_loops_prove_indices() {
    let codes = |src: &str| -> Vec<String> { lower(src).1.into_iter().map(|d| d.code).collect() };
    let proven = "\
def find(xs: list[int], t: int) -> int:
    lo = 0
    hi = len(xs) - 1
    while lo <= hi:
        mid = (lo + hi) // 2
        if xs[mid] == t:
            return mid
        elif xs[mid] < t:
            lo = mid + 1
        else:
            hi = mid - 1
    return -1

def total(xs: list[int]) -> int:
    t = 0
    i = 0
    while i < len(xs):
        t += xs[i]
        i += 1
    return t
";
    assert!(codes(proven).is_empty(), "{:?}", codes(proven));
    let off_by_one = "\
def f(xs: list[int]) -> int:
    lo = 0
    hi = len(xs)
    while lo <= hi:
        mid = (lo + hi) // 2
        if xs[mid] > 0:
            return mid
        lo = mid + 1
    return -1
";
    assert_eq!(codes(off_by_one), ["W820"]);
}

#[test]
fn upper_and_lower_take_a_char() {
    let src = "def both(s: str) -> str:\n    out = \"\"\n    for c in s.chars():\n\
        \x20       out += c.upper() + c.lower()\n    return out\n\n\
        pub def main() uses io:\n    io.print(both(\"aB\"))\n";
    assert_eq!(run(src), "AaBb\n");
}

#[test]
fn post_names_resolve_in_a_hole_body() {
    let codes = |src: &str| {
        lower(src)
            .1
            .iter()
            .filter(|d| d.severity == yong_diag::Severity::Error)
            .map(|d| d.code.clone())
            .collect::<Vec<_>>()
    };
    let head = "def f(n: int) -> int:\n    \"\"\"Doc.\"\"\"\n";
    assert_eq!(
        codes(&format!("{head}    post result >= 0\n    ...\n")),
        ["E200"]
    );
    assert!(codes(&format!("{head}    post out >= n\n    ...\n")).is_empty());
}

#[test]
fn a_method_with_no_loaded_signature_does_not_panic() {
    let src = "def f(xss: list[list[int]]) -> int:\n    \"\"\"Doc.\"\"\"\n    post out == xss.map(lambda xs: xs.len()).sum()\n    ...\n";
    let (m, d) = yong_xin::parse("t.xin", src, false);
    assert!(
        d.iter().all(|d| d.severity != yong_diag::Severity::Error),
        "{d:?}"
    );
    let _ = elaborate("t.xin", src, &m.expect("parses"), Options::default());
}

#[test]
fn a_method_on_a_literal_lowers() {
    let src = "pub def main() uses io:\n    io.print(\"-\".join([\"a\", \"b\"]))\n";
    assert_eq!(run(src), "a-b\n");
}
