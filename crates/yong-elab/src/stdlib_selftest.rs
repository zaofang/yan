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

#[test]
fn fake_fs_reads_and_fails_with_fs_error() {
    let src = "pub def main() uses io:\n    with fs = fake_fs({\"a\": \"x\"}):\n\
               \x20       a = fs.read(\"a\") else \"?\"\n\
               \x20       b = fs.read(\"b\") else err -> err.reason.copy()\n\
               \x20       io.print(f\"{a} {b}\")\n";
    assert_eq!(run(src), "x not found\n");
}

#[test]
fn failures_inject_into_error_with_context() {
    let src = "class Bad:\n    msg: str\n\n\
               def inner() -> int fail Bad:\n    fail Bad(msg=\"inner\")\n\n\
               def plain() -> int fail Error:\n    return inner()?\n\n\
               def outer() -> int fail Error:\n    return inner().context(\"outer\")?\n\n\
               pub def main() uses io:\n    a = plain() else err -> len(err.context)\n\
               \x20   b = outer() else err -> err.message.char_count()\n    io.print(f\"{a} {b}\")\n";
    assert_eq!(run(src), "0 16\n");
}

#[test]
fn generic_std_bodies_lower_at_their_instance() {
    let src = "pub def main() uses io:\n    xs = [3, 1, 2]\n\
               \x20   for i, x in enumerate(xs):\n        io.print(f\"{i} {x}\")\n\
               \x20   ys = [9]\n    xs.extend(ys)\n\
               \x20   io.print(f\"{zip(xs, ['a', 'b'])} {reversed(xs)}\")\n";
    assert_eq!(
        run(src),
        "0 3\n1 1\n2 2\n[(3, 'a'), (1, 'b')] [9, 2, 1, 3]\n"
    );
}
