//! Actor supervision (9.6, RFC 0056 D2 and D3): supervisor scopes, restartable children,
//! the three strategies and restart intensity.

use std::collections::VecDeque;
use std::rc::Rc;

use yan_std::registry::{Id, id::*};

use super::bind::{Bind, bad_args};
use super::vm::{Clo, Vm};
use super::{Interp, R, Value, panic_with};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Restart {
    Permanent,
    Transient,
    Temporary,
}

#[derive(Clone, Copy, PartialEq)]
enum Strategy {
    OneForOne,
    OneForAll,
    RestForOne,
}

struct Sup {
    strategy: Strategy,
    intensity: usize,
    period: i128,
    /// Its children, by actor, in start order.
    children: Vec<usize>,
    /// When each restart within the last `period` happened.
    restarts: VecDeque<i128>,
}

/// Every supervisor, the root first, and the open `with supervisor` scopes with the frame base
/// that opened each.
pub(crate) struct Sups {
    all: Vec<Sup>,
    open: Vec<(usize, usize)>,
}

impl Default for Sups {
    /// The root supervisor std binds before `main` and each test: `one_for_one`, 1 restart per
    /// 5 s (R957).
    fn default() -> Self {
        Sups {
            all: vec![Sup {
                strategy: Strategy::OneForOne,
                intensity: 1,
                period: 5_000_000_000,
                children: Vec::new(),
                restarts: VecDeque::new(),
            }],
            open: Vec::new(),
        }
    }
}

/// How an actor is supervised.
pub(crate) struct Child {
    pub(crate) sup: usize,
    pub(crate) restart: Restart,
    pub(crate) start: Option<Rc<Clo>>,
}

/// The supervisor of an actor started now, which takes it as its last child.
pub(crate) fn adopt(it: &mut Interp, id: usize, restart: Restart, start: Option<Rc<Clo>>) -> Child {
    let sup = it.sups.open.last().map_or(0, |s| s.0);
    it.sups.all[sup].children.push(id);
    Child {
        sup,
        restart,
        start,
    }
}

/// `with supervisor(spec):` opens a scope at frame `base`.
pub(crate) fn begin(it: &mut Interp, spec: &Value, base: usize) -> R<()> {
    let Value::Data { fields, .. } = spec else {
        return bad_args(STD_ACTOR_ONE_FOR_ONE);
    };
    let strategy = match &fields[0].1 {
        Value::Data { name, .. } if name == "OneForAll" => Strategy::OneForAll,
        Value::Data { name, .. } if name == "RestForOne" => Strategy::RestForOne,
        _ => Strategy::OneForOne,
    };
    let (Value::Int(intensity), Value::Duration(period)) = (&fields[1].1, &fields[2].1) else {
        return bad_args(STD_ACTOR_ONE_FOR_ONE);
    };
    it.sups.all.push(Sup {
        strategy,
        intensity: (*intensity).max(0) as usize,
        period: *period,
        children: Vec::new(),
        restarts: VecDeque::new(),
    });
    let id = it.sups.all.len() - 1;
    it.sups.open.push((id, base));
    Ok(())
}

/// The innermost scope ends: its children stop in reverse start order (9.6).
pub(crate) fn end(it: &mut Interp) {
    if let Some((s, _)) = it.sups.open.pop() {
        for c in it.sups.all[s].children.clone().into_iter().rev() {
            it.actors[c].stop(super::sync::failure("Shutdown"));
        }
    }
}

/// A frame at `base` returned: the scopes it opened end with it.
pub(crate) fn end_frame(it: &mut Interp, base: usize) {
    while it.sups.open.last().is_some_and(|s| s.1 >= base) {
        end(it);
    }
}

/// A handler of actor `id` failed, which stops it (RFC 0056 slice 3). Its supervisor restarts
/// it and, by strategy, its siblings, unless that exceeds the restart intensity, which stops
/// every child and escalates; nothing above a supervisor can restart it, so escalation ends the
/// run as a panic of kind `supervisor`.
pub(crate) fn crash(vm: &mut Vm, id: usize, reason: Value) -> R<()> {
    vm.it.actors[id].stop(reason);
    let (s, restart) = {
        let c = vm.it.actors[id].child();
        (c.sup, c.restart)
    };
    if restart == Restart::Temporary {
        return Ok(());
    }
    let now = super::time::mono_now(&vm.it);
    let sup = &mut vm.it.sups.all[s];
    while sup.restarts.front().is_some_and(|t| now - t >= sup.period) {
        sup.restarts.pop_front();
    }
    if sup.restarts.len() >= sup.intensity {
        let (n, p) = (sup.intensity, sup.period);
        for c in sup.children.clone().into_iter().rev() {
            vm.it.actors[c].stop(super::sync::failure("Shutdown"));
        }
        return panic_with(
            "supervisor",
            format!("a supervisor exceeded its restart intensity of {n} per {p} ns"),
        );
    }
    sup.restarts.push_back(now);
    let at = sup.children.iter().position(|c| *c == id).expect("a child");
    let group = match sup.strategy {
        Strategy::OneForOne => vec![id],
        Strategy::OneForAll => sup.children.clone(),
        Strategy::RestForOne => sup.children[at..].to_vec(),
    };
    for &c in group.iter().rev() {
        if c != id {
            vm.it.actors[c].stop(super::sync::failure("Shutdown"));
        }
    }
    for c in group {
        if vm.it.actors[c].child().restart == Restart::Temporary {
            continue;
        }
        let start = vm.it.actors[c]
            .child()
            .start
            .clone()
            .expect("a restartable child");
        let state = vm.apply(&start, Vec::new())?;
        vm.it.actors[c].restart_with(state);
    }
    Ok(())
}

fn spec(strategy: &str, a: &mut [Value]) -> R<Value> {
    let [
        intensity @ Value::Int(_),
        period @ Value::Duration(_),
        shutdown @ Value::Duration(_),
    ] = a
    else {
        return bad_args(STD_ACTOR_ONE_FOR_ONE);
    };
    let field = |n: &str, v: &mut Value| (n.into(), std::mem::take(v));
    Ok(Value::Data {
        name: "Supervisor".into(),
        fields: vec![
            (
                "strategy".into(),
                Value::Data {
                    name: strategy.into(),
                    fields: Vec::new(),
                },
            ),
            field("intensity", intensity),
            field("period", period),
            field("shutdown", shutdown),
        ],
    })
}

pub(super) static BINDS: &[(Id, Bind)] = &[
    (STD_ACTOR_ONE_FOR_ONE, Bind::Plain(|a| spec("OneForOne", a))),
    (STD_ACTOR_ONE_FOR_ALL, Bind::Plain(|a| spec("OneForAll", a))),
    (
        STD_ACTOR_REST_FOR_ONE,
        Bind::Plain(|a| spec("RestForOne", a)),
    ),
    (
        STD_ACTOR_START,
        Bind::Vm(|vm, _, a| {
            let [
                Value::Closure(start),
                Value::Int(restart),
                Value::Int(cap),
                Value::Int(policy),
            ] = a
            else {
                return bad_args(STD_ACTOR_START);
            };
            let restart = match restart {
                0 => Restart::Permanent,
                1 => Restart::Transient,
                _ => Restart::Temporary,
            };
            let (start, cap, policy) = (start.clone(), *cap, *policy);
            let home = crate::cluster::current(&vm.it, &vm.task_path);
            let state = vm.apply(&start, Vec::new())?;
            super::sync::spawn(&mut vm.it, home, state, cap, restart, Some(start), policy)
        }),
    ),
];
