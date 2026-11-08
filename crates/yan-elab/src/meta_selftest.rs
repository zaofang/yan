//! `std.meta` (RFC 0051 stage 1): `Type` handles and `info` at stage 0.

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

const MIRROR: &str = "type Point = { x: int, y: float }\n\n\
                      mirror(t: Type) -> Type =\n\
                      \x20 match meta.info(t)\n\
                      \x20   Record(info:) ->\n\
                      \x20     let fields = info.fields.map(f -> FieldInfo(name: f.name + \"2\", ty: f.ty, public: True, annotations: []))\n\
                      \x20     meta.record_type(RecordInfo(name: \"Mirror\", params: [], fields: fields, annotations: [], layout: Layout(size: 0, align: 1)))\n\
                      \x20   _ -> t\n\n";

#[test]
fn a_type_function_reifies_a_record() {
    let src = format!(
        "{MIRROR}\
         type Twin = mirror(Point)\n\
         type Again = mirror(Point)\n\n\
         main() uses io =\n\
         \x20 let a: Twin = Mirror(x2: 1, y2: 2.5)\n\
         \x20 let b: Again = Mirror(x2: 3, y2: 2.5)\n\
         \x20 io.print(\"\\(a.x2) \\(b.y2)\")\n"
    );
    assert_eq!(run(&src), "1 2.5\n");
}

#[test]
fn a_reified_name_that_is_taken_is_e265() {
    let src = format!(
        "{MIRROR}\
         type Mirror = Point\n\
         type Twin = mirror(Point)\n\n\
         main() uses io =\n\
         \x20 io.print(\"\")\n"
    );
    assert!(
        yn(&src).1.iter().any(|d| d.starts_with("E265")),
        "{:?}",
        yn(&src).1
    );
}

#[test]
fn type_functions_reify_sums_and_newtypes_and_chain() {
    let src = "type Shape = Circle(r: float) | Dot()\n\n\
               wrap(t: Type, n: int) -> Type =\n\
               \x20 match meta.info(t)\n\
               \x20   Sum(info:) ->\n\
               \x20     let vs = info.variants.map(v -> VariantInfo(name: v.name + \"\\(n)\", fields: v.fields))\n\
               \x20     meta.sum_type(SumInfo(name: \"S\\(n)\", params: [], variants: vs, annotations: [], layout: Layout(size: 0, align: 1)))\n\
               \x20   _ -> meta.newtype(NewtypeInfo(name: \"N\\(n)\", inner: t, annotations: []))\n\n\
               type Wrapped = wrap(Shape, 1)\n\
               type Id = wrap(int, 2)\n\
               type Both = wrap(Wrapped, 3)\n\n\
               main() uses io =\n\
               \x20 let w: Wrapped = Circle1(r: 2.0)\n\
               \x20 let i: Id = N2(7)\n\
               \x20 let b: Both = Circle13(r: 1.0)\n\
               \x20 io.print(\"\\(w) \\(i.value) \\(b)\")\n";
    let out = run(src);
    assert!(
        out.contains("Circle1") && out.contains('7') && out.contains("Circle13"),
        "{out}"
    );
}

#[test]
fn yan_agent_applies_type_functions_but_does_not_define_one() {
    let src = "class Point:\n    x: int\n\ndef same(t: Type) -> Type:\n    return t\n\ndef main() -> None:\n    pass\n";
    let (m, d) = yan_yag::parse("t.yag", src, false);
    assert!(
        d.iter().all(|d| d.severity != yan_diag::Severity::Error),
        "{d:?}"
    );
    let (_, d) =
        elaborate("t.yag", src, &m.expect("parses"), Options::default()).expect("supported");
    assert!(d.iter().any(|d| d.code == "E283"), "{d:?}");
}

