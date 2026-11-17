use super::*;

const SHAPES: &str = "pub class Rect:\n    w: int\n    h: int\n\n\
pub type Shape = Square(side: int) | Wide(rect: Rect)\n\n\
pub limit: int = 100\n\n\
pub def area(s: Shape) -> int:\n    match s:\n        case Square(side=n):\n            return n * n\n        case Wide(rect=r):\n            return r.w * r.h\n\n\
pub def clamp(n: int) -> int:\n    if n > limit:\n        return limit\n    return n\n";

const REPORT: &str = "pub def twice(r: shapes.Rect) -> int:\n    return shapes.clamp(shapes.area(shapes.Wide(rect=r))) * 2\n";

fn parse(src: &str) -> j::Module {
    let (m, d) = yan_yag::parse("t.yag", src, false);
    assert!(d.is_empty(), "{d:?}");
    m.expect("parses")
}

fn run(main: &str) -> Result<String, String> {
    let sources = [("shapes", SHAPES), ("report", REPORT)];
    let load = |n: &str| sources.iter().find(|(m, _)| *m == n).map(|(_, s)| parse(s));
    let env = PackageEnv {
        this: "main".into(),
        modules: ["main", "report", "shapes"].map(String::from).to_vec(),
        load: &load,
    };
    let (y, d) = match elaborate_pkg(
        "main.yag",
        main,
        &parse(main),
        Options::default(),
        None,
        Some(&env),
    ) {
        Ok((y, _, d)) => (y, d),
        Err(u) => return Err(u.what),
    };
    assert!(d.is_empty(), "{d:?}");
    let checked = yan_check::check("main.yag", main, &y);
    assert!(checked.is_empty(), "{checked:?}\n{}", yan_core::print(&y));
    let mut out = Vec::new();
    yan_interp::run(&y, &mut out).expect("runs");
    Ok(String::from_utf8(out).expect("utf-8"))
}

#[test]
fn a_qualified_name_pulls_the_item_and_what_it_names() {
    let main =
        "pub def main() uses io:\n    io.print(str(report.twice(shapes.Rect(w=20, h=30))))\n";
    assert_eq!(run(main).as_deref(), Ok("200\n"));
}

#[test]
fn a_variant_and_a_pattern_may_be_qualified() {
    let main = "pub def main() uses io:\n    match shapes.Square(side=3):\n        case shapes.Square(side=n):\n            io.print(str(n))\n        case shapes.Wide(rect=r):\n            io.print(\"wide\")\n";
    assert_eq!(run(main).as_deref(), Ok("3\n"));
}

#[test]
fn a_local_may_share_a_module_name() {
    let main = "pub def main() uses io:\n    shapes = 4\n    io.print(str(shapes))\n";
    assert_eq!(run(main).as_deref(), Ok("4\n"));
}

#[test]
fn two_modules_declaring_one_variant_name_are_refused() {
    let main = "type Shape = Square(side: int)\n\npub def main() uses io:\n    io.print(str(shapes.area(shapes.Square(side=1))))\n";
    let err = run(main).expect_err("refused");
    assert!(err.contains("variant name `Square`"), "{err}");
}
