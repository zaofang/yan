use super::*;

fn run(src: &str) -> String {
    let (m, d) = yong_jian::parse("t.jian", src, false);
    assert!(d.is_empty(), "{d:?}");
    let (y, d) =
        elaborate("t.jian", src, &m.expect("parses"), Options::default()).expect("supported");
    assert!(d.is_empty(), "{d:?}");
    let checked = yong_check::check("t.jian", src, &y);
    assert!(checked.is_empty(), "{checked:?}\n{}", yong_core::print(&y));
    let mut out = Vec::new();
    yong_interp::run(&y, &mut out).expect("runs");
    String::from_utf8(out).expect("utf-8")
}

#[test]
fn prelude_bodies_come_from_std() {
    let src = "pub def main() uses io:\n    xs = [1, 2, 3]\n\
               \x20   io.print(f\"{sum(xs)} {any([False, True])} {all([True, False])}\")\n";
    assert_eq!(run(src), "6 True False\n");
}

#[test]
fn methods_take_their_signature_from_std() {
    let src = "pub def main() uses io:\n    d: dict[str, int] = {\"a\": 1, \"b\": 2}\n\
               \x20   r = d.remove(\"a\") else 0\n    xs = [1, 2]\n    xs.append(r)\n\
               \x20   io.print(f\"{len(d)} {xs} {xs.index(2) else -1}\")\n";
    assert_eq!(run(src), "1 [1, 2, 1] 1\n");
}

#[test]
fn every_intrinsic_method_instantiates() {
    let recv = |module: &str| match module {
        "text" => Some(Ty::Str),
        "list" => Some(Ty::List(Box::new(Ty::Int))),
        "set" => Some(Ty::Set(Box::new(Ty::Int))),
        "dict" => Some(Ty::Dict(Box::new(Ty::Str), Box::new(Ty::Int))),
        "result" => Some(Ty::Result(Box::new(Ty::Int), Box::new(Ty::Str))),
        _ => None,
    };
    for &(module, src) in yong_std::SOURCES {
        let Some(t) = recv(module) else { continue };
        let (m, _) = yong_jian::parse(module, src, true);
        for it in m.expect("parses").items {
            let j::ItemKind::Func(f) = &it.kind else {
                continue;
            };
            let is_method = f
                .params
                .first()
                .is_some_and(|p| matches!(p, j::Param::Value { name, .. } if name == "self"));
            let path = format!("std.{module}.{}", f.name);
            let d = stdlib::decl(&path).expect("declared");
            if is_method && d.intrinsic {
                assert!(d.method(&t).is_some(), "{path} on {t}");
            }
        }
    }
}
