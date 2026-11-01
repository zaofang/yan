//! Non-gating tests for the native std functions (SQ28). Expected strings come from CPython 3.12.

use super::Value::{self, *};
use super::{bind, native, prelude};

fn call(name: &str, mut args: Vec<Value>) -> Value {
    match bind::call(name, &mut args) {
        Ok(Ok(v)) => v,
        _ => panic!("{name} failed"),
    }
}

fn s(x: &str) -> Value {
    Str(x.into())
}

fn shown(v: &Value) -> String {
    prelude::show(v, false)
}

#[test]
fn dict_keeps_insertion_order_across_removal() {
    let mut d = call("std.dict.new", vec![]);
    for (k, v) in [("b", 1), ("a", 2), ("c", 3), ("b", 4)] {
        let mut args = vec![d, s(k), Int(v)];
        bind::call("std.dict.insert", &mut args).unwrap().unwrap();
        d = args.swap_remove(0);
    }
    assert_eq!(shown(&d), "{'b': 4, 'a': 2, 'c': 3}");
    let mut args = vec![d, s("a")];
    assert_eq!(
        shown(&bind::call("std.dict.remove", &mut args).unwrap().unwrap()),
        "2"
    );
    assert_eq!(shown(&args[0]), "{'b': 4, 'c': 3}");
    assert_eq!(
        shown(&call("std.dict.get", vec![args[0].clone(), s("z")])),
        "None"
    );
}

#[test]
fn empty_set_shows_as_set() {
    assert_eq!(shown(&call("std.set.new", vec![])), "set()");
}

#[test]
fn splitlines_matches_cpython() {
    let lines = |x: &str| shown(&call("std.text.splitlines", vec![s(x)]));
    assert_eq!(
        lines("a\nb\r\nc\rd\x0be\u{85}f\u{2028}g\n"),
        "['a', 'b', 'c', 'd', 'e', 'f', 'g']"
    );
    assert_eq!(lines(""), "[]");
    assert_eq!(lines("a\n\nb\n"), "['a', '', 'b']");
}

#[test]
fn partition_and_index() {
    let p = |x: &str| shown(&call("std.text.partition", vec![s(x), s("=")]));
    assert_eq!(p("k=v=w"), "('k', '=', 'v=w')");
    assert_eq!(p("kv"), "('kv', '', '')");
    let xs = List(vec![Int(0), Int(1), Int(2)]);
    assert_eq!(
        shown(&call("std.list.index", vec![xs.clone(), Int(2)])),
        "2"
    );
    assert_eq!(shown(&call("std.list.index", vec![xs, Int(9)])), "None");
}

#[test]
fn parse_int_and_float_are_strict() {
    let int = |x: &str| shown(&call("std.text.parse_int", vec![s(x)]));
    assert_eq!(int("-42"), "-42");
    assert_eq!(int(" 4"), "None");
    assert_eq!(int("9223372036854775808"), "None");
    let float = |x: &str| shown(&call("std.text.parse_float", vec![s(x)]));
    assert_eq!(float("17.5"), "17.5");
    assert_eq!(float("1e3"), "1000.0");
    assert_eq!(float("x1"), "None");
}

#[test]
fn bigint_floor_division_rounds_down() {
    let big = |i: i128| call("std.prelude.bigint", vec![Int(i)]);
    let op = |o: &str, a: i128, b: i128| {
        let (Value::BigInt(a), Value::BigInt(b)) = (big(a), big(b)) else {
            unreachable!()
        };
        shown(&native::bigint_op(o, &a, &b).unwrap().unwrap())
    };
    assert_eq!(op("add", i64::MAX.into(), 1), "9223372036854775808");
    assert_eq!(op("floordiv", -7, 2), "-4");
    assert_eq!(op("mod", -7, 2), "1");
    assert_eq!(op("mod", 7, -2), "-1");
}

#[test]
fn float_repr_matches_cpython() {
    for (f, want) in [
        (0.1, "0.1"),
        (1e16, "1e+16"),
        (1e-5, "1e-05"),
        (123456789012345678.0, "1.2345678901234568e+17"),
        (-0.0, "-0.0"),
        (1.0 / 3.0, "0.3333333333333333"),
        (5e-324, "5e-324"),
        (f64::MAX, "1.7976931348623157e+308"),
        (0.0001, "0.0001"),
        (9999999999999998.0, "9999999999999998.0"),
    ] {
        assert_eq!(prelude::float_repr(f), want);
    }
}
