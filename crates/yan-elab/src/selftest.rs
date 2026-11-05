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
fn raised_marked(src: &str) -> (String, Vec<String>) {
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
               \x20   zs = [1]\n    ws = zs\n    a, b = ws, [2]\n    a, b = b, a\n    for i in range(2):\n        v = a\n        io.print(f\"{len(v)}\")\n\
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
    let src = "def pair() -> int fail Cancelled:\n    tx, rx = Channel[int](2)\n    tx.send(1)?\n    tx.send(2)?\n    a = rx.recv()?\n    b = rx.recv()?\n    return a + b\n\n\
               def empty() -> int fail Cancelled | Timeout uses time:\n    _, rx = Channel[int](1)\n    with timeout(5 * ms):\n        return rx.recv()?\n    return 0\n\n\
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
