//! Subscripts (4.4, R457): inlined `sub at` reads, writes and diagnostics.

use super::*;

fn yn(src: &str) -> (y::Module, Vec<String>) {
    let (ast, d) = yan_yn::parse("t.yn", src);
    assert!(d.is_empty(), "{d:?}");
    let (h, d) = yan_yn::lower("t.yn", src, &ast).expect("lowers");
    assert!(d.is_empty(), "{d:?}");
    let (m, d) = elaborate(
        "t.yn",
        src,
        &h,
        Options {
            yn: true,
            ..Options::default()
        },
    )
    .expect("supported");
    (
        m,
        d.into_iter()
            .map(|d| format!("{} {}:{}", d.code, d.span.line, d.span.col))
            .collect(),
    )
}

fn run(src: &str) -> String {
    let (m, d) = yn(src);
    assert!(d.iter().all(|d| !d.starts_with('E')), "{d:?}");
    let mut out = Vec::new();
    yan_interp::run(&m, &mut out).expect("runs");
    String::from_utf8(out).unwrap()
}

const GRID: &str = "type Grid = { cells: List[int], log: int }\n\n\
    impl Index[int] for Grid\n\
    \x20 sub at(self, i: int) -> int =\n\
    \x20   let j = i * 2\n\
    \x20   yield self.cells[j]\n\
    \x20   self.log += 1\n\n";

#[test]
fn reads_writes_and_post_yield_code_run_once_per_statement() {
    let src = format!(
        "{GRID}main() uses io =\n\
         \x20 var g = Grid(cells: [1, 2, 3, 4, 5, 6], log: 0)\n\
         \x20 g[1] = 20\n\
         \x20 g[2] += 7\n\
         \x20 let v = g[1] + 1\n\
         \x20 print(v)\n\
         \x20 print(g.cells)\n\
         \x20 print(g.log)\n\
         \x20 0\n"
    );
    assert_eq!(run(&src), "21\n[1, 2, 20, 4, 12, 6]\n3\n");
}

#[test]
fn a_branching_body_selects_the_place() {
    let src = "type C = { a: int, b: int }\n\n\
        impl Index[bool] for C\n\
        \x20 sub at(self, k: bool) -> int =\n\
        \x20   if k\n\
        \x20     yield self.a\n\
        \x20   else\n\
        \x20     yield self.b\n\n\
        main() uses io =\n\
        \x20 var c = C(a: 1, b: 2)\n\
        \x20 c[False] += 5\n\
        \x20 print(c[True] + c[False])\n\
        \x20 0\n";
    assert_eq!(run(src), "8\n");
}

#[test]
fn a_read_bound_to_a_new_name_hoists_the_statements_before_the_branch() {
    let src = "type C = { a: int, b: int }\n\n\
        impl Index[int] for C\n\
        \x20 sub at(self, k: int) -> int =\n\
        \x20   let j = k + 1\n\
        \x20   if j > 2\n\
        \x20     yield self.a\n\
        \x20   else\n\
        \x20     yield self.b\n\n\
        main() uses io =\n\
        \x20 var c = C(a: 4, b: 1)\n\
        \x20 let x = c[1]\n\
        \x20 let y = c[5]\n\
        \x20 print(x + y)\n\
        \x20 0\n";
    assert_eq!(run(src), "5\n");
}

#[test]
fn e422_when_a_path_yields_zero_or_two_times() {
    let none = "type C = { a: int }\n\nimpl Index[bool] for C\n\
        \x20 sub at(self, k: bool) -> int =\n\
        \x20   if k\n\
        \x20     yield self.a\n";
    assert_eq!(yn(none).1, ["E422 4:7"]);
    let two = "type C = { a: int }\n\nimpl Index[bool] for C\n\
        \x20 sub at(self, k: bool) -> int =\n\
        \x20   yield self.a\n\
        \x20   yield self.a\n";
    assert_eq!(yn(two).1, ["E422 4:7"]);
}

#[test]
fn e423_when_the_inout_mode_cannot_write_the_yielded_value() {
    let src = "type C = { a: int }\n\nimpl Index[int] for C\n\
        \x20 sub at(self, k: int) -> int =\n\
        \x20   yield self.a + k\n\n\
        f() -> int =\n\
        \x20 var c = C(a: 1)\n\
        \x20 c[0] = 2\n\
        \x20 c[1]\n";
    assert_eq!(yn(src).1, ["E423 5:11"]);
}
