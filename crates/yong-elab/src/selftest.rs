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
