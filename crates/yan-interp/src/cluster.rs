//! std.cluster and std.sim: the `sim` handler's nodes and which node code runs on (RFC 0063 D1).

use crate::R;
use yan_std::registry::{Id, id::*};

use yan_rt::wire::{Def, Limits, Schema, Schemas, Val, decode, encode};

use super::bind::{Bind, bad_args};
use super::vm::Vm;
use super::{Interp, Key, Value};

/// The node task `path` runs on: the innermost node entered by it or a task it descends from
/// (D1.2), else node 0, the binding task's (D1.1).
pub(crate) fn current(it: &Interp, path: &[usize]) -> usize {
    it.nodes
        .iter()
        .rev()
        .find(|(p, _)| path.starts_with(p))
        .map_or(0, |n| n.1)
}

/// Task `path` runs on `node` until the matching [`leave`] (D1.3).
pub(crate) fn enter(it: &mut Interp, path: Vec<usize>, node: usize) {
    it.nodes.push((path, node));
}

pub(crate) fn leave(it: &mut Interp, path: &[usize]) {
    if let Some(k) = it.nodes.iter().rposition(|(p, _)| p == path) {
        it.nodes.remove(k);
    }
}

/// Node `i`'s id: the first 128 bits of BLAKE3 over `"yan.sim.node" || seed || i` (D6).
fn node_id(seed: i128, i: i128) -> Value {
    let mut h = blake3::Hasher::new();
    h.update(b"yan.sim.node");
    h.update(&seed.to_le_bytes());
    h.update(&i.to_le_bytes());
    let hex: String = h.finalize().as_bytes()[..16]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Value::Data {
        name: "Node".into(),
        fields: vec![("id".into(), Value::Str(hex.into()))],
    }
}

/// How many nodes the `sim` handler `h` has, or `None` when `h` is not one.
pub(crate) fn size(h: &Value) -> Option<usize> {
    matches!(h, Value::Data { name, .. } if name == "Sim").then(|| ids(h).len())
}

fn ids(h: &Value) -> &[Value] {
    match h {
        Value::Data { name, fields } if name == "Sim" => match &fields[0].1 {
            Value::List(ns) => ns,
            _ => &[],
        },
        _ => &[],
    }
}

/// The index of node `n` in the handler's view.
fn index(h: &Value, n: &Value) -> Option<usize> {
    ids(h).iter().position(|m| m == n)
}

