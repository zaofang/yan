use super::*;

fn lower(src: &str) -> (y::Module, Vec<Diagnostic>) {
    let (m, d) = yong_jian::parse("t.jian", src, false);
    assert!(
        d.iter().all(|d| d.severity != yong_diag::Severity::Error),
        "{d:?}"
    );
    elaborate("t.jian", src, &m.expect("parses"), false).expect("supported")
}

#[test]
fn hello_lowers_to_checked_yong() {
    let src = "pub def main() uses io:\n    x = 1 + 2\n    io.print(f\"{x}\")\n";
    let (m, d) = lower(src);
    assert!(d.is_empty(), "{d:?}");
    let checked = yong_check::check("t.jian", src, &m);
    assert!(checked.is_empty(), "{checked:?}");
    let text = yong_core::print(&m);
    assert!(text.contains("add_checked_i64"), "{text}");
}

fn run(src: &str) -> String {
    let (m, d) = lower(src);
    assert!(d.is_empty(), "{d:?}");
    let checked = yong_check::check("t.jian", src, &m);
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
