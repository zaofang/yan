//! `std.meta` (RFC 0051 stage 1): `Type` handles and `info` at stage 0.

use super::*;

fn yn(src: &str) -> (y::Module, Vec<String>) {
    let (ast, d) = yan_yn::parse("t.yn", src);
    assert!(d.is_empty(), "{d:?}");
    let (h, d) = yan_yn::lower("t.yn", src, &ast).expect("lowers");
    assert!(d.is_empty(), "{d:?}");
    let (m, d) = elaborate("t.yn", src, &h, Options::default()).expect("supported");
    (
        m,
        d.into_iter()
            .map(|d| format!("{} {}:{} {}", d.code, d.span.line, d.span.col, d.message))
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

const TYPES: &str = "type Point = { x: int, y: float }\n\
                     type Shape = Circle(r: float) | Rect(w: float, h: float) | Dot()\n\
                     type P = Point\n\n";

#[test]
fn info_describes_records_sums_newtypes_and_builtins() {
    let src = format!(
        "{TYPES}\
         describe(t: Type) -> str =\n\
         \x20 match meta.info(t)\n\
         \x20   Record(info:) -> \"record \\(info.name) \\(info.fields.len()) \\(info.fields[1].name)\"\n\
         \x20   Sum(info:) -> \"sum \\(info.name) \\(info.variants.len()) \\(info.variants[1].fields.len())\"\n\
         \x20   Newtype(info:) -> \"newtype \\(info.name)\"\n\
         \x20   Scalar(info:) -> \"scalar \\(info.name) \\(info.bits) \\(info.signed) \\(info.float)\"\n\
         \x20   Optional(inner:) -> \"optional\"\n\
         \x20   Builtin(info:) -> \"builtin \\(info.name) \\(info.args.len())\"\n\
         \x20   _ -> \"other\"\n\n\
         let a: str = #describe(Point)\n\
         let b: str = #describe(Shape)\n\
         let d: str = #describe(i32)\n\
         let e: str = #describe(List[int])\n\n\
         main() uses io =\n\
         \x20 io.print(a)\n\
         \x20 io.print(b)\n\
         \x20 io.print(d)\n\
         \x20 io.print(e)\n"
    );
    assert_eq!(
        run(&src),
        "record Point 2 y\nsum Shape 3 2\nscalar i32 32 True False\nbuiltin List 1\n"
    );
}

#[test]
fn handles_compare_by_identity_through_aliases() {
    let src = format!(
        "{TYPES}\
         same(a: Type, b: Type) -> bool = a == b\n\n\
         field_is(t: Type, u: Type) -> bool =\n\
         \x20 match meta.info(t)\n\
         \x20   Record(info:) -> info.fields[0].ty == u\n\
         \x20   _ -> False\n\n\
         let a: bool = #same(P, Point)\n\
         let b: bool = #same(Point, Shape)\n\
         let c: bool = #field_is(Point, int)\n\n\
         main() uses io =\n\
         \x20 io.print(\"\\(a) \\(b) \\(c)\")\n"
    );
    assert_eq!(run(&src), "True False True\n");
}

#[test]
fn type_info_needs_a_wildcard_arm() {
    let src = format!(
        "{TYPES}\
         n(t: Type) -> int =\n\
         \x20 match meta.info(t)\n\
         \x20   Record(info:) -> 1\n\n\
         main() uses io =\n\
         \x20 io.print(\"\")\n"
    );
    assert!(matches!(&yn(&src).1[..], [e] if e.starts_with("E503")));
}

#[test]
fn a_type_stays_at_stage_0() {
    let src = format!(
        "{TYPES}\
         ident(t: Type) -> Type = t\n\n\
         let t: Type = #ident(Point)\n\n\
         main() uses io =\n\
         \x20 io.print(\"\\(ident(Point))\")\n"
    );
    let d = yn(&src).1;
    assert!(
        d.len() == 2 && d.iter().all(|e| e.starts_with("E262")),
        "{d:?}"
    );
}
