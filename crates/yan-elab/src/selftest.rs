use super::*;

fn lower(src: &str) -> (y::Module, Vec<Diagnostic>) {
    let (m, d) = yan_yag::parse("t.yag", src, false);
    assert!(
        d.iter().all(|d| d.severity != yan_diag::Severity::Error),
        "{d:?}"
    );
    elaborate("t.yag", src, &m.expect("parses"), Options::default()).expect("supported")
}

#[test]
fn hello_lowers_to_checked_yir() {
    let src = "pub def main() uses io:\n    x = 1 + 2\n    io.print(f\"{x}\")\n";
    let (m, d) = lower(src);
    assert!(d.is_empty(), "{d:?}");
    let checked = yan_check::check("t.yag", src, &m);
    assert!(checked.is_empty(), "{checked:?}");
    let text = yan_core::print(&m);
    assert!(text.contains("add_checked_i64"), "{text}");
}

fn run(src: &str) -> String {
    let (m, d) = lower(src);
    assert!(d.is_empty(), "{d:?}");
    let checked = yan_check::check("t.yag", src, &m);
    assert!(checked.is_empty(), "{checked:?}\n{}", yan_core::print(&m));
    let mut out = Vec::new();
    yan_interp::run(&m, &mut out).expect("runs");
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

/// Law 1 (16.5) through the shared raise: Yan Agent to Yan Core and back prints the same text.
fn raised(src: &str) -> String {
    let (y, d) = lower(src);
    assert!(
        d.iter().all(|d| d.severity != yan_diag::Severity::Error),
        "{d:?}"
    );
    let h = yan_hir::raise::raise(&y, &yan_surface::TriviaTable::default(), &StdInfo)
        .expect("raises")
        .module;
    yan_yag::print(&yan_yag::resugar(h).expect("in the Yan Agent subset"))
}

/// `src` through elaboration and raise with the marks elaboration wrote.
fn raised_marked(src: &str) -> (String, Vec<yan_hir::raise::Unused>) {
    let (m, _) = yan_yag::parse("t.yag", src, false);
    let (y, table, d) = elaborate_in("t.yag", src, &m.expect("parses"), Options::default(), None)
        .expect("supported");
    assert!(
        d.iter().all(|d| d.severity != yan_diag::Severity::Error),
        "{d:?}"
    );
    let r = yan_hir::raise::raise(&y, &table, &StdInfo).expect("raises");
    let unused = yan_hir::raise::unconsumed(&table, &r);
    (
        yan_yag::print(&yan_yag::resugar(r.module).expect("in the Yan Agent subset")),
        unused,
    )
}

#[test]
fn keyword_marks_restore_the_written_argument_spelling() {
    // `step` is keyword-capable, and `raise` alone would name it; `start` would print positionally.
    let src = "\
pub def main() uses io:
    io.print(f\"{iter.count(start=5, step=2).take(2).collect()}\")
    io.print(f\"{iter.count(5, 2).take(2).collect()}\")
";
    let (text, unused) = raised_marked(src);
    assert_eq!(text, src);
    assert!(unused.is_empty(), "{unused:?}");
}

#[test]
fn constructor_field_marks_restore_positional_and_keyword_fields() {
    let src = "\
class P:
    x: int
    y: int

pub def main() uses io:
    a = P(1, y=2)
    b = P(x=1, y=2)
    io.print(f\"{a.x} {b.y}\")
";
    let (text, unused) = raised_marked(src);
    assert_eq!(text, src);
    assert!(unused.is_empty(), "{unused:?}");
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
    let checked = yan_check::check("t.yag", src, &m);
    assert!(checked.is_empty(), "{checked:?}");
    let mut out = Vec::new();
    yan_interp::run(&m, &mut out).expect("runs");
    assert_eq!(String::from_utf8(out).unwrap(), "3\n");
}

#[test]
fn a_failed_assert_reports_its_line_and_sides() {
    let src = "pub def main():\n    x = 1 + 1\n    assert x == 3\n";
    let (m, _) = lower(src);
    let r = yan_interp::run(&m, &mut Vec::new());
    let Err(yan_interp::Stop::Panic(p)) = r else {
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
    let (m, _) = yan_yag::parse("t.yag", src, true);
    let strict = Options {
        strict: true,
        ..Options::default()
    };
    let (_, d) = elaborate("t.yag", src, &m.expect("parses"), strict).expect("supported");
    assert_eq!(
        d[0].message, "`str` has no indexing; use `name.chars()`",
        "{d:?}"
    );
}

#[test]
fn yn_spells_types_its_own_way() {
    let t = Ty::Fn(
        false,
        vec![Ty::Dict(Box::new(Ty::Str), Box::new(Ty::Int))],
        Box::new(Ty::Option(Box::new(Ty::List(Box::new(Ty::BigInt))))),
    );
    assert_eq!(t.yn(), "(Map[str, int]) -> List[BigInt]?");
    assert_eq!(
        t.to_string(),
        "Callable[[dict[str, int]], list[bigint] | None]"
    );
    assert_eq!(Ty::Unit.yn(), "()");
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
fn unhandled_failure_in_a_total_function_gets_the_default_template() {
    let src = "def parse(s: str) -> int fail str:\n    if s == \"\":\n        fail \"empty\"\n    return 1\n\n\
               def a(s: str) -> int:\n    x = parse(s)\n    return x + 1\n";
    let (_, d) = lower(src);
    let codes: Vec<_> = d.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(codes, ["E315"], "{d:?}");
    assert_eq!(d[0].fixes.len(), 1, "{d:?}");
    assert_eq!(d[0].fixes[0]["applicability"], "template");
    assert_eq!(d[0].fixes[0]["edits"][0]["text"], "parse(s) else ...");
}

#[test]
fn unhandled_failure_in_a_failing_function_gets_the_question_mark_fix() {
    let src = "def parse(s: str) -> int fail str:\n    if s == \"\":\n        fail \"empty\"\n    return 1\n\n\
               def a(s: str) -> int fail str:\n    x = parse(s)\n    return x + 1\n";
    let (_, d) = lower(src);
    let codes: Vec<_> = d.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(codes, ["E315"], "{d:?}");
    let fixed = apply_fixes(src, &d);
    assert!(fixed.contains("x = parse(s)?"), "{fixed}");
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
fn a_var_method_on_a_field_or_element_is_stored_back() {
    let src = [
        "class Q:",
        "    items: list[int]",
        "",
        "pub def main() uses io:",
        "    q = Q(items=[1])",
        "    xss = [[1], [2]]",
        "    d: dict[str, list[int]] = {\"a\": [1]}",
        "    q.items.append(3)",
        "    xss[1].append(4)",
        "    a = [0]",
        "    if \"a\" in d:",
        "        d[\"a\"].append(2)",
        "        a = d[\"a\"]",
        "    io.print(f\"{q.items} {xss} {a}\")",
        "",
    ]
    .join("\n");
    assert_eq!(run(&src), "[1, 3] [[1], [2, 4]] [1, 2]\n");
}

#[test]
fn upper_and_lower_take_a_char() {
    let src = "def both(s: str) -> str:\n    out = \"\"\n    for c in s.chars():\n\
        \x20       out += c.upper() + c.lower()\n    return out\n\n\
        pub def main() uses io:\n    io.print(both(\"aB\"))\n";
    assert_eq!(run(src), "AaBb\n");
}

#[test]
fn sum_any_and_all_take_lists_and_comprehensions() {
    let src = "pub def main() uses io:\n    xs = [1, 2, 3]\n\
        \x20   io.print(f\"{sum(x * 2 for x in xs)} {sum(xs)} {all(x > 0 for x in xs)} {any(x > 5 for x in xs)}\")\n";
    assert_eq!(run(src), "12 6 True False\n");
}

#[test]
fn a_call_result_can_be_called() {
    let src = "def inc(x: int) -> int:\n    return x + 1\n\n\
        def pick() -> Callable[[int], int]:\n    return inc\n\n\
        pub def main() uses io:\n    fs = [inc]\n    io.print(f\"{pick()(1)}{fs[0](5)}\")\n";
    assert_eq!(run(src), "26\n");
}

#[test]
fn post_names_resolve_in_a_hole_body() {
    let codes = |src: &str| {
        lower(src)
            .1
            .iter()
            .filter(|d| d.severity == yan_diag::Severity::Error)
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
    let (m, d) = yan_yag::parse("t.yag", src, false);
    assert!(
        d.iter().all(|d| d.severity != yan_diag::Severity::Error),
        "{d:?}"
    );
    let _ = elaborate("t.yag", src, &m.expect("parses"), Options::default());
}

#[test]
fn a_method_on_a_literal_lowers() {
    let src = "pub def main() uses io:\n    io.print(\"-\".join([\"a\", \"b\"]))\n";
    assert_eq!(run(src), "a-b\n");
}

#[test]
fn consumption_is_written() {
    let src = "def f(xs: list[int], c: bool) -> list[int] uses io:\n    ys = xs\n    if c:\n        return ys\n\
               \x20   zs = [1]\n    ws = zs\n    a, b = ws, [2]\n    a, b = b, a\n    for i in range(2):\n        v = a.copy()\n        io.print(f\"{len(v)}\")\n\
               \x20   return b\n\n\
               pub def main() uses io:\n    io.print(f\"{len(f([3], False))}\")\n";
    let text = yan_core::print(&lower(src).0);
    for line in [
        "let ys: List[i64] = copy xs",
        "let ws: List[i64] = move zs",
        "let _t5: List[i64] = move b",
        "let _t6: List[i64] = move a",
        "let v: List[i64] = copy a",
    ] {
        assert!(text.contains(line), "{line}\n{text}");
    }
    assert_eq!(run(src), "1\n1\n1\n");
}

#[test]
fn a_loop_that_returns_after_the_read_moves() {
    let src = "def f() -> int:\n    zs = [1]\n    for i in range(2):\n        ws = zs\n        return len(ws)\n    return 0\n\n\
               pub def main() uses io:\n    io.print(f\"{f()}\")\n";
    let text = yan_core::print(&lower(src).0);
    assert!(text.contains("let ws: List[i64] = move zs"), "{text}");
    assert_eq!(run(src), "1\n");
}

#[test]
fn print_takes_any_show_value() {
    let src = "pub def main() uses io:\n    print(42)\n    print([1, 2])\n    print(\"hi\")\n";
    assert_eq!(run(src), "42\n[1, 2]\nhi\n");
}

#[test]
fn exact_int_literals_type_as_floats() {
    let src = "def half(x: float) -> float:\n    return x / 2\n\n\
               pub def main() uses io:\n    x = 1.5 * 2\n    y = 2 * x\n\
               \x20   io.print(f\"{x} {y} {half(3)} {x ** 2} {2.0 ** -1} {x == 3}\")\n";
    assert_eq!(run(src), "3.0 6.0 1.5 9.0 0.5 True\n");
    let (_, d) = lower("pub def main() uses io:\n    x = 9007199254740993 * 1.0\n    print(x)\n");
    assert_eq!(
        d.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["E201"]
    );
}

#[test]
fn python_idioms_min_max_sorted_repetition() {
    let src = "pub def main() uses io:\n    xs = [3, 1, 2]\n    e: list[int] = []\n\
               \x20   ws = [[1, 1], [2], [3, 3]]\n\
               \x20   io.print(f\"{max(xs)} {min(xs, key=lambda x: -x)} {max(e, default=0)} {max(4, 5)}\")\n\
               \x20   io.print(f\"{sorted(ws, key=len, reverse=True)} {sorted(xs, reverse=True)}\")\n\
               \x20   ws.sort(key=len)\n    d = \"-\"\n    z = \"x\"\n\
               \x20   io.print(f\"{ws} {d * 3} {2 * [0]} {z * -1}|\")\n";
    assert_eq!(
        run(src),
        "3 3 0 5\n[[1, 1], [3, 3], [2]] [3, 2, 1]\n[[2], [1, 1], [3, 3]] --- [0, 0] |\n"
    );
}

#[test]
fn union_aliases_expand_after_fail() {
    let src = "class FsError:\n    msg: str\n\nclass Timeout:\n    secs: int\n\n\
               type IoErr = FsError | Timeout\n\n\
               def f(n: int) -> int fail IoErr:\n    if n > 0:\n        fail Timeout(secs=n)\n    return n\n\n\
               def g(n: int) -> int fail IoErr | FsError:\n    return f(n)?\n\n\
               pub def main() uses io:\n    r: Result[int, IoErr] = g(1)\n    io.print(f\"{r.is_ok()} {g(0) else 7}\")\n";
    assert_eq!(run(src), "False 0\n");
}

#[test]
fn nested_def_without_captures_is_lifted() {
    let src = "def helper() -> int:\n    return 1\n\n\
               pub def main() uses io:\n    def sq(x: int) -> int:\n        return x * x\n\
               \x20   def helper(s: str) -> str:\n        return s + \"!\"\n\
               \x20   s = \"a\"\n    io.print(f\"{sq(3)} {helper(s)} {other()}\")\n\n\
               def other() -> int:\n    return helper()\n";
    assert_eq!(run(src), "9 a! 1\n");
    let (_, d) = yan_yag::parse(
        "t.yag",
        "def f(n: int) -> int:\n    def g() -> int:\n        return n\n    return g()\n",
        false,
    );
    assert_eq!(
        d.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["E100"]
    );
}

#[test]
fn exclusive_returning_branches_share_one_use() {
    let src = "type Tree = Leaf() | Node(left: Tree, key: int, right: Tree)\n\n\
               def insert(t: Tree, x: int) -> Tree:\n    decreases t\n    match t:\n\
               \x20       case Leaf():\n            return Node(left=Leaf(), key=x, right=Leaf())\n\
               \x20       case Node(l, k, r):\n            if x < k:\n\
               \x20               return Node(left=insert(l, x), key=k, right=r)\n\
               \x20           if x > k:\n                return Node(left=l, key=k, right=insert(r, x))\n\
               \x20           return Node(left=l, key=k, right=r)\n";
    let (_, d) = lower(src);
    assert!(d.iter().all(|d| d.code != "E413"), "{d:?}");
}

#[test]
fn reassignment_ends_a_moved_name() {
    let src = "def f(n: int) -> int:\n    xs = [0]\n    for i in range(n):\n        ys = xs\n        xs = [i]\n        print(len(ys))\n    return len(xs)\n\n\
               def g(n: int) -> int:\n    xs = [0]\n    ys = xs\n    if n > 0:\n        xs = [n]\n    return len(xs) + len(ys)\n";
    let (_, d) = lower(src);
    let e413: Vec<_> = d
        .iter()
        .filter(|d| d.code == "E413")
        .map(|d| d.span.line)
        .collect();
    assert_eq!(e413, [11], "{d:?}");
}

#[test]
fn std_functions_called_only_in_a_contract_are_lowered() {
    let src = "def below(n: int) -> list[int]:\n    post all(p < n for p in out)\n    return list(range(n))\n\n\
               pub def main() uses io:\n    io.print(f\"{below(3)}\")\n";
    assert_eq!(run(src), "[0, 1, 2]\n");
}

#[test]
fn a_cancel_scope_needs_timeout_in_the_fail_type() {
    let body = "uses time:\n    with timeout(1 * s):\n        pass\n    return 0\n";
    let codes = |fail: &str| {
        let (_, d) = lower(&format!(
            "class Timeout:\n    pass\n\ndef f() -> int{fail} {body}"
        ));
        d.into_iter().map(|d| d.code).collect::<Vec<_>>()
    };
    assert_eq!(codes(""), ["E310"]);
    assert!(codes(" fail Timeout").is_empty());
}

#[test]
fn channels_run_and_their_waits_time_out() {
    let src = "def pair() -> int fail Cancelled:\n    tx, rx = Channel[int](2)\n    tx.send(1)?\n    tx.send(2)?\n    a = rx.recv()? else 0\n    b = rx.recv()? else 0\n    return a + b\n\n\
               def empty() -> int fail Cancelled | Timeout uses time:\n    tx, rx = Channel[int](1)\n    with timeout(5 * ms):\n        return rx.recv()? else 0\n    tx.send(0)?\n    return 0\n\n\
               def full() -> int fail Cancelled | Timeout uses time:\n    tx, _ = Channel[int](1)\n    with timeout(5 * ms):\n        tx.send(1)?\n        tx.send(2)?\n    return 0\n\n\
               pub def main() uses io, time:\n    p = pair() else _ -> -1\n    e = empty() else _ -> -1\n    f = full() else _ -> -1\n    io.print(f\"{p} {e} {f}\")\n";
    assert_eq!(run(src), "3 -1 -1\n");
}

#[cfg(unix)]
#[test]
fn a_deadline_stops_a_host_process() {
    let src = "def slow() -> int fail ProcError | Timeout uses proc, time:\n    with timeout(50 * ms):\n        _ = proc.run(Command(\"sleep\").arg(\"5\"))?\n        return 1\n    return 0\n\n\
               pub def main() uses io, proc, time:\n    n = slow() else _ -> -1\n    io.print(f\"{n}\")\n";
    let t = std::time::Instant::now();
    assert_eq!(run(src), "-1\n");
    assert!(t.elapsed() < std::time::Duration::from_secs(2));
}

#[test]
fn annotation_marks_restore_written_and_omitted_binding_types() {
    let src = "\
pub def main() uses io:
    a: int = 1
    b = 2
    xs: list[int] = []
    ys = []
    ys.append(a)
    io.print(f\"{a} {b} {xs} {ys}\")
";
    let (text, unused) = raised_marked(src);
    assert_eq!(text, src);
    assert!(unused.is_empty(), "{unused:?}");
}

#[test]
fn call_marks_restore_method_and_function_spellings() {
    let src = "\
pub def main() uses io:
    xs = [1, 2]
    s = \"ab\"
    io.print(f\"{len(xs)} {xs.len()} {len(s)} {s.char_count()}\")
";
    let (text, unused) = raised_marked(src);
    assert_eq!(text, src);
    assert!(unused.is_empty(), "{unused:?}");
}

#[test]
fn pre_marks_restore_contracts_from_asserts() {
    let src = "\
def half(x: int) -> int:
    pre x % 2 == 0
    return x // 2

pub def main() uses io:
    io.print(f\"{half(4)}\")
";
    let (text, unused) = raised_marked(src);
    assert_eq!(text, src);
    assert!(unused.is_empty(), "{unused:?}");
}

#[test]
fn literal_spellings_restore_non_decimal_integers() {
    let src = "\
pub def main() uses io:
    io.print(f\"{bytes.from_list([0xfb, 0b11, 1_0])}\")
    x = 0o17
    io.print(f\"{x}\")
";
    let (text, unused) = raised_marked(src);
    assert_eq!(text, src);
    assert!(unused.is_empty(), "{unused:?}");
}

#[test]
fn decorator_marks_restore_derive_and_prove() {
    let src = "\
@@prove

@derive(Eq, Hash)
class P:
    x: int

pub def main() uses io:
    io.print(\"hi\")
";
    let (text, unused) = raised_marked(src);
    assert_eq!(text, src);
    assert!(unused.is_empty(), "{unused:?}");
}

#[test]
fn post_marks_restore_contracts_and_out() {
    let src = "\
def f(x: int) -> int:
    post out > x
    return x + 1

def g(x: int, lo: int) -> int:
    post out >= lo
    if x < lo:
        return lo
    return x

def h(x: int) -> int:
    post out >= 0
    return 5

pub def main() uses io:
    io.print(f\"{f(1)} {g(0, 2)} {h(1)}\")
";
    let (text, unused) = raised_marked(src);
    assert_eq!(text, src);
    assert!(unused.is_empty(), "{unused:?}");
}

fn codes(src: &str) -> Vec<String> {
    lower(src).1.into_iter().map(|d| d.code).collect()
}

#[test]
fn derive_wire_accepts_nested_records_and_sums() {
    let src = "\
@derive(Wire)
class P:
    x: int
    tags: list[str]
    next: P | None

@derive(Wire)
type Shape = Dot(at: P) | Poly(pts: list[P], names: dict[str, float]) | Empty()

pub def main() uses io:
    io.print(\"hi\")
";
    assert_eq!(codes(src), Vec::<String>::new());
}

#[test]
fn derive_rejects_fields_that_do_not_implement_the_trait() {
    let src = "\
class Plain:
    n: int

@derive(Wire)
class A:
    f: Callable[[int], int]

@derive(Wire)
class B:
    p: Plain

@derive(Eq)
class C:
    p: Plain

@derive(Hash)
class D:
    x: float

pub def main() uses io:
    io.print(\"hi\")
";
    assert_eq!(codes(src), ["E282", "E282", "E282", "E282"]);
}

#[test]
fn derive_wire_capability_field_adds_a_note() {
    let src = "\
@derive(Wire)
class A:
    c: Io

pub def main() uses io:
    io.print(\"hi\")
";
    let d = lower(src).1;
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0].code, "E282");
    assert_eq!(d[0].notes, ["capabilities are never `Wire` (17.4)"]);
}

#[test]
fn nested_optional_levels_are_not_added_inside_containers() {
    let codes = |src: &str| -> Vec<String> { lower(src).1.into_iter().map(|d| d.code).collect() };
    let src = "type M = int | None\n\ndef f(xs: list[int | None]) -> int:\n    ys: list[M | None] = xs\n    return len(ys)\n";
    assert!(codes(src).contains(&"E201".to_string()));
    let ok = "type M = int | None\n\ndef f(x: int | None) -> int:\n    y: M | None = x\n    if y is None:\n        return 0\n    return 1\n";
    assert!(!codes(ok).contains(&"E201".to_string()));
}

#[test]
fn deny_promotes_lint_and_checks_codes() {
    let body =
        "def f(xs: list[int]) -> int:\n    if len(xs) == 1:\n        return 0\n    return xs[0]\n";
    let sev = |src: &str| {
        let (_, d) = lower(src);
        d.iter()
            .map(|d| (d.code.clone(), d.severity))
            .collect::<Vec<_>>()
    };
    use yan_diag::Severity::{Error, Warning};
    assert_eq!(sev(body), [("W820".to_owned(), Warning)]);
    assert_eq!(
        sev(&format!("@deny(W820)\n{body}")),
        [("W820".to_owned(), Error)]
    );
    let c = |src: &str| {
        sev(src)
            .into_iter()
            .map(|(c, _)| c)
            .collect::<std::collections::BTreeSet<_>>()
    };
    assert!(c(&format!("@deny(W820)\n@expect(W820, reason=\"x\")\n{body}")).contains("E277"));
    assert!(c(&format!("@deny(E501)\n{body}")).contains("E278"));
}

#[test]
fn wire_names_rename_json_keys_csv_columns_and_tags() {
    let src = "@derive(Wire, Show, Eq)\n\
               type R = { @wire(name: \"user-id\") id: int, name: str }\n\n\
               @derive(Wire, Show, Eq)\n\
               type S = @wire(name: \"circle\") Circle(@wire(name: \"radius\") r: float) | Unit()\n\n\
               main() uses io =\n\
               \x20 io.print(json.dump[R](R(id: 1, name: \"a\")))\n\
               \x20 io.print(json.dump[S](Circle(r: 1.5)))\n\
               \x20 io.print(csv.dump[R]([R(id: 1, name: \"a\")]))\n\
               \x20 s = json.parse[S](\"\\{\\\"tag\\\":\\\"circle\\\",\\\"radius\\\":2}\") else e -> Unit()\n\
               \x20 io.print(json.dump[S](s))\n\
               \x20 old = json.parse[R](\"\\{\\\"id\\\":1,\\\"name\\\":\\\"a\\\"}\") else e -> R(id: 0, name: \"\")\n\
               \x20 io.print(\"\\(old.id)\")\n";
    let (ast, d) = yan_yn::parse("t.yn", src);
    assert!(d.is_empty(), "{d:?}");
    let (h, d) = yan_yn::lower("t.yn", src, &ast).expect("lowers");
    assert!(d.is_empty(), "{d:?}");
    let (m, d) = elaborate("t.yn", src, &h, Options::default()).expect("supported");
    assert!(d.is_empty(), "{d:?}");
    let mut out = Vec::new();
    yan_interp::run(&m, &mut out).expect("runs");
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "{\"user-id\":1,\"name\":\"a\"}\n{\"tag\":\"circle\",\"radius\":1.5}\n\
         user-id,name\n1,a\n\n{\"tag\":\"circle\",\"radius\":2.0}\n0\n"
    );
}

#[test]
fn item_level_wire_names_tag_and_unknown() {
    let src = r#"@derive(Wire, Show, Eq)
@wire(names={"id": "user-id"}, unknown="ignore")
class R:
    id: int
    name: str

@derive(Wire, Show, Eq)
@wire(names={"Circle": "circle", "r": "radius"}, tag="kind")
type S = Circle(r: float) | Unit()

@derive(Wire, Show, Eq)
@wire(names={"Red": "r", "Green": "g"})
type C = Red() | Green()

@derive(Wire, Show, Eq)
class P:
    c: C

pub def main() uses io:
    io.print(json.dump[R](R(id=1, name="a")))
    io.print(json.dump[S](Circle(r=1.5)))
    io.print(csv.dump[R]([R(id=1, name="a")]))
    io.print(csv.dump[P]([P(c=Green())]))
    s = json.parse[S]("{\"kind\":\"circle\",\"radius\":2}") else e -> Unit()
    io.print(json.dump[S](s))
    r = json.parse[R]("{\"user-id\":3,\"name\":\"b\",\"x\":0}") else e -> R(id=0, name="")
    io.print(f"{r.id}")
    rs = csv.parse[R]("user-id,name,extra\n4,c,z\n") else e -> []
    for x in rs:
        io.print(f"{x.id}")
    ps = csv.parse[P]("c\nr\n") else e -> []
    for x in ps:
        io.print(f"{x.c}")
"#;
    assert_eq!(
        run(src),
        "{\"user-id\":1,\"name\":\"a\"}\n{\"kind\":\"circle\",\"radius\":1.5}\n\
         user-id,name\n1,a\n\nc\ng\n\n{\"kind\":\"circle\",\"radius\":2.0}\n3\n4\nRed()\n"
    );
}

#[test]
fn item_level_wire_errors() {
    let diags = |src: &str| {
        let (_, d) = lower(src);
        d.iter().map(|d| d.code.clone()).collect::<Vec<_>>()
    };
    let rec = |w: &str| format!("@derive(Wire)\n@wire({w})\nclass R:\n    a: int\n    b: int\n");
    assert_eq!(diags(&rec("names={\"c\": \"x\"}")), ["E331"]);
    assert_eq!(diags(&rec("names={\"a\": \"x\", \"b\": \"x\"}")), ["E331"]);
    assert_eq!(diags(&rec("tag=\"k\"")), ["E331"]);
    assert_eq!(diags(&rec("unknown=\"maybe\"")), ["E331"]);
    let sum = "@derive(Wire)\n@wire(tag=\"k\")\ntype S = A(k: int) | B()\n";
    assert_eq!(diags(sum), ["E330"]);
    assert!(diags(&rec("names={\"a\": \"x\"}")).is_empty());
}

#[test]
fn sibling_channel_waits_are_e713_only_in_plain_par() {
    let codes = |kw: &str| {
        let src = format!(
            "pub def main() uses io:\n    tx, rx = Channel[int](0)\n    {kw}:\n        \
             tx.send(1) else e -> io.print(\"x\")\n        \
             v = rx.recv() else e -> -1\n        io.print(f\"{{v}}\")\n"
        );
        let (_, d) = lower(&src);
        d.iter().map(|d| d.code.clone()).collect::<Vec<_>>()
    };
    assert_eq!(codes("par"), ["E713"]);
    assert!(codes("par concurrent").is_empty());
}

#[test]
fn par_concurrent_under_bound_pool_is_e708() {
    let codes = |h: &str| {
        let src = format!(
            "pub def main() uses io:\n    with sched = {h}:\n        par concurrent:\n            \
             io.print(\"a\")\n            io.print(\"b\")\n"
        );
        let (_, d) = lower(&src);
        d.iter().map(|d| d.code.clone()).collect::<Vec<_>>()
    };
    assert_eq!(codes("pool()"), ["E708"]);
    assert!(codes("pct()").is_empty());
}

fn edition_codes(value: &str) -> Vec<String> {
    let src = format!(
        "package: Package = Package(name=\"@a/b\", version=\"1.0.0\", yan=\">=1\", edition=\"{value}\")\n"
    );
    let (m, _) = yan_yag::parse("package.yag", &src, false);
    let (_, d) =
        elaborate("package.yag", &src, &m.expect("parses"), Options::default()).expect("supported");
    d.into_iter().map(|d| d.code).collect()
}

#[test]
fn named_editions_are_accepted() {
    for ok in ["next", "2026", "2027", "2031"] {
        assert!(edition_codes(ok).is_empty(), "{ok}");
    }
}

#[test]
fn malformed_editions_are_e332() {
    for bad in ["", "27", "2025", "20270", "v2027", "Next", "2027a"] {
        assert_eq!(edition_codes(bad), ["E332"], "{bad:?}");
    }
}

#[test]
fn unknown_manifest_fields_are_e018() {
    let codes = |field: &str| {
        let src = format!(
            "package: Package = Package(name=\"@a/b\", version=\"1.0.0\", edition=\"2026\", yan=\">=1\", {field}=1)\n"
        );
        let (m, _) = yan_yag::parse("package.yag", &src, false);
        let (_, d) = elaborate("package.yag", &src, &m.expect("parses"), Options::default())
            .expect("supported");
        d.into_iter().map(|d| d.code).collect::<Vec<_>>()
    };
    assert_eq!(codes("colour"), ["E018"]);
    assert!(codes("members").is_empty());
}

#[test]
fn missing_manifest_fields_are_e018() {
    let codes = |args: &str, file: &str| {
        let src = format!("package: Package = Package({args})\n");
        let (m, _) = yan_yag::parse(file, &src, false);
        let (_, d) =
            elaborate(file, &src, &m.expect("parses"), Options::default()).expect("supported");
        d.into_iter().map(|d| d.code).collect::<Vec<_>>()
    };
    assert_eq!(codes("name=\"@a/b\"", "package.yag"), ["E018"; 3]);
    assert!(codes("name=\"@a/b\"", "script.yag").is_empty());
    assert!(codes("name=\"@a/b\", members=[]", "package.yag").is_empty());
}

// Fail-transparent closures (spec 4.6): a failure inside a non-escaping closure argument is the caller's.
const FAILING_CALLBACK: &str = "class Bad:\n    code: int\n\nclass Other:\n    n: int\n\n\
    def check(x: int) -> int fail Bad:\n    if x < 0:\n        fail Bad(code=x)\n    return x\n\n\
    def apply(f: Callable[[int], int], x: int) -> int:\n    return f(x)\n\n";

#[test]
fn failing_closure_is_accepted_by_a_failing_caller() {
    let src = format!(
        "{FAILING_CALLBACK}def go(x: int) -> int fail Bad:\n    return apply(lambda v: check(v)?, x)\n\n\
         pub def main() uses io:\n    a = go(4) else err -> err.code\n    b = go(-2) else err -> err.code\n    io.print(f\"{{a}} {{b}}\")\n"
    );
    assert_eq!(run(&src), "4 -2\n");
}

#[test]
fn failing_closure_is_rejected_by_a_non_failing_caller() {
    let src = format!(
        "{FAILING_CALLBACK}def go(x: int) -> int:\n    return apply(lambda v: check(v)?, x)\n\npub def main() uses io:\n    io.print(f\"{{go(1)}}\")\n"
    );
    let (_, d) = lower(&src);
    assert!(d.iter().any(|d| d.code == "E310"), "{d:?}");
}

#[test]
fn failing_closure_is_rejected_by_a_caller_with_another_failure_type() {
    let src = format!(
        "{FAILING_CALLBACK}def go(x: int) -> int fail Other:\n    return apply(lambda v: check(v)?, x)\n\npub def main() uses io:\n    io.print(\"x\")\n"
    );
    let (m, d) = lower(&src);
    assert!(d.is_empty(), "{d:?}");
    let checked = yan_check::check("t.yag", &src, &m);
    assert!(checked.iter().any(|d| d.code == "E310"), "{checked:?}");
}

#[test]
fn an_arm_after_a_catch_all_is_w853() {
    let src = "\
pub def f(n: int) -> int:
    match n:
        case _:
            return 0
        case 1:
            return 1
";
    assert!(codes(src).contains(&"W853".to_owned()), "{:?}", codes(src));
}

#[test]
fn guards_that_cover_int_make_a_catch_all_unnecessary_and_a_later_arm_dead() {
    let ok = "\
pub def f(n: int) -> int:
    \"doc\"
    match n:
        case x if x < 0:
            return 0
        case x if x >= 0:
            return 1
";
    assert_eq!(codes(ok), Vec::<String>::new());
    let dead = "\
pub def f(n: int) -> int:
    \"doc\"
    match n:
        case x if x < 0:
            return 0
        case x if x >= 0:
            return 1
        case 5:
            return 2
";
    assert_eq!(codes(dead), ["W853"]);
    let partial = "\
pub def f(n: int) -> int:
    \"doc\"
    match n:
        case x if x < 0:
            return 0
";
    assert_eq!(codes(partial), ["E503"]);
}

#[test]
fn constant_time_rejects_secret_dependent_control_flow() {
    let body = |stmt: &str| {
        format!(
            "@constant_time\npub def f(s: Secret[int], t: list[int]) -> int:\n    \"doc\"\n    let k = s.expose()\n{stmt}    return 0\n"
        )
    };
    for bad in [
        "    if k > 0:\n        return 1\n",
        "    return t[k]\n",
        "    return 10 % k\n",
        "    let m = k + 1\n    while m > 0:\n        m -= 1\n",
    ] {
        assert!(codes(&body(bad)).contains(&"E424".to_owned()), "{bad}");
    }
    let ok = body("    let m = k + 1\n    let n = m * 2\n");
    assert!(!codes(&ok).contains(&"E424".to_owned()), "{:?}", codes(&ok));
}

#[test]
fn w853_offers_to_remove_the_whole_arm() {
    let src = "\
pub def f(n: int) -> int:
    \"doc\"
    match n:
        case _:
            return 0
        case 1:
            let a = 1
            return a
    return 2
";
    let d = lower(src).1;
    let w = d.iter().find(|d| d.code == "W853").expect("W853");
    let edit = &w.fixes[0]["edits"][0];
    assert_eq!(
        edit["search"],
        "        case 1:\n            let a = 1\n            return a\n"
    );
    assert_eq!(edit["text"], "");
}

fn cost_diags(src: &str) -> Vec<Diagnostic> {
    let (m, d) = yan_yag::parse("t.yag", src, false);
    assert!(d.is_empty(), "{d:?}");
    let (_, d) =
        elaborate("t.yag", src, &m.expect("parses"), Options::default()).expect("supported");
    d.into_iter()
        .filter(|d| d.code != "W813" && d.code != "N21")
        .collect()
}

#[test]
fn a_cost_bound_outside_the_proof_fragment_that_no_input_breaks_is_w851() {
    let src = "\
def countdown(n: int) -> int:
    pre n >= 0
    cost steps <= n
    t = n
    while t > 0:
        t -= 1
    return t
";
    let d = cost_diags(src);
    let codes: Vec<&str> = d.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(codes, ["W851"], "{d:?}");
    assert!(d[0].message.contains("counter"), "{}", d[0].message);
}

#[test]
fn linear_cost_bounds_are_proven_and_raise_nothing() {
    let src = "\
def total(xs: list[int]) -> int:
    cost steps <= len(xs), alloc <= 0
    t = 0
    for x in xs:
        t += x
    return t

def build(n: int) -> list[int]:
    pre n >= 0
    cost steps <= n, alloc <= n + 1
    ys: list[int] = []
    for i in range(n):
        ys.append(i)
    return ys

def twice(xs: list[int]) -> int:
    cost steps <= 2 * len(xs) + 2
    return total(xs) + total(xs)

def pick(a: int, b: int) -> int:
    cost steps <= 0, alloc <= 0
    return a * b + 1 if a > b else b
";
    assert!(cost_diags(src).is_empty(), "{:?}", cost_diags(src));
}

#[test]
fn a_negative_bound_is_refuted_at_a_negative_input() {
    let src = "\
def upto(n: int) -> int:
    cost steps <= n
    t = 0
    for i in range(n):
        t += i
    return t
";
    let d = cost_diags(src);
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0].code, "E618");
    assert!(d[0].message.contains("n = -1"), "{}", d[0].message);
}

#[test]
fn a_refuted_cost_bound_is_e618_with_the_smallest_witness() {
    let src = "\
def pairs(xs: list[int]) -> int:
    cost steps <= 2 * len(xs) + 1
    t = 0
    for x in xs:
        for y in xs:
            t += x * y
    return t
";
    let d = cost_diags(src);
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0].code, "E618");
    assert_eq!(d[0].quote, "2 * len(xs) + 1");
    assert!(d[0].message.contains("len(xs) = 2"), "{}", d[0].message);
    assert_eq!(d[0].counterexample, Some(serde_json::json!({"len(xs)": 2})));
}

#[test]
fn an_alloc_bound_counts_fresh_heap_values_and_growth_and_is_proven_or_refuted() {
    let src = "\
def build(n: int) -> list[int]:
    pre n >= 0
    cost alloc <= n + 1
    ys: list[int] = []
    for i in range(n):
        ys.append(i)
    return ys

def shown(n: int) -> int:
    cost alloc <= 1
    t = 0
    for i in range(n):
        t += len(str(i))
    return t
";
    let d = cost_diags(src);
    let codes: Vec<&str> = d.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(codes, ["E618"], "{d:?}");
    assert!(d[0].message.contains("n = 2"), "{}", d[0].message);
    assert!(d[0].message.contains("allocations"), "{}", d[0].message);
}

#[test]
fn a_cost_probe_skips_inputs_that_break_pre_and_sees_through_fail() {
    let src = "\
class Big:
    n: int

def count(n: int) -> int fail Big:
    pre n >= 2
    cost steps <= 4
    if n > 100:
        fail Big(n=n)
    t = 0
    for i in range(n):
        t += i
    return t
";
    let d = cost_diags(src);
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0].code, "E618");
    assert!(d[0].message.contains("n = 5"), "{}", d[0].message);
}

#[test]
fn a_cost_counter_panics_with_kind_contract_at_run_time() {
    let src = "\
def spin(n: int) -> int:
    cost steps <= 3
    t = 0
    for i in range(n):
        t += i
    return t

pub def main() uses io:
    io.print(f\"{spin(3)}\")
    io.print(f\"{spin(4)}\")
";
    let (m, _) = lower(src);
    let mut out = Vec::new();
    let Err(yan_interp::Stop::Panic(p)) = yan_interp::run(&m, &mut out) else {
        panic!("the counter does not stop `spin(4)`");
    };
    assert_eq!(String::from_utf8(out).expect("utf-8"), "3\n");
    assert_eq!(p.kind, "contract");
    assert!(p.fields.iter().any(|(n, _)| n == "steps"), "{:?}", p.fields);
}

const SHARED_TYPES: &str = "def vol(w: int, h: int, d: int) -> int:\n    return w * 100 + h * 10 + d\n\nclass Box:\n    w: int\n    h: int\n    d: int\n\npub def main() uses io:\n    a = 2\n    b = 3\n    c = 4\n    io.print(f\"{vol(a, b, c)} {vol(a, b, 5)} {Box(a, b, c).h}\")\n";

#[test]
fn str_slices_and_int_of_str_are_n50_n51() {
    let src = "pub def main() uses io:\n    s = \"hello world\"\n    io.print(s[0:5])\n    io.print(s[-5:])\n    io.print(s[::-1])\n    io.print(s[3:100])\n    n = int(\" 12 \")\n    io.print(f\"{n + 1}\")\n";
    let (m, d) = lower(src);
    let codes: Vec<_> = d.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(codes, ["N50", "N50", "N50", "N50", "N51"], "{d:?}");
    let mut out = Vec::new();
    yan_interp::run(&m, &mut out).expect("runs");
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "hello\nworld\ndlrow olleh\nlo world\n13\n"
    );
}

#[test]
fn itertools_enumerates_in_python_order() {
    let src = "pub def main() uses io:\n    xs = [1, 2, 3]\n    io.print(f\"{list(itertools.permutations(xs, 2))}\")\n    io.print(f\"{list(itertools.combinations(xs, 2))}\")\n    io.print(f\"{list(itertools.combinations_with_replacement([1, 2], 2))}\")\n    io.print(f\"{list(itertools.product([1, 2], [3, 4]))}\")\n    io.print(f\"{list(itertools.pairwise(xs))}\")\n    io.print(f\"{list(itertools.accumulate(xs))}\")\n    io.print(f\"{list(itertools.accumulate(xs, lambda a, b: a * b, initial=2))}\")\n";
    let (m, d) = lower(src);
    assert!(
        d.iter().all(|d| d.code != "E200" && d.code != "E201"),
        "{d:?}"
    );
    let mut out = Vec::new();
    yan_interp::run(&m, &mut out).expect("runs");
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "[[1, 2], [1, 3], [2, 1], [2, 3], [3, 1], [3, 2]]\n\
         [[1, 2], [1, 3], [2, 3]]\n\
         [[1, 1], [1, 2], [2, 2]]\n\
         [(1, 3), (1, 4), (2, 3), (2, 4)]\n\
         [(1, 2), (2, 3)]\n\
         [1, 3, 6]\n\
         [2, 2, 4, 12]\n"
    );
}

#[test]
fn int_of_a_bad_str_panics_with_conversion() {
    let (m, _) = lower("pub def main() uses io:\n    n = int(\"zz\")\n    io.print(f\"{n}\")\n");
    let mut out = Vec::new();
    let Err(yan_interp::Stop::Panic(p)) = yan_interp::run(&m, &mut out) else {
        panic!("int of a bad str does not panic");
    };
    assert_eq!(p.kind, "conversion");
}

#[test]
fn positional_args_for_shared_types_are_n44() {
    let (m, d) = lower(SHARED_TYPES);
    let codes: Vec<_> = d.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(codes, ["N49", "N49", "N49"], "{d:?}");
    let mut out = Vec::new();
    yan_interp::run(&m, &mut out).expect("runs");
    assert_eq!(String::from_utf8(out).unwrap(), "234 235 3\n");
}

#[test]
fn strict_keeps_e210_for_shared_types() {
    let (m, _) = yan_yag::parse("t.yag", SHARED_TYPES, true);
    let opts = Options {
        strict: true,
        ..Options::default()
    };
    let (_, d) = elaborate("t.yag", SHARED_TYPES, &m.expect("parses"), opts).expect("supported");
    assert!(d.iter().any(|d| d.code == "E210"), "{d:?}");
}

#[test]
fn empty_literal_argument_takes_the_optional_parameter_type() {
    let src = "def n(xs: list[int] | None) -> int:\n    ex n([]) == 0\n    if xs is None:\n        return -1\n    return len(xs)\n\n\
def m(d: dict[str, int] | None = None) -> int:\n    ex m({}) == 0\n    return 0\n\n\
def k() -> int:\n    return n([]) + m({})\n";
    let (_, d) = lower(src);
    assert!(d.iter().all(|d| d.code != "E241"), "{d:?}");
    let (_, d) = lower("def k() -> int:\n    ys = []\n    return 0\n");
    assert!(d.iter().any(|d| d.code == "E241"), "{d:?}");
}

fn accuracy_diags(src: &str) -> Vec<Diagnostic> {
    lower(src)
        .1
        .into_iter()
        .filter(|d| d.code == "W852")
        .collect()
}

#[test]
fn cancelling_square_roots_are_w852_with_the_conjugate_fix() {
    let src = "\
import math

def gap(x: float) -> float:
    pre x > 1.0
    return math.sqrt(x + 1.0) - math.sqrt(x)
";
    let d = accuracy_diags(src);
    assert_eq!(d.len(), 1, "{d:?}");
    let fix = d[0]
        .fixes
        .first()
        .map(|f| f.to_string())
        .unwrap_or_else(|| d[0].message.clone());
    assert!(
        fix.contains("1.0 / (math.sqrt(x + 1.0) + math.sqrt(x))"),
        "{fix}"
    );
}

#[test]
fn a_difference_of_squares_is_w852() {
    let src = "\
def d(x: float) -> float:
    pre x > 1.0
    return (x + 1.0) * (x + 1.0) - x * x
";
    let d = accuracy_diags(src);
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d[0].quote, "(x + 1.0) * (x + 1.0) - x * x");
    assert!(d[0].message.contains("loses 0.0"), "{}", d[0].message);
}

#[test]
fn accurate_or_narrowed_or_effectful_float_code_is_not_w852() {
    let src = "\
import math

def poly(x: float) -> float:
    return x * x + 2.0 * x

def near(x: float) -> float:
    pre x >= 0.0 and x <= 1.0
    return math.sqrt(x + 1.0) - math.sqrt(x)

def show(x: float) -> float uses io:
    io.print(\"x\")
    return math.sqrt(x + 1.0) - math.sqrt(x)
";
    assert_eq!(accuracy_diags(src), Vec::<Diagnostic>::new());
}

#[test]
fn w852_sampling_is_deterministic() {
    let src = "\
def gap(x: float) -> float:
    pre x > 1.0
    return 1.0 / (x + 1.0) - 1.0 / x
";
    let d = accuracy_diags(src);
    assert_eq!(d.len(), 1, "{d:?}");
    assert_eq!(d, accuracy_diags(src));
}

#[test]
fn raise_restores_extern_blocks() {
    let src = "unsafe extern \"C\" lib sort link \"sort\"\n\
               \x20 safe qsort_view(n: int, cmp: scoped (int, int) -> int)\n\
               \x20 unsafe free_greeting(p: int)\n\n\
               main() = 0\n";
    let (ast, d) = yan_yn::parse("t.yn", src);
    assert!(d.is_empty(), "{d:?}");
    let (h, d) = yan_yn::lower("t.yn", src, &ast).expect("lowers");
    assert!(d.is_empty(), "{d:?}");
    // E902 (outside an ffi module) does not stop the block being elaborated.
    let (m, _) = elaborate("t.yn", src, &h, Options::default()).expect("supported");
    let raised = yan_hir::raise::raise(&m, &yan_surface::TriviaTable::default(), &StdInfo)
        .expect("raises")
        .module;
    let x = raised
        .items
        .iter()
        .find_map(|i| match &i.kind {
            yan_hir::ItemKind::Extern(x) => Some(x),
            _ => None,
        })
        .expect("extern block");
    assert_eq!(
        (x.abi.as_str(), x.lib.as_str(), x.link.as_deref()),
        ("C", "sort", Some("sort"))
    );
    assert_eq!(x.items.len(), 2);
    let yan_hir::XItem::Fn { safe, params, .. } = &x.items[0] else {
        panic!("fn")
    };
    assert!(*safe);
    assert_eq!(params[1].callback, Some(yan_hir::Callback::Scoped));
}

#[test]
fn manifest_outside_the_data_profile_is_e144() {
    let codes = |src: &str, file: &str| {
        let (m, _) = yan_yag::parse(file, src, false);
        let (_, d) =
            elaborate(file, src, &m.expect("parses"), Options::default()).expect("supported");
        d.into_iter().map(|d| d.code).collect::<Vec<_>>()
    };
    let pkg = |v: &str| {
        format!(
            "package: Package = Package(name=\"@a/b\", version={v}, edition=\"2026\", yan=\">=1\")\n"
        )
    };
    assert!(codes(&pkg("\"1.0.0\""), "package.yag").is_empty());
    assert_eq!(codes(&pkg("\"1.\" + \"0\""), "package.yag"), ["E144"]);
    assert_eq!(codes(&pkg("str(1)"), "package.yag"), ["E144"]);
    assert!(codes(&pkg("\"1.\" + \"0\""), "script.yag").is_empty());
}

fn access_codes(claim: &str) -> Vec<String> {
    let src = format!(
        "unsafe extern \"C\" lib k link \"k\"\n\
         \x20 {claim}\n\
         \x20 safe scale(a: Array[8, f64], n: int) -> Array[8, f64]\n\n\
         main() = 0\n"
    );
    let (ast, d) = yan_yn::parse("t.yn", &src);
    assert!(d.is_empty(), "{d:?}");
    let (h, d) = yan_yn::lower("t.yn", &src, &ast).expect("lowers");
    assert!(d.is_empty(), "{d:?}");
    let (_, d) = elaborate("t.yn", &src, &h, Options::default()).expect("supported");
    d.into_iter()
        .filter(|d| d.code == "E284")
        .map(|d| d.message)
        .collect()
}

#[test]
fn access_claim_binds_every_array_operand_once() {
    let d = access_codes("@access(a: rows(0), out: elementwise)");
    assert!(d.is_empty(), "{d:?}");
    assert!(access_codes("@access(out: tile(4, 8), a: whole)").is_empty());
    let one = |c: &str| {
        let d = access_codes(c);
        assert_eq!(d.len(), 1, "{c}: {d:?}");
    };
    one("@access(a: whole)");
    one("@access(a: whole, a: whole, out: whole)");
    one("@access(a: whole, out: whole, b: whole)");
    one("@access(a: whole, n: whole, out: whole)");
    one("@access(a: rows(1), out: whole)");
    one("@access(a: tile(0, 4), out: whole)");
    one("@access(a: stripes(1), out: whole)");
}

#[test]
fn access_claim_on_extern_takes_a_halo() {
    let d = access_codes("@access(a: rows(0, halo: 1), out: elementwise)");
    assert!(d.is_empty(), "{d:?}");
    assert!(access_codes("@access(a: tile(4, 4, halo: 2), out: whole)").is_empty());
    let one = |c: &str| {
        let d = access_codes(c);
        assert_eq!(d.len(), 1, "{c}: {d:?}");
    };
    one("@access(a: rows(0, halo: 1, halo: 1), out: whole)");
    one("@access(a: rows(0, halo: -1), out: whole)");
    one("@access(a: rows(0, size: 1), out: whole)");
    one("@access(a: elementwise(halo: 1), out: whole)");
    one("@access(a: whole, out: rows(0, halo: 1))");
}