pub(super) static BINDS: &[(Id, Bind)] = &[
    (
        STD_SIM_SIM,
        Bind::Cap(|it, a| match a {
            [Value::Int(n), Value::Int(seed), faults] if *n >= 1 => {
                let sim = Value::Data {
                    name: "Sim".into(),
                    fields: vec![
                        (
                            "nodes".into(),
                            Value::List((0..*n).map(|i| node_id(*seed, i)).collect()),
                        ),
                        ("faults".into(), faults.clone()),
                        ("seed".into(), Value::Int(*seed)),
                        ("epoch".into(), Value::Int(crate::time::mono_now(it))),
                    ],
                };
                it.sim = Some(sim.clone());
                it.seq.clear();
                Ok(sim)
            }
            [Value::Int(_), Value::Int(_), _] => {
                crate::panic("precondition `nodes >= 1` of `sim` failed")
            }
            _ => bad_args(STD_SIM_SIM),
        }),
    ),
    (
        STD_SIM_FAULTS,
        Bind::Plain(|a| {
            let names = ["drop", "delay", "partition", "heal", "cuts"];
            Ok(Value::Data {
                name: "FaultPlan".into(),
                fields: names
                    .iter()
                    .zip(a)
                    .map(|(n, v)| ((*n).into(), v.clone()))
                    .collect(),
            })
        }),
    ),
    (
        STD_CLUSTER_NODES,
        Bind::Plain(|a| Ok(Value::List(ids(&a[0]).to_vec()))),
    ),
    // One `Down` arrives on the receiver when the target stops (R411).
    (
        STD_CLUSTER_MONITOR,
        Bind::Vm(|vm: &mut Vm, _, a| {
            let id = crate::sync::chan(&a[1]);
            let owner = current(&vm.it, &vm.task_path);
            let it = &mut vm.it;
            it.chans
                .push(crate::sync::Chan::new(usize::MAX, usize::MAX));
            let c = it.chans.len() - 1;
            it.monitors.push((id, c, owner));
            deliver(it, id);
            crate::simfault::sweep(it);
            Ok(crate::sync::end("Receiver", c))
        }),
    ),
    (
        STD_CLUSTER_SELF_NODE,
        Bind::Vm(|vm: &mut Vm, _, a| {
            let k = current(&vm.it, &vm.task_path);
            match ids(&a[0]).get(k) {
                Some(n) => Ok(n.clone()),
                None => bad_args(STD_CLUSTER_SELF_NODE),
            }
        }),
    ),
    // The request message of a call through `Remote` (RFC 0063 D5); `take` reports its failure.
    (
        STD_CLUSTER_REACH,
        Bind::Vm(|vm: &mut Vm, _, a| {
            let me = vm.task_path.clone();
            let from = current(&vm.it, &me);
            let to = vm.it.actors[crate::sync::chan(&a[0])].home;
            crate::simfault::request(&mut vm.it, &me, from, to);
            Ok(a[0].clone())
        }),
    ),
    // The reply message: the call fails when it is lost or late, though the handler ran.
    (
        STD_CLUSTER_ARRIVE,
        Bind::Vm(|vm: &mut Vm, _, a| {
            let me = vm.task_path.clone();
            let outcome = std::mem::take(&mut a[0]);
            Ok(crate::simfault::reply(&mut vm.it, &me, outcome))
        }),
    ),
    // An argument that cannot be encoded fails the call `SchemaMismatch` before the handler runs.
    (
        STD_CLUSTER_WIRE,
        Bind::Vm(|vm: &mut Vm, _, a| match roundtrip(a, STD_CLUSTER_WIRE)? {
            Ok(v) => Ok(v),
            Err(_) => {
                crate::simfault::mismatch(&mut vm.it, &vm.task_path);
                Ok(std::mem::take(&mut a[0]))
            }
        }),
    ),
    // The `inout` copy of a call through `Remote` replaces its place unless the reply was lost.
    (
        STD_CLUSTER_COMMIT,
        Bind::Vm(|vm: &mut Vm, _, a| {
            if vm.it.replied {
                a[0] = std::mem::take(&mut a[1]);
            }
            Ok(Value::Unit)
        }),
    ),
    // A start argument of `cluster.spawn` that cannot be encoded has no call to fail.
    (
        STD_CLUSTER_WIRE_START,
        Bind::Vm(|_, _, a| match roundtrip(a, STD_CLUSTER_WIRE_START)? {
            Ok(v) => Ok(v),
            Err(why) => crate::panic_with("wire", format!("a start argument: {why}")),
        }),
    ),
    // An argument of a send through `Remote` that cannot be encoded fails the send (R408).
    (
        STD_CLUSTER_WIRE_SEND,
        Bind::Vm(
            |vm: &mut Vm, _, a| match roundtrip(a, STD_CLUSTER_WIRE_SEND)? {
                Ok(v) => Ok(v),
                Err(_) => {
                    vm.it.sending =
                        Some((vm.task_path.clone(), crate::sync::failure("SchemaMismatch")));
                    Ok(std::mem::take(&mut a[0]))
                }
            },
        ),
    ),
    (STD_CLUSTER_HANDLE, Bind::Plain(|a| Ok(a[0].clone()))),
    (
        STD_CLUSTER_FAULT_SEND,
        Bind::Vm(|vm: &mut Vm, _, a| {
            let me = vm.task_path.clone();
            let from = current(&vm.it, &me);
            let to = vm.it.actors[crate::sync::chan(&a[0])].home;
            crate::simfault::request_send(&mut vm.it, &me, from, to);
            Ok(Value::Unit)
        }),
    ),
    (
        STD_CLUSTER_ADMIT,
        Bind::Vm(|vm: &mut Vm, _, _| {
            let me = vm.task_path.clone();
            Ok(match crate::simfault::admit(&mut vm.it, &me) {
                Some(e) => crate::data("Err", "error", e),
                None => crate::data("Ok", "value", Value::Unit),
            })
        }),
    ),
    // The reply cannot be encoded: the handler ran, and the call fails `SchemaMismatch` (R408).
    (
        STD_CLUSTER_WIRE_REPLY,
        Bind::Vm(
            |vm: &mut Vm, _, a| match roundtrip(a, STD_CLUSTER_WIRE_REPLY)? {
                Ok(v) => Ok(v),
                Err(_) => {
                    crate::simfault::mismatch(&mut vm.it, &vm.task_path);
                    Ok(std::mem::take(&mut a[0]))
                }
            },
        ),
    ),
    (
        STD_CLUSTER_PLACE,
        Bind::Vm(|vm: &mut Vm, _, a| match index(&a[0], &a[1]) {
            Some(k) => {
                let me = vm.task_path.clone();
                if current(&vm.it, &me) != k {
                    vm.it.placed = Some(k);
                }
                enter(&mut vm.it, me, k);
                Ok(Value::Unit)
            }
            None => crate::panic("`On(node)` names a node outside this cluster"),
        }),
    ),
    (
        STD_CLUSTER_GROUP,
        Bind::Plain(|a| match &a[1] {
            Value::Str(_) => Ok(Value::Data {
                name: "Group".into(),
                fields: vec![("name".into(), a[1].clone())],
            }),
            _ => bad_args(STD_CLUSTER_GROUP),
        }),
    ),
    (
        STD_CLUSTER_GROUP_JOIN,
        Bind::Vm(|vm: &mut Vm, _, a| {
            let members = group_members(&mut vm.it, &a[0]);
            if !members.contains(&a[1]) {
                members.push(a[1].clone());
            }
            Ok(Value::Unit)
        }),
    ),
    (
        STD_CLUSTER_GROUP_LEAVE,
        Bind::Vm(|vm: &mut Vm, _, a| {
            group_members(&mut vm.it, &a[0]).retain(|m| *m != a[1]);
            Ok(Value::Unit)
        }),
    ),
    (
        STD_CLUSTER_GROUP_MEMBERS,
        Bind::Vm(|vm: &mut Vm, _, a| Ok(Value::List(group_members(&mut vm.it, &a[0]).clone()))),
    ),
    (
        STD_CLUSTER_LEAVE,
        Bind::Vm(|vm: &mut Vm, _, _| {
            let me = vm.task_path.clone();
            leave(&mut vm.it, &me);
            vm.it.placed = None;
            Ok(Value::Unit)
        }),
    ),
];