#[test]
fn a_type_function_called_at_run_time_is_e262() {
    let src = format!(
        "{MIRROR}\
         main() uses io =\n\
         \x20 let t = mirror(Point)\n\
         \x20 io.print(\"\")\n"
    );
    assert!(
        yn(&src).1.iter().any(|d| d.starts_with("E262")),
        "{:?}",
        yn(&src).1
    );
}

#[test]
fn an_application_may_appear_in_any_type_position() {
    let src = format!(
        "{MIRROR}\
         type Box = {{ inner: mirror(Point), more: List[mirror(Point)] }}\n\n\
         twin(p: mirror(Point)) -> mirror(Point) =\n\
         \x20 p\n\n\
         main() uses io =\n\
         \x20 let a: mirror(Point) = Mirror(x2: 1, y2: 2.5)\n\
         \x20 let b = Box(inner: twin(a), more: [a])\n\
         \x20 io.print(\"\\(b.inner.x2) \\(b.more.len())\")\n"
    );
    assert_eq!(run(&src), "1 1\n");
}

#[test]
fn application_arguments_are_stage_0_constants_and_calls() {
    let src = "type Shape = Circle(r: float) | Dot()\n\n\
               let k: int = 3\n\n\
               two() -> int = 2\n\n\
               wrap(t: Type, n: int) -> Type =\n\
               \x20 meta.newtype(NewtypeInfo(name: \"N\\(n)\", inner: t, annotations: []))\n\n\
               main() uses io =\n\
               \x20 let a: wrap(int, k) = N3(7)\n\
               \x20 let b: wrap(int, two() + 1) = N3(8)\n\
               \x20 io.print(\"\\(a.value) \\(b.value)\")\n";
    assert_eq!(run(src), "7 8\n");
}

#[test]
fn an_application_that_depends_on_its_own_result_is_not_a_hang() {
    let src = format!(
        "{MIRROR}\
         type Loop = mirror(Loop)\n\n\
         main() uses io =\n\
         \x20 io.print(\"\")\n"
    );
    assert!(
        yn(&src).1.iter().any(|d| d.starts_with("E261")),
        "{:?}",
        yn(&src).1
    );
}

#[test]
fn applications_run_in_dependency_order() {
    let src = "wrap(t: Type, n: int) -> Type =\n\
               \x20 meta.newtype(NewtypeInfo(name: \"N\\(n)\", inner: t, annotations: []))\n\n\
               type Outer = wrap(Inner, 2)\n\
               type Inner = wrap(int, 1)\n\n\
               main() uses io =\n\
               \x20 let o: Outer = N2(N1(5))\n\
               \x20 io.print(\"\\(o.value.value)\")\n";
    assert_eq!(run(src), "5\n");
}

#[test]
fn info_reports_a_newtype_as_a_newtype() {
    let src = "type Meters = newtype[float]\n\n\
               kind(t: Type) -> str =\n\
               \x20 match meta.info(t)\n\
               \x20   Newtype(info:) -> \"newtype \\(info.name)\"\n\
               \x20   Record(info:) -> \"record\"\n\
               \x20   _ -> \"other\"\n\n\
               let k: str = #kind(Meters)\n\n\
               main() uses io =\n\
               \x20 io.print(k)\n";
    assert_eq!(run(src), "newtype Meters\n");
}

#[test]
fn info_reports_layout_of_records_and_sums() {
    let src = format!(
        "{TYPES}\
         lay(t: Type) -> str =\n\
         \x20 match meta.info(t)\n\
         \x20   Record(info:) -> \"\\(info.layout.size) \\(info.layout.align)\"\n\
         \x20   Sum(info:) -> \"\\(info.layout.size) \\(info.layout.align)\"\n\
         \x20   _ -> \"other\"\n\n\
         let a: str = #lay(Point)\n\
         let b: str = #lay(Shape)\n\
\n\
         main() uses io =\n\
         \x20 io.print(a)\n\
         \x20 io.print(b)\n"
    );
    assert_eq!(run(&src), "16 8\n24 8\n");
}
