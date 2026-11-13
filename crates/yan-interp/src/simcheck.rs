//! std.sim: the PCT scheduler and trace validation against a spec (RFC 0065 D6, R999).

use std::collections::HashMap;

use yan_std::registry::{Id, id::*};

use super::bind::{Bind, bad_args};
use super::{Interp, Value, panic};

use yan_rt::pct::Pct;

pub(crate) fn pct(it: &mut Interp, a: &[Value]) -> Value {
    let [Value::Int(depth), Value::Int(seed), Value::Int(steps)] = a else {
        unreachable!("checked by the caller")
    };
    it.pct = Some(Pct::new(*seed as u64, *depth as i64, *steps as i64));
    Value::Data {
        name: "Pct".into(),
        fields: vec![
            ("depth".into(), Value::Int(*depth)),
            ("seed".into(), Value::Int(*seed)),
            ("steps".into(), Value::Int(*steps)),
        ],
    }
}

fn field<'a>(v: &'a Value, name: &str) -> &'a Value {
    match v {
        Value::Data { fields, .. } => &fields.iter().find(|(n, _)| &**n == name).expect("field").1,
        _ => unreachable!("a record"),
    }
}

fn text(v: &Value) -> &str {
    match v {
        Value::Str(s) => s,
        _ => unreachable!("a str"),
    }
}

fn list(v: &Value) -> &[Value] {
    match v {
        Value::List(l) => l,
        _ => unreachable!("a list"),
    }
}

/// The first step of `trace` that the spec's machine for its actor does not allow.
fn check(trace: &[Value], spec: &Value) -> Value {
    let mut states: HashMap<&str, &str> = HashMap::new();
    let mut edges: HashMap<(&str, &str, &str), &str> = HashMap::new();
    for m in list(field(spec, "machines")) {
        let actor = text(field(m, "actor"));
        states.insert(actor, text(field(m, "start")));
        for e in list(field(m, "edges")) {
            edges.insert(
                (actor, text(field(e, "src")), text(field(e, "event"))),
                text(field(e, "dst")),
            );
        }
    }
    for (i, step) in trace.iter().enumerate() {
        let (actor, event) = (text(field(step, "actor")), text(field(step, "event")));
        let state = states.get(actor).copied();
        match state.and_then(|s| edges.get(&(actor, s, event))) {
            Some(next) => {
                states.insert(actor, next);
            }
            None => {
                return Value::Data {
                    name: "Rejected".into(),
                    fields: vec![
                        ("index".into(), Value::Int(i as i128)),
                        ("actor".into(), Value::Str(actor.into())),
                        ("event".into(), Value::Str(event.into())),
                        ("state".into(), Value::Str(state.unwrap_or("").into())),
                    ],
                };
            }
        }
    }
    Value::Data {
        name: "Accepted".into(),
        fields: Vec::new(),
    }
}

pub(super) static BINDS: &[(Id, Bind)] = &[
    (
        STD_SIM_PCT,
        Bind::Cap(|it, a| match a {
            [Value::Int(d), Value::Int(_), Value::Int(k)] if *d >= 1 && *k >= 1 => Ok(pct(it, a)),
            [Value::Int(_), Value::Int(_), Value::Int(_)] => {
                panic("precondition `depth >= 1 and steps >= 1` of `pct` failed")
            }
            _ => bad_args(STD_SIM_PCT),
        }),
    ),
    (
        STD_SIM_CHECK_TRACE,
        Bind::Plain(|a| match a {
            [Value::List(trace), spec] => Ok(check(trace, spec)),
            _ => bad_args(STD_SIM_CHECK_TRACE),
        }),
    ),
];