/// The members of the group `g` names, which the first use creates empty (R414).
fn group_members<'a>(it: &'a mut Interp, g: &Value) -> &'a mut Vec<Value> {
    let Value::Data { fields, .. } = g else {
        unreachable!("a `Group` is a record of its name")
    };
    let Value::Str(name) = &fields[0].1 else {
        unreachable!("a `Group` names itself with a string")
    };
    let name = name.to_string();
    let k = it
        .groups
        .iter()
        .position(|(n, _)| *n == name)
        .unwrap_or_else(|| {
            it.groups.push((name, Vec::new()));
            it.groups.len() - 1
        });
    &mut it.groups[k].1
}

/// An interpreter value as the `Wire` codec's `Val` of schema `t` (R402).
fn to_val(sc: &Schemas, t: &Schema, v: &Value) -> Result<Val, String> {
    let list = |t: &Schema, xs: &mut dyn Iterator<Item = &Value>| {
        xs.map(|x| to_val(sc, t, x)).collect::<Result<Vec<_>, _>>()
    };
    Ok(match (t, v) {
        (Schema::Unit, Value::Unit) => Val::Unit,
        (Schema::Bool, Value::Bool(b)) => Val::Bool(*b),
        (Schema::Int, Value::Int(i)) => {
            Val::Int(i64::try_from(*i).map_err(|_| format!("{i} exceeds 64 bits"))?)
        }
        (Schema::Float, Value::Float(f)) => Val::Float(*f),
        (Schema::Char, Value::Char(c)) => Val::Char(*c),
        (Schema::Str, Value::Str(s)) => Val::Str(s.to_string()),
        (Schema::Bytes, Value::Bytes(b)) => Val::Bytes(b.to_vec()),
        (Schema::List(e), Value::List(xs)) => Val::List(list(e, &mut xs.iter())?),
        (Schema::Set(e), Value::Set(xs)) => Val::List(list(e, &mut xs.iter().map(|k| &k.0))?),
        (Schema::Dict(k, e), Value::Dict(d)) => Val::Dict(
            d.iter()
                .map(|(a, b)| Ok((to_val(sc, k, &a.0)?, to_val(sc, e, b)?)))
                .collect::<Result<_, String>>()?,
        ),
        (Schema::Option(_), Value::Data { name, fields })
            if name.as_str() == "None" && fields.is_empty() =>
        {
            Val::Option(None)
        }
        (Schema::Option(e), Value::Data { name, fields }) if name.as_str() == "Some" => {
            Val::Option(Some(Box::new(to_val(sc, e, &fields[0].1)?)))
        }
        (Schema::Tuple(ts), Value::Tuple(xs)) if ts.len() == xs.len() => Val::Tuple(
            ts.iter()
                .zip(xs)
                .map(|(t, x)| to_val(sc, t, x))
                .collect::<Result<_, _>>()?,
        ),
        (Schema::Named(n), Value::Data { name, fields }) => match &sc.defs[n] {
            Def::Record(fs) => Val::Record(fields_val(sc, fs, fields)?),
            Def::Sum(vs) => {
                let k = vs
                    .iter()
                    .position(|(v, _)| v.as_str() == name.as_str())
                    .ok_or(format!("`{name}` is not a variant of `{n}`"))?;
                Val::Variant(k, fields_val(sc, &vs[k].1, fields)?)
            }
        },
        _ => return Err(format!("a value that is not a {t:?}")),
    })
}

