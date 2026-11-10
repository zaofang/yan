//! std.cluster and std.sim: the `sim` handler's nodes and which node code runs on (RFC 0063 D1).

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
        Bind::Plain(|a| match a {
            [Value::Int(n), Value::Int(seed)] if *n >= 1 => Ok(Value::Data {
                name: "Sim".into(),
                fields: vec![(
                    "nodes".into(),
                    Value::List((0..*n).map(|i| node_id(*seed, i)).collect()),
                )],
            }),
            [Value::Int(_), Value::Int(_)] => {
                crate::panic("precondition `nodes >= 1` of `sim` failed")
            }
            _ => bad_args(STD_SIM_SIM),
        }),
    ),
    (
        STD_CLUSTER_NODES,
        Bind::Plain(|a| Ok(Value::List(ids(&a[0]).to_vec()))),
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
    // Same-node and cross-node calls share one path until faults land (#929 step 3).
    (STD_CLUSTER_REACH, Bind::Plain(|a| Ok(a[0].clone()))),
    (
        STD_CLUSTER_WIRE,
        Bind::Plain(|a| {
            let [x, Value::Str(sc)] = a else {
                return bad_args(STD_CLUSTER_WIRE);
            };
            let sc: Schemas = serde_json::from_str(sc).expect("the elaborator's schema");
            let root = &sc.defs[&sc.root];
            let Def::Record(f) = root else {
                unreachable!("the root is a record")
            };
            let fail = |why: String| {
                crate::panic_with(
                    "wire",
                    format!("a call through `Remote` cannot encode its value: {why}"),
                )
            };
            let v = match to_val(&sc, &f[0].1, x) {
                Ok(v) => v,
                Err(w) => return fail(w),
            };
            let bytes = match encode(&sc, &Val::Record(vec![v])) {
                Ok(b) => b,
                Err(e) => return fail(format!("{e:?}")),
            };
            match decode(&sc, &bytes, Limits::default()) {
                Ok(Val::Record(mut vs)) => Ok(from_val(&sc, &f[0].1, vs.remove(0))),
                Ok(_) => unreachable!("the root is a record"),
                Err(e) => fail(format!("{e:?}")),
            }
        }),
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
        STD_CLUSTER_LEAVE,
        Bind::Vm(|vm: &mut Vm, _, _| {
            let me = vm.task_path.clone();
            leave(&mut vm.it, &me);
            vm.it.placed = None;
            Ok(Value::Unit)
        }),
    ),
];

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
