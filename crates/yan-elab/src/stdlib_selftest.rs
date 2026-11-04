use super::*;

fn run(src: &str) -> String {
    let (m, d) = yan_yag::parse("t.yag", src, false);
    assert!(d.is_empty(), "{d:?}");
    let (y, d) =
        elaborate("t.yag", src, &m.expect("parses"), Options::default()).expect("supported");
    assert!(d.is_empty(), "{d:?}");
    let checked = yan_check::check("t.yag", src, &y);
    assert!(checked.is_empty(), "{checked:?}\n{}", yan_core::print(&y));
    let mut out = Vec::new();
    yan_interp::run(&y, &mut out).expect("runs");
    String::from_utf8(out).expect("utf-8")
}

#[test]
fn prelude_bodies_come_from_std() {
    let src = "pub def main() uses io:\n    xs = [1, 2, 3]\n\
               \x20   io.print(f\"{sum(xs)} {any([False, True])} {all([True, False])}\")\n";
    assert_eq!(run(src), "6 True False\n");
}

#[test]
fn prelude_sum_takes_floats_in_the_fixed_order() {
    let src = "pub def main() uses io:\n    xs = [0.5, 0.25, -0.0]\n\
               \x20   io.print(f\"{sum(xs)} {sum([-0.0])}\")\n";
    assert_eq!(run(src), "0.75 -0.0\n");
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
        m if m.starts_with("collections.") => {
            let (n, k) = crate::ty::collection(&m["collections.".len()..]).expect("a type");
            Some(Ty::Coll(n, vec![Ty::Int; k]))
        }
        _ => None,
    };
    for &(module, src) in yan_std::SOURCES {
        let Some(t) = recv(module) else { continue };
        let (m, _) = yan_yag::parse(module, src, true);
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
            if is_method && d.intrinsic && !d.typed_at_call() {
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

#[test]
fn regex_matches_like_python() {
    let src = "pub def main() uses io:\n    r = regex.compile(r\"(?P<y>\\d+)-(\\d+)\")\n\
               \x20   m = r.search(\"on 2024-10\") else return\n\
               \x20   io.print(f\"{m.text()} {m.group(2)} {m.named('y')} {m.start()} {m.end()}\")\n\
               \x20   io.print(f\"{len(r.find_all('1-2 3-4'))} {r.sub('\\\\2/\\\\g<y>', 'a 1-2 b')}\")\n\
               \x20   io.print(f\"{regex.compile('(,)').split('a,b')}\")\n\
               \x20   v = regex.compile(r\"\\w+\").search(\"  h\u{e9}llo\") else return\n\
               \x20   io.print(f\"{v.start()} {v.end()}\")\n";
    assert_eq!(
        run(src),
        "2024-10 10 2024 3 10\n2 a 2/1 b\n['a', ',', 'b']\n2 7\n"
    );
}

#[test]
fn literal_patterns_are_checked_at_stage_0() {
    let src = "def f() -> int:\n    r = regex.compile(\"(a)\\\\1\")\n    return len(r.pattern)\n";
    let (m, _) = yan_yag::parse("t.yag", src, false);
    let (_, d) =
        elaborate("t.yag", src, &m.expect("parses"), Options::default()).expect("supported");
    assert!(d.iter().any(|d| d.code == "E545"), "{d:?}");
}

#[test]
fn collections_build_query_and_show() {
    let src = "pub def main() uses io:\n    d = deque([1, 2, 3], maxlen=3)\n\
               \x20   d.append(4)\n    d.rotate(1)\n    h = heap([5, 1, 3])\n    h.push(0)\n\
               \x20   m: sorted_dict[int, str] = sorted_dict()\n    m[2] = \"b\"\n    m[1] = \"a\"\n\
               \x20   s = sorted_set([3, 1, 2])\n    total = 0\n    for x in s:\n        total += x\n\
               \x20   io.print(f\"{d} {d.first() else -1} {h.pop()} {h} {m} {m.get(2) else '?'} {m.floor(5)} {s.range(2, 9)} {total}\")\n";
    assert_eq!(
        run(src),
        "deque([4, 2, 3], maxlen=3) 4 0 heap([1, 3, 5]) sorted_dict({1: 'a', 2: 'b'}) b (2, 'b') [2, 3] 6\n"
    );
}

#[test]
fn list_sorts_and_bisects_like_python() {
    let src = "pub def main() uses io:\n    xs = [3, 1, 2, 2]\n    xs.sort()\n\
               \x20   xs.insort(2)\n    xs.insert(-1, 9)\n\
               \x20   io.print(f\"{xs} {xs.bisect_left(2)} {xs.bisect_right(2)} {xs.slice_exact(1, 3)}\")\n";
    assert_eq!(run(src), "[1, 2, 2, 2, 9, 3] 1 4 [2, 2]\n");
}

/// The registry yan-std generates by line scan agrees with the parsed declarations (R561).
#[test]
fn registry_matches_declarations() {
    use yan_std::registry::{self, Effect};
    let mut declared = 0;
    for &(module, src) in yan_std::SOURCES {
        let (m, _) = yan_yag::parse(module, src, true);
        for it in m.expect("parses").items {
            let has = |d: &str| it.decorators.iter().any(|x| x.path == [d]);
            let defs: Vec<(j::FuncDef, bool)> = match it.kind {
                j::ItemKind::Func(f) if has("intrinsic") => vec![(f, false)],
                j::ItemKind::Trait(t) if has("capability") => {
                    t.sigs.into_iter().map(|(_, f)| (f, true)).collect()
                }
                _ => continue,
            };
            for (f, cap) in defs {
                declared += 1;
                let path = format!("std.{module}.{}", f.name);
                let id = registry::id(&path).unwrap_or_else(|| panic!("{path} not registered"));
                let e = registry::entry(id);
                let vals: Vec<_> = f
                    .params
                    .iter()
                    .filter_map(|p| match p {
                        j::Param::Value {
                            conv, default, ty, ..
                        } => Some((conv, default, ty)),
                        _ => None,
                    })
                    .collect();
                let effect = if cap {
                    Effect::Cap
                } else if vals.iter().any(|(c, ..)| **c == Some(j::Conv::Var)) {
                    Effect::Mutates
                } else {
                    Effect::Pure
                };
                let min = vals.iter().filter(|(_, d, _)| d.is_none()).count();
                let fails = f.ret.as_ref().is_some_and(|r| !r.fail.is_empty());
                let callback = vals.iter().any(|(.., t)| has_callable(t));
                assert_eq!(
                    (e.min_args, e.max_args, e.effect, e.fails, e.callback),
                    (min, vals.len(), effect, fails, callback),
                    "{path}"
                );
            }
        }
    }
    let registered = registry::ENTRIES
        .iter()
        .filter(|e| e.path.starts_with("std."))
        .count();
    assert_eq!(declared, registered);
}

fn has_callable(t: &j::Type) -> bool {
    match t {
        j::Type::Callable { .. } => true,
        j::Type::Named { args, .. } => args
            .iter()
            .any(|a| matches!(a, j::TArg::Type(t) if has_callable(t))),
        j::Type::None => false,
    }
}