fn fields_val(
    sc: &Schemas,
    fs: &[(String, Schema)],
    vs: &[(super::Name, Value)],
) -> Result<Vec<Val>, String> {
    fs.iter()
        .zip(vs)
        .map(|((_, t), (_, v))| to_val(sc, t, v))
        .collect()
}

/// The inverse of [`to_val`]; the codec only decodes values of the schema's shape.
fn from_val(sc: &Schemas, t: &Schema, v: Val) -> Value {
    let list = |t: &Schema, xs: Vec<Val>| {
        xs.into_iter()
            .map(|x| from_val(sc, t, x))
            .collect::<Vec<_>>()
    };
    match (t, v) {
        (_, Val::Unit) => Value::Unit,
        (_, Val::Bool(b)) => Value::Bool(b),
        (_, Val::Int(i)) => Value::Int(i.into()),
        (_, Val::Float(f)) => Value::Float(f),
        (_, Val::Char(c)) => Value::Char(c),
        (_, Val::Str(s)) => Value::Str(s.into()),
        (_, Val::Bytes(b)) => Value::Bytes(b.into()),
        (Schema::List(e), Val::List(xs)) => Value::List(list(e, xs)),
        (Schema::Set(e), Val::List(xs)) => Value::Set(list(e, xs).into_iter().map(Key).collect()),
        (Schema::Dict(k, e), Val::Dict(d)) => Value::Dict(
            d.into_iter()
                .map(|(a, b)| (Key(from_val(sc, k, a)), from_val(sc, e, b)))
                .collect(),
        ),
        (Schema::Option(_), Val::Option(None)) => Value::Data {
            name: "None".into(),
            fields: Vec::new(),
        },
        (Schema::Option(e), Val::Option(Some(x))) => Value::Data {
            name: "Some".into(),
            fields: vec![("value".into(), from_val(sc, e, *x))],
        },
        (Schema::Tuple(ts), Val::Tuple(xs)) => {
            Value::Tuple(ts.iter().zip(xs).map(|(t, x)| from_val(sc, t, x)).collect())
        }
        (Schema::Named(n), Val::Record(xs)) => {
            let Def::Record(fs) = &sc.defs[n] else {
                unreachable!("a record")
            };
            Value::Data {
                name: n.as_str().into(),
                fields: named(sc, fs, xs),
            }
        }
        (Schema::Named(n), Val::Variant(k, xs)) => {
            let Def::Sum(vs) = &sc.defs[n] else {
                unreachable!("a sum")
            };
            Value::Data {
                name: vs[k].0.as_str().into(),
                fields: named(sc, &vs[k].1, xs),
            }
        }
        (t, v) => unreachable!("{v:?} does not decode against {t:?}"),
    }
}

fn named(sc: &Schemas, fs: &[(String, Schema)], xs: Vec<Val>) -> Vec<(super::Name, Value)> {
    fs.iter()
        .zip(xs)
        .map(|((n, t), x)| (n.as_str().into(), from_val(sc, t, x)))
        .collect()
}

/// Delivers the `Stopped` of each monitor of actor `id`, which has just stopped.
pub(crate) fn deliver(it: &mut Interp, id: usize) {
    let Some(reason) = it.actors[id].stopped.clone() else {
        return;
    };
    let (fired, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut it.monitors)
        .into_iter()
        .partition(|(a, _, _)| *a == id);
    it.monitors = rest;
    for (_, c, _) in fired {
        let down = Value::Data {
            name: "Stopped".into(),
            fields: vec![("reason".into(), reason.clone())],
        };
        it.chans[c].q.push_back(down);
        it.progress += 1;
    }
}

/// Stops actor `id` and tells its monitors.
pub(crate) fn stop(it: &mut Interp, id: usize, reason: Value) {
    it.actors[id].stop(reason);
    deliver(it, id);
}

/// `x` after the encode and decode of a call through `Remote`, or why it cannot be.
fn roundtrip(a: &[Value], id: Id) -> R<Result<Value, String>> {
    let [x, Value::Str(sc)] = a else {
        return bad_args(id);
    };
    let sc: Schemas = serde_json::from_str(sc).expect("the elaborator's schema");
    let Def::Record(f) = &sc.defs[&sc.root] else {
        unreachable!("the root is a record")
    };
    let v = match to_val(&sc, &f[0].1, x) {
        Ok(v) => v,
        Err(w) => return Ok(Err(w)),
    };
    let bytes = match encode(&sc, &Val::Record(vec![v])) {
        Ok(b) => b,
        Err(e) => return Ok(Err(format!("{e:?}"))),
    };
    Ok(match decode(&sc, &bytes, Limits::default()) {
        Ok(Val::Record(mut vs)) => Ok(from_val(&sc, &f[0].1, vs.remove(0))),
        Ok(_) => unreachable!("the root is a record"),
        Err(e) => Err(format!("{e:?}")),
    })
}
