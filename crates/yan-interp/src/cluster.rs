//! std.cluster and std.sim: the `sim` handler's nodes and which node code runs on (RFC 0063 D1).

use yan_std::registry::{Id, id::*};

use super::bind::{Bind, bad_args};
use super::vm::Vm;
use super::{Interp, Value};

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
        STD_CLUSTER_PLACE,
        Bind::Vm(|vm: &mut Vm, _, a| match index(&a[0], &a[1]) {
            Some(k) => {
                let me = vm.task_path.clone();
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
            Ok(Value::Unit)
        }),
    ),
];
