//! std.regex over interpreter values. A `Regex` is a record holding its pattern; the compiled
//! automata are cached by pattern text.

use std::cell::RefCell;
use std::collections::HashMap;

use regex::{Captures, Regex};

use super::{Key, R, Value, panic};

#[derive(Clone, Copy)]
enum Anchor {
    None,
    Start,
    Both,
}

thread_local! {
    static CACHE: RefCell<HashMap<String, Regex>> = RefCell::new(HashMap::new());
}

/// The pattern was checked when its `Regex` was made, so wrapping it cannot fail.
fn compiled(pattern: &str, a: Anchor) -> Regex {
    let p = match a {
        Anchor::None => pattern.to_owned(),
        Anchor::Start => format!(r"\A(?:{pattern})"),
        Anchor::Both => format!(r"\A(?:{pattern})\z"),
    };
    CACHE.with(|c| {
        c.borrow_mut()
            .entry(p.clone())
            .or_insert_with(|| yong_std::regex::build(&p).expect("checked by compile"))
            .clone()
    })
}

fn record(name: &str, fields: Vec<(&str, Value)>) -> Value {
    Value::Data {
        name: name.into(),
        fields: fields.into_iter().map(|(n, v)| (n.into(), v)).collect(),
    }
}

fn field<'v>(v: &'v Value, name: &str) -> &'v Value {
    match v {
        Value::Data { fields, .. } => {
            &fields
                .iter()
                .find(|(n, _)| n == name)
                .expect("std record")
                .1
        }
        _ => unreachable!("std record"),
    }
}

fn chars(s: &str, byte: usize) -> Value {
    Value::Int(s[..byte].chars().count() as i128)
}

fn found(re: &Regex, s: &str, c: &Captures) -> Value {
    let whole = c.get(0).expect("group 0");
    let groups = c
        .iter()
        .map(|g| super::native::opt(g.map(|g| Value::Str(g.as_str().into()))))
        .collect();
    let names = re
        .capture_names()
        .enumerate()
        .filter_map(|(i, n)| Some((Key(Value::Str(n?.into())), Value::Int(i as i128))))
        .collect();
    record(
        "Match",
        vec![
            ("whole", Value::Str(whole.as_str().into())),
            ("begin", chars(s, whole.start())),
            ("finish", chars(s, whole.end())),
            ("groups", Value::List(groups)),
            ("names", Value::Dict(names)),
        ],
    )
}

fn first(r: &Value, s: &str, a: Anchor) -> Value {
    let Value::Str(p) = field(r, "pattern") else {
        unreachable!("str field")
    };
    let re = compiled(p, a);
    super::native::opt(re.captures(s).map(|c| found(&re, s, &c)))
}

pub(super) fn compile(p: &str) -> Result<Value, Value> {
    match yong_std::regex::build(p) {
        Ok(_) => Ok(record("Regex", vec![("pattern", Value::Str(p.into()))])),
        Err(reason) => Err(record(
            "RegexError",
            vec![
                ("pattern", Value::Str(p.into())),
                ("reason", Value::Str(reason.into())),
            ],
        )),
    }
}

pub(super) fn call(name: &str, args: &[Value]) -> Option<R<Value>> {
    use Value::*;
    let pattern = |r: &Value| match field(r, "pattern") {
        Str(p) => p.clone(),
        _ => unreachable!("str field"),
    };
    Some(Ok(match (name, args) {
        ("std.regex.escape", [Str(s)]) => Str(regex::escape(s).into()),
        ("std.regex.search", [r, Str(s)]) => first(r, s, Anchor::None),
        ("std.regex.match", [r, Str(s)]) => first(r, s, Anchor::Start),
        ("std.regex.fullmatch", [r, Str(s)]) => first(r, s, Anchor::Both),
        ("std.regex.find_all", [r, Str(s)]) => {
            let re = compiled(&pattern(r), Anchor::None);
            List(re.captures_iter(s).map(|c| found(&re, s, &c)).collect())
        }
        // As Python: captured groups are kept between the pieces, "" for one that did not take part.
        ("std.regex.split", [r, Str(s)]) => {
            let re = compiled(&pattern(r), Anchor::None);
            let mut out = Vec::new();
            let mut last = 0;
            for c in re.captures_iter(s) {
                let m = c.get(0).expect("group 0");
                out.push(Str(s[last..m.start()].into()));
                out.extend(
                    c.iter()
                        .skip(1)
                        .map(|g| Str(g.map_or("", |g| g.as_str()).into())),
                );
                last = m.end();
            }
            out.push(Str(s[last..].into()));
            List(out)
        }
        ("std.regex.sub", [r, Str(repl), Str(s)]) => {
            let re = compiled(&pattern(r), Anchor::None);
            Str(re
                .replace_all(s, yong_std::regex::replacement(repl).as_str())
                .into_owned()
                .into())
        }
        ("std.regex.group", [m, Int(i)]) => match field(m, "groups") {
            List(gs) => match usize::try_from(*i).ok().and_then(|i| gs.get(i)) {
                Some(g) => g.clone(),
                None => return Some(panic("bounds")),
            },
            _ => unreachable!("list field"),
        },
        ("std.regex.named", [m, Str(n)]) => match field(m, "names") {
            Dict(ns) => match ns.get(&Key(Str(n.clone()))) {
                Some(Int(i)) => match field(m, "groups") {
                    List(gs) => gs[*i as usize].clone(),
                    _ => unreachable!("list field"),
                },
                _ => return Some(panic("bounds")),
            },
            _ => unreachable!("dict field"),
        },
        _ => return None,
    }))
}
