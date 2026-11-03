//! Yan IR: data types, the `.yir` text parser and the canonical printer.

pub mod ast;
pub mod hash;
pub use yan_std::intrinsics;
mod lexer;
mod parser;
mod printer;

pub use parser::parse;
pub use printer::{print, ty as type_text};

#[cfg(test)]
mod selftest {
    use super::*;

    const SAMPLE: &str = r#"-- header line kept
module app.pricing

@inline
@doc(text = "x", 1)
pub fn value[T: Type is Eq + Show, comptime N: Nat](let xs: List[i64], inout acc: i64, cap io: Io) -> i64 fail E | F
    requires lt_i64(let acc, let 10)
    ensures true
    decreases acc, xs
{
    let x: i64 = add_checked_i64(let acc, let 1)
    var y: Tuple(i64, bool) = (x, true)
    acc = x
    acc.f[i] = copy y.f
    let z: Option[i64] = std.list.get[i64](let xs, let 0)?
    let w: i64 = parse(let s)? via ParseError.into
    let r: Result[i64, E] = try parse(let s)
    let p: Point = Point { x: x, y: 2 }
    let e: Unit = Unit {}
    let u: Unit = ()
    let c: char = '\n'
    let s: Str = "a\"b\u{7}"
    let f: Fn(let i64) -> i64 uses io, log = fn [let x inout y](let a: i64) -> i64 uses none {
        ret a
    }
    if x {
        ret 1
    } else {
        ret -2
    }
    if x {}
    match z {
        Some { value: v, .. } => {
            ret v
        }
        None | Other.Thing => {}
        1..=9 => {}
        [a, b, .. rest] => {}
        (_, 'c', -1.5e3) => {}
    }
    jp k(let r: i64) {
        ret r
    }
    loop 'outer invariant le_i64(let x, let 9) decreases x {
        break 'outer
        continue 'outer
    }
    jump k(x)
    with io = dyn fake_io {
        par concurrent {
            {
                print(cap io, let s)
            }
            {}
        }
    }
    comptime for t: Type in types {}
    comptime if flag {
        assert eq_i64(let x, let 1)
    } else {
        unreachable
    }
    unsafe {
        asm("nop", in(reg) x, out("rax") y, clobber_abi("C"), options(nostack, pure))
    }
    drop x
    reuse y as Cons { head: a, tail: b }
    become value(let xs, inout acc, cap io)
}

gen squares(let n: i64) -> i64 {
    yield n
}

pub record Point[T: Type] { pub x: i64, y: f64 = 0.5, tag: Str = "p" }

record Empty {}

sum Shape { Circle { r: f64 }, Empty }

newtype Percent = i64 where le_i64(let x, let 100)

trait Show[T: Type] {
    type Out
    fn show(let x: T) -> Str
        ensures true
    fn twice(let x: T) -> Str {
        ret x
    }
    derived {}
}

unsafe impl Show[i64] for dyn Box[T] {
    type Out = Str
    pub fn show(let x: i64) -> Str {
        ret x
    }
}

const LIMIT: Map[Str, List[i64]] = Config { a: [1, 2], b: (1, "x") }

test pricing.works {
    assert true
}

unsafe extern "C" lib m link "libm" {
    safe fn cos(x: f64) -> f64 uses none nonblocking
    unsafe fn printf(fmt: Ptr[u8], ...) -> i32
    unsafe fn sort(cmp: scoped Fn(let i64) -> i64) -> Unit from cmp
    type Handle drop close
    safe static errno: i32
}

fn refined(let x: {v: i64 | ge_i64(let v, let 0)}) -> Never {
    unreachable
}
"#;

    fn roundtrip(src: &str) -> String {
        let (m, d) = parse("t.yir", src);
        assert!(
            d.is_empty(),
            "{:#?}",
            d.iter().map(|d| d.render_line(None)).collect::<Vec<_>>()
        );
        let once = print(&m.unwrap());
        let (m2, d2) = parse("t.yir", &once);
        assert!(
            d2.is_empty(),
            "reparse failed:\n{once}\n{:?}",
            d2.iter().map(|d| d.render_line(None)).collect::<Vec<_>>()
        );
        assert_eq!(print(&m2.unwrap()), once);
        once
    }

    #[test]
    fn every_production_round_trips() {
        let printed = roundtrip(SAMPLE);
        assert!(printed.starts_with("-- header line kept\nmodule app.pricing\n"));
    }

    #[test]
    fn canonical_text_is_a_fixed_point() {
        let canon = roundtrip(SAMPLE);
        assert_eq!(roundtrip(&canon), canon);
    }

    #[test]
    fn parsed_shape() {
        let (m, _) = parse(
            "t.yir",
            "module m\nfn f(let x: i64) -> i64 {\n    xs[i] = x\n    ret x\n}\n",
        );
        let m = m.unwrap();
        let ast::ItemKind::Fn(f) = &m.items[0].kind else {
            panic!()
        };
        let body = f.body.as_ref().unwrap();
        assert!(
            matches!(&body.stmts[0].kind, ast::StmtKind::Assign { place, .. } if place.projs.len() == 1)
        );
        assert_eq!(body.stmts[1].span.line, 4);
    }

    fn codes(src: &str) -> Vec<(String, u32, u32, String)> {
        parse("t.yir", src)
            .1
            .into_iter()
            .map(|d| (d.code, d.span.line, d.span.col, d.item))
            .collect()
    }

    #[test]
    fn missing_module_header() {
        assert_eq!(
            codes("fn f() -> Unit {}"),
            [("E100".into(), 1, 1, String::new())]
        );
    }

    #[test]
    fn errors_recover_at_item_boundaries() {
        let src =
            "module m\nfn f(let x i64) -> Unit {\n}\nfn ok() -> Unit {}\nrecord R { x i64 }\n";
        assert_eq!(
            codes(src),
            [
                ("E100".into(), 2, 12, "m.f".into()),
                ("E100".into(), 5, 14, "m.R".into())
            ]
        );
    }

    #[test]
    fn unclosed_block_is_e102_at_the_opener() {
        assert_eq!(
            codes("module m\nfn f() -> Unit {\n    ret ()\n"),
            [("E102".into(), 2, 16, "m.f".into())]
        );
    }

    #[test]
    fn break_value_must_be_on_the_same_line() {
        let src = "module m\nfn f() -> Unit {\n    loop 'l {\n        break 'l\n        x = y\n    }\n}\n";
        let (m, d) = parse("t.yir", src);
        assert!(d.is_empty());
        assert!(print(&m.unwrap()).contains("break 'l\n"));
    }

    /// 22.5: the parser never panics. Every prefix of the sample is a malformed input.
    #[test]
    fn prefixes_never_panic() {
        for (i, _) in SAMPLE.char_indices() {
            let (m, d) = parse("t.yir", &SAMPLE[..i]);
            assert!(m.is_some() || !d.is_empty());
        }
    }

    /// M0 criterion: the parser and printer round-trip every `conformance/yir` file.
    #[test]
    fn conformance_yir_files_round_trip() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/yir");
        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in entries.flatten() {
                let p = e.path();
                let in_fail = p.components().any(|c| c.as_os_str() == "fail");
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "yir") && !in_fail {
                    roundtrip(&std::fs::read_to_string(&p).unwrap());
                }
            }
        }
    }
}
