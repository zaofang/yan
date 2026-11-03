//! std.regex over interpreter values; the kernels are in `yan_rt::regex` (R561). A `Regex` is a
//! record holding its pattern.

use yan_rt::regex::{self as rx, Anchor, Captures, Regex};
use yan_std::registry::{Id, id::*};

use super::bind::{Bind, bad_args};
use super::native::opt;
use super::{Key, Value, out_of_range, panic_with};
use Value::*;

fn record(name: &str, fields: Vec<(&str, Value)>) -> Value {
    Data {
        name: name.into(),
        fields: fields.into_iter().map(|(n, v)| (n.into(), v)).collect(),
    }
}

fn field<'v>(v: &'v Value, name: &str) -> &'v Value {
    match v {
        Data { fields, .. } => {
            &fields
                .iter()
                .find(|(n, _)| n == name)
                .expect("std record")
                .1
        }
        _ => unreachable!("std record"),
    }
}

fn compiled(r: &Value, a: Anchor) -> Regex {
    match field(r, "pattern") {
        Str(p) => rx::compiled(p, a),
        _ => unreachable!("str field"),
    }
}

fn found(re: &Regex, s: &str, c: &Captures) -> Value {
    let whole = c.get(0).expect("group 0");
    let groups = c
        .iter()
        .map(|g| opt(g.map(|g| Str(g.as_str().into()))))
        .collect();
    let names = re
        .capture_names()
        .enumerate()
        .filter_map(|(i, n)| Some((Key(Str(n?.into())), Int(i as i128))))
        .collect();
    record(
        "Match",
        vec![
            ("whole", Str(whole.as_str().into())),
            ("begin", Int(rx::chars(s, whole.start()) as i128)),
            ("finish", Int(rx::chars(s, whole.end()) as i128)),
            ("groups", List(groups)),
            ("names", Dict(names)),
        ],
    )
}

fn first(r: &Value, s: &str, a: Anchor) -> Value {
    let re = compiled(r, a);
    opt(re.captures(s).map(|c| found(&re, s, &c)))
}

pub(super) fn compile(p: &str) -> Result<Value, Value> {
    match rx::build(p) {
        Ok(_) => Ok(record("Regex", vec![("pattern", Str(p.into()))])),
        Err(reason) => Err(record(
            "RegexError",
            vec![("pattern", Str(p.into())), ("reason", Str(reason.into()))],
        )),
    }
}

macro_rules! f {
    ($id:ident, |$a:ident| $pat:pat => $e:expr) => {
        (
            $id,
            Bind::Plain(|$a| match &*$a {
                $pat => Ok($e),
                #[allow(unreachable_patterns)]
                _ => bad_args($id),
            }),
        )
    };
}

pub(super) static BINDS: &[(Id, Bind)] = &[
    f!(STD_REGEX_ESCAPE, |a| [Str(s)] => Str(rx::escape(s).into())),
    f!(STD_REGEX_SEARCH, |a| [r, Str(s)] => first(r, s, Anchor::None)),
    f!(STD_REGEX_MATCH, |a| [r, Str(s)] => first(r, s, Anchor::Start)),
    f!(STD_REGEX_FULLMATCH, |a| [r, Str(s)] => first(r, s, Anchor::Both)),
    f!(STD_REGEX_FIND_ALL, |a| [r, Str(s)] => {
        let re = compiled(r, Anchor::None);
        List(re.captures_iter(s).map(|c| found(&re, s, &c)).collect())
    }),
    f!(STD_REGEX_SPLIT, |a| [r, Str(s)] => List(
        rx::split(&compiled(r, Anchor::None), s)
            .into_iter()
            .map(|p| Str(p.into()))
            .collect(),
    )),
    f!(STD_REGEX_SUB, |a| [r, Str(repl), Str(s)] => {
        Str(rx::sub(&compiled(r, Anchor::None), repl, s).into())
    }),
    f!(STD_REGEX_GROUP, |a| [m, Int(i)] => match field(m, "groups") {
        List(gs) => match usize::try_from(*i).ok().and_then(|i| gs.get(i)) {
            Some(g) => g.clone(),
            None => return out_of_range(*i, gs.len()),
        },
        _ => unreachable!("list field"),
    }),
    f!(STD_REGEX_NAMED, |a| [m, Str(n)] => match field(m, "names") {
        Dict(ns) => match ns.get(&Key(Str(n.clone()))) {
            Some(Int(i)) => match field(m, "groups") {
                List(gs) => gs[*i as usize].clone(),
                _ => unreachable!("list field"),
            },
            _ => return panic_with("bounds", format!("the pattern has no group named `{n}`")),
        },
        _ => unreachable!("dict field"),
    }),
];
