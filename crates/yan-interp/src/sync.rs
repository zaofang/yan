//! std.rc, std.sync, std.chan, std.actor and std.lattice: checked by the elaborator and the Yan Core checker.
//! The interpreter runs channels and local actors on one thread and stops with "not supported"
//! at the rest.

use std::collections::VecDeque;
use std::rc::Rc;

use yan_std::registry::{Id, id::*};

use super::bind::{Bind, bad_args};
use super::supervise::{self, Child, Restart};
use super::time::{mono_now, wait_out, wait_until};
use super::vm::{Clo, Vm};
use super::{Interp, R, Value, data, panic, unsupported};

/// A channel: its capacity, queued values and live `Sender`s. With none left and the queue
/// empty it is closed (9.5); a channel nothing ever closes counts `usize::MAX`.
pub(crate) struct Chan {
    pub cap: usize,
    pub q: VecDeque<Value>,
    pub senders: usize,
}

impl Chan {
    pub fn new(cap: usize, senders: usize) -> Self {
        Chan {
            cap,
            q: VecDeque::new(),
            senders,
        }
    }
}

/// A `Sender` goes out of scope: the last one closes its channel.
pub(crate) fn drop_sender(it: &mut Interp, v: &Value) {
    if let Value::Data { name, .. } = v
        && &**name == "Sender"
    {
        let s = &mut it.chans[chan(v)].senders;
        *s = s.saturating_sub(1);
        it.progress += 1;
    }
}

pub(crate) fn end(name: &str, chan: usize) -> Value {
    Value::Data {
        name: name.into(),
        fields: vec![("chan".into(), Value::Int(chan as i128))],
    }
}

pub(crate) fn chan(v: &Value) -> usize {
    match v {
        Value::Data { fields, .. } => match fields[0].1 {
            Value::Int(c) => c as usize,
            _ => unreachable!("a channel end or an actor"),
        },
        _ => unreachable!("a channel end or an actor"),
    }
}

/// A branch of a `par concurrent` at task level parks and the VM switches to another task (R390).
/// Elsewhere nothing can end the wait: under a deadline it fails with `Timeout` at the deadline
/// (9.4). Without one, every task parked or no task at all is a global deadlock (R396); a wait
/// in a call nested in a task stops the run.
fn blocked(it: &mut Interp, op: &str, chan: usize) -> R<Result<Value, Value>> {
    if it.park_ok && !it.force {
        it.park = Some((op == "recv", chan));
        return Ok(Ok(Value::Unit));
    }
    match wait_out(it) {
        Some(t) => Ok(Err(t)),
        None if it.force || it.in_par == 0 => panic("deadlock"),
        None => unsupported(format!(
            "a `{op}` that waits on another task, in the interpreter"
        )),
    }
}

/// A wait that cannot park leaves nothing else to run, so an idle actor runs a queued message
/// first, as it would at `par concurrent` quiescence (RFC 0056 D4); whether one ran.
fn idle_ran(vm: &mut Vm) -> R<bool> {
    if vm.it.park_ok && !vm.it.force {
        return Ok(false);
    }
    run_idle(vm)
}

/// In a cancelled task a blocking operation fails `Cancelled` (9.4).
fn cancelled(it: &Interp) -> Option<Value> {
    it.cancel.iter().any(|c| *c).then(|| Value::Data {
        name: "Cancelled".into(),
        fields: Vec::new(),
    })
}

/// What a send to a full mailbox does (R974).
#[derive(Clone, Copy, PartialEq)]
enum Policy {
    Block,
    DropOldest,
    DropNew,
    Reject,
}

/// A local actor (RFC 0056 D5): its state, `None` while a handler holds it, and its mailbox of
/// sent messages, each a closure that makes the handler call and the arguments it takes.
pub(crate) struct Actor {
    state: Option<Value>,
    capacity: usize,
    policy: Policy,
    mail: VecDeque<(Rc<Clo>, Value)>,
    /// A queued message is running, so the actor takes no other from its mailbox.
    running: bool,
    /// The task path of the handler holding the state, while one does.
    holder: Option<Vec<usize>>,
    /// Why a failed handler or its supervisor stopped it, until it restarts (R977).
    pub(crate) stopped: Option<Value>,
    /// Starts so far, so a `@reentrant` handler that lent the state cannot retake a restart's.
    starts: i128,
    /// Why the last start stopped, for a `@reentrant` handler that lent it the state.
    ended: Option<Value>,
    /// The task paths of the `@reentrant` handlers that lent the state, innermost last, each
    /// marked once its retake failed.
    lent: Vec<(Vec<usize>, bool)>,
    /// The node it runs on (RFC 0063 D1).
    pub(crate) home: usize,
    child: Child,
}

impl Actor {
    /// Its queue is dropped and its state with it (R813).
    pub(crate) fn stop(&mut self, reason: Value) {
        self.stopped = Some(reason);
        self.state = None;
        self.holder = None;
        self.mail.clear();
    }

    pub(crate) fn child(&self) -> &Child {
        &self.child
    }

    /// A restart starts from `state` with an empty mailbox (R813, R958).
    pub(crate) fn restart_with(&mut self, state: Value) {
        self.ended = self.stopped.take();
        self.starts += 1;
        self.state = Some(state);
    }
}

/// A new actor with `state`, a child of the supervisor bound here (R957).
pub(crate) fn spawn(
    it: &mut Interp,
    home: usize,
    state: Value,
    cap: i128,
    restart: Restart,
    start: Option<Rc<Clo>>,
    policy: i128,
) -> R<Value> {
    if cap < 1 {
        return panic("precondition `capacity >= 1` of `spawn` failed");
    }
    let id = it.actors.len();
    let child = supervise::adopt(it, id, restart, start);
    it.actors.push(Actor {
        state: Some(state),
        capacity: cap as usize,
        policy: match policy {
            1 => Policy::DropOldest,
            2 => Policy::DropNew,
            3 => Policy::Reject,
            _ => Policy::Block,
        },
        mail: VecDeque::new(),
        running: false,
        holder: None,
        stopped: None,
        starts: 0,
        ended: None,
        lent: Vec::new(),
        home,
        child,
    });
    Ok(end("Actor", id))
}

/// `Stopped(reason)` (R977).
fn stopped(reason: &Value) -> Value {
    Value::Data {
        name: "Stopped".into(),
        fields: vec![("reason".into(), reason.clone())],
    }
}

/// `Crashed(error)`: the shown text of a handler's failure (R977).
pub(crate) fn crashed(e: &Value) -> Value {
    Value::Data {
        name: "Crashed".into(),
        fields: vec![(
            "error".into(),
            Value::Str(super::prelude::show(e, false).into()),
        )],
    }
}

/// A failed send gives back its arguments (R976).
fn undelivered(reason: Value, args: Value) -> Value {
    Value::Data {
        name: "Undelivered".into(),
        fields: vec![("reason".into(), reason), ("args".into(), args)],
    }
}

/// Run actor `id`'s oldest queued message if the actor is idle; whether one ran.
fn run_queued(vm: &mut Vm, id: usize) -> R<bool> {
    let a = &mut vm.it.actors[id];
    if a.running || a.state.is_none() || a.stopped.is_some() {
        return Ok(false);
    }
    let Some((m, args)) = a.mail.pop_front() else {
        return Ok(false);
    };
    a.running = true;
    // The message runs under this std frame, so it cannot park; the caller still may.
    let park_ok = vm.it.park_ok;
    let r = vm.apply(&m, vec![args]);
    vm.it.park_ok = park_ok;
    vm.it.actors[id].running = false;
    vm.it.progress += 1;
    r.map(|_| true)
}

/// Run one queued message of the first idle actor that has one; whether one ran.
pub(crate) fn run_idle(vm: &mut Vm) -> R<bool> {
    for id in 0..vm.it.actors.len() {
        if run_queued(vm, id)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Task `me` waits on actor `id`, keyed apart from channels (R959): it parks where it can.
/// Where it cannot, the call deadline ends the wait with `Timeout`: the enclosing scope's, or
/// 5 s after the wait began (9.6). `None` means it parked.
fn wait(it: &mut Interp, id: usize, me: &[usize], what: &str) -> R<Option<Value>> {
    if it.park_ok && !it.force {
        if !it.wait_since.iter().any(|(p, _)| p == me) {
            let now = mono_now(it);
            it.wait_since.push((me.to_vec(), now));
        }
        it.park = Some((false, usize::MAX - 1 - id));
        return Ok(None);
    }
    let since = it.wait_since.iter().find(|(p, _)| p == me).map(|w| w.1);
    let d = match it.deadline {
        Some(d) => d,
        None if it.force || it.in_par == 0 => {
            since.unwrap_or_else(|| mono_now(it)) + DEFAULT_DEADLINE
        }
        None => {
            return unsupported(format!(
                "{what} that waits on a busy actor, in the interpreter"
            ));
        }
    };
    unwait(it, me);
    Ok(Some(wait_until(it, d)))
}

/// The deadline of a `call` or a `Block` send outside every deadline scope (9.6).
const DEFAULT_DEADLINE: i128 = 5_000_000_000;

/// Another handler holds actor `id`: the caller `me` waits until it is free (R397). `None` means
/// it parked.
fn wait_call(it: &mut Interp, id: usize, me: Vec<usize>) -> R<Option<Value>> {
    if it.park_ok && !it.force && !it.actor_waits.iter().any(|(p, _)| *p == me) {
        it.actor_waits.push((me.clone(), id));
    }
    wait(it, id, &me, "a handler call")
}

/// Task `me` takes actor `id`'s state for a handler, or fails; `None` means it parked to wait
/// for the handler holding it (R397).
fn take_state(vm: &mut Vm, id: usize, me: Vec<usize>) -> R<Option<Result<Value, Value>>> {
    if let Some(k) = vm.it.doomed.iter().position(|p| *p == me) {
        vm.it.doomed.swap_remove(k);
        unwait(&mut vm.it, &me);
        return Ok(Some(Err(failure("Deadlock"))));
    }
    if let Some(r) = &vm.it.actors[id].stopped {
        let e = stopped(r);
        unwait(&mut vm.it, &me);
        return Ok(Some(Err(e)));
    }
    if let Some(s) = vm.it.actors[id].state.take() {
        vm.it.actors[id].holder = Some(me.clone());
        unwait(&mut vm.it, &me);
        return Ok(Some(Ok(s)));
    }
    match cycle(&vm.it, &me, id) {
        Some(y) if y == me => {
            unwait(&mut vm.it, &me);
            return Ok(Some(Err(failure("Deadlock"))));
        }
        Some(y) if !vm.it.doomed.contains(&y) => {
            vm.it.doomed.push(y);
            vm.it.progress += 1;
        }
        _ => {}
    }
    Ok(wait_call(&mut vm.it, id, me)?.map(Err))
}

fn unwait(it: &mut Interp, me: &[usize]) {
    it.actor_waits.retain(|(p, _)| p != me);
    it.wait_since.retain(|(p, _)| p != me);
}

/// The state goes back to actor `id` after a handler ran.
fn give_back(it: &mut Interp, id: usize, state: Value) {
    let a = &mut it.actors[id];
    a.state = Some(state);
    a.holder = None;
    it.progress += 1;
}

/// The youngest caller of a wait cycle that `me` closes by waiting on actor `id`, if it closes
/// one (RFC 0056 D6). Each actor's holder waits for the actor it is parked on, and a task waits
/// for the tasks of its own `par concurrent` blocks, whose paths extend its own.
fn cycle(it: &Interp, me: &[usize], id: usize) -> Option<Vec<usize>> {
    let mut callers = vec![me.to_vec()];
    let mut at = id;
    for _ in 0..=it.actors.len() {
        let h = it.actors[at].holder.as_ref()?;
        if callers.iter().any(|c| c.starts_with(h)) {
            return callers.into_iter().max();
        }
        at = it.actor_waits.iter().find(|(p, _)| p == h)?.1;
        callers.push(h.clone());
    }
    None
}

pub(crate) fn failure(name: &str) -> Value {
    Value::Data {
        name: name.into(),
        fields: Vec::new(),
    }
}

pub(super) static BINDS: &[(Id, Bind)] = &[
    (
        STD_ACTOR_SPAWN,
        Bind::Vm(|vm, _, a| match a {
            [state, Value::Int(cap)] => {
                let home = crate::cluster::current(&vm.it, &vm.task_path);
                let s = std::mem::take(state);
                spawn(&mut vm.it, home, s, *cap, Restart::Temporary, None, 0)
            }
            _ => bad_args(STD_ACTOR_SPAWN),
        }),
    ),
    (
        STD_ACTOR_SPAWN_AS,
        Bind::Vm(|vm, _, a| match a {
            [state, Value::Int(cap), Value::Int(policy)] => {
                let home = crate::cluster::current(&vm.it, &vm.task_path);
                let s = std::mem::take(state);
                spawn(&mut vm.it, home, s, *cap, Restart::Temporary, None, *policy)
            }
            _ => bad_args(STD_ACTOR_SPAWN_AS),
        }),
    ),
    (STD_ACTOR_SEND_ONLY, Bind::Plain(|a| Ok(a[0].clone()))),
    (
        STD_ACTOR_TAKE,
        Bind::Vm(|vm, _, a| {
            let id = chan(&a[0]);
            let me = vm.task_path.clone();
            if let Some(e) = crate::simfault::refused(&mut vm.it, &me) {
                return Ok(data("Err", "error", e));
            }
            // A call is a barrier: the actor runs every message queued before it (R813).
            if !vm.it.doomed.contains(&me) {
                while run_queued(vm, id)? {}
            }
            Ok(match take_state(vm, id, me.clone())? {
                None => {
                    crate::simfault::forget(&mut vm.it, &me);
                    Value::Unit
                }
                Some(Ok(s)) => {
                    let home = vm.it.actors[id].home;
                    crate::cluster::enter(&mut vm.it, me, home);
                    data("Ok", "value", s)
                }
                Some(Err(e)) => {
                    crate::simfault::forget(&mut vm.it, &me);
                    data("Err", "error", e)
                }
            })
        }),
    ),
    (
        STD_ACTOR_LEND,
        Bind::Vm(|vm, _, a| {
            let id = chan(&a[0]);
            let me = vm.task_path.clone();
            give_back(&mut vm.it, id, std::mem::take(&mut a[1]));
            let a = &mut vm.it.actors[id];
            a.lent.push((me, false));
            Ok(Value::Int(a.starts))
        }),
    ),
    (
        STD_ACTOR_RETAKE,
        Bind::VmFallible(|vm, a| {
            let (id, me) = (chan(&a[0]), vm.task_path.clone());
            let Value::Int(since) = a[1] else {
                return bad_args(STD_ACTOR_RETAKE);
            };
            let act = &vm.it.actors[id];
            let r = if act.starts != since && act.stopped.is_none() {
                let ended = act.ended.as_ref().expect("a restarted actor stopped first");
                Err(stopped(ended))
            } else {
                match take_state(vm, id, me.clone())? {
                    None => return Ok(Ok(Value::Unit)),
                    Some(r) => r,
                }
            };
            let lent = &mut vm.it.actors[id].lent;
            let k = lent
                .iter()
                .rposition(|(p, _)| *p == me)
                .expect("lent first");
            match r {
                Ok(s) => {
                    lent.remove(k);
                    a[2] = s;
                    Ok(Ok(Value::Unit))
                }
                Err(e) => {
                    lent[k].1 = true;
                    Ok(Err(e))
                }
            }
        }),
    ),
    (
        STD_ACTOR_SEND,
        Bind::VmFallible(|vm, a| {
            let id = chan(&a[0]);
            let me = vm.task_path.clone();
            if let Some(e) = crate::simfault::lost_send(&mut vm.it, &me) {
                return Ok(Err(undelivered(e, std::mem::take(&mut a[1]))));
            }
            if let Some(r) = &vm.it.actors[id].stopped {
                let e = stopped(r);
                return Ok(Err(undelivered(e, std::mem::take(&mut a[1]))));
            }
            let full = |it: &Interp| it.actors[id].mail.len() >= it.actors[id].capacity;
            if full(&vm.it) {
                match vm.it.actors[id].policy {
                    Policy::Reject => {
                        return Ok(Err(undelivered(failure("Full"), std::mem::take(&mut a[1]))));
                    }
                    Policy::DropNew => return Ok(Ok(Value::Unit)),
                    Policy::DropOldest => {
                        vm.it.actors[id].mail.pop_front();
                    }
                    // A full mailbox parks a `Block` sender, so the actor runs its oldest
                    // message first; waiting is a cancellation point (9.6).
                    Policy::Block => {
                        if let Some(c) = cancelled(&vm.it) {
                            return Ok(Err(undelivered(c, std::mem::take(&mut a[1]))));
                        }
                        if !run_queued(vm, id)? {
                            let me = vm.task_path.clone();
                            return Ok(match wait(&mut vm.it, id, &me, "a send")? {
                                Some(t) => Err(undelivered(t, std::mem::take(&mut a[1]))),
                                None => Ok(Value::Unit),
                            });
                        }
                    }
                }
            }
            let me = vm.task_path.clone();
            unwait(&mut vm.it, &me);
            let Value::Closure(m) = std::mem::take(&mut a[2]) else {
                return bad_args(STD_ACTOR_SEND);
            };
            let args = std::mem::take(&mut a[1]);
            vm.it.actors[id].mail.push_back((m, args));
            vm.it.progress += 1;
            Ok(Ok(Value::Unit))
        }),
    ),
    (
        STD_ACTOR_PUT,
        Bind::Vm(|vm, _, a| {
            let me = vm.task_path.clone();
            crate::cluster::leave(&mut vm.it, &me);
            give_back(&mut vm.it, chan(&a[0]), std::mem::take(&mut a[1]));
            Ok(Value::Unit)
        }),
    ),
    (
        STD_ACTOR_SETTLE,
        Bind::Vm(|vm, _, a| {
            let id = chan(&a[0]);
            let me = vm.task_path.clone();
            let outcome = std::mem::take(&mut a[2]);
            crate::cluster::leave(&mut vm.it, &me);
            let failed = match &outcome {
                Value::Data { name, fields } if name == "Err" => Some(crashed(&fields[0].1)),
                _ => None,
            };
            let act = &mut vm.it.actors[id];
            if act.holder.as_ref() == Some(&me) {
                give_back(&mut vm.it, id, std::mem::take(&mut a[1]));
            } else {
                // A `@reentrant` handler failed while it lent the state, which stays with the
                // actor, or its retake failed and it holds nothing to settle.
                let k = act.lent.iter().rposition(|(p, _)| *p == me);
                if k.is_none_or(|k| act.lent.remove(k).1) {
                    return Ok(outcome);
                }
            }
            // A failed handler may have left the state half updated, so it stops the actor;
            // the caller still gets the failure (RFC 0060 D3).
            if let Some(reason) = failed {
                supervise::crash(vm, id, reason)?;
            }
            Ok(outcome)
        }),
    ),
    (
        STD_ACTOR_REPLY,
        Bind::CapFallible(|_, a| match std::mem::take(&mut a[0]) {
            Value::Data { name, mut fields } => {
                let v = fields.pop().expect("a result payload").1;
                Ok(if name == "Ok" { Ok(v) } else { Err(v) })
            }
            _ => bad_args(STD_ACTOR_REPLY),
        }),
    ),
    (STD_ACTOR_SHARE, Bind::Plain(|a| Ok(a[0].clone()))),
    (
        STD_CHAN_CHANNEL,
        Bind::Cap(|it, a| match a {
            [Value::Int(cap)] => {
                it.chans.push(Chan::new(*cap as usize, 1));
                let c = it.chans.len() - 1;
                Ok(Value::Tuple(vec![end("Sender", c), end("Receiver", c)]))
            }
            _ => bad_args(STD_CHAN_CHANNEL),
        }),
    ),
    (
        STD_CHAN_RECEIVER_RECV,
        Bind::VmFallible(|vm, a| {
            let c = chan(&a[0]);
            loop {
                if let Some(e) = cancelled(&vm.it) {
                    return Ok(Err(e));
                }
                crate::simfault::sweep(&mut vm.it);
                if let Some(v) = vm.it.chans[c].q.pop_front() {
                    vm.it.progress += 1;
                    return Ok(Ok(crate::native::opt(Some(v))));
                }
                if vm.it.chans[c].senders == 0 {
                    return Ok(Ok(crate::native::opt(None)));
                }
                if !idle_ran(vm)? {
                    return blocked(&mut vm.it, "recv", c);
                }
            }
        }),
    ),
    (
        STD_CHAN_RECEIVER_TRY_RECV,
        Bind::VmFallible(|vm, a| {
            let c = chan(&a[0]);
            if let Some(e) = cancelled(&vm.it) {
                return Ok(Err(e));
            }
            crate::simfault::sweep(&mut vm.it);
            if let Some(v) = vm.it.chans[c].q.pop_front() {
                vm.it.progress += 1;
                return Ok(Ok(v));
            }
            Ok(Err(failure(if vm.it.chans[c].senders == 0 {
                "Closed"
            } else {
                "Empty"
            })))
        }),
    ),
    (
        STD_CHAN_RECEIVER_SHARE,
        Bind::Plain(|_| unsupported("`std.chan.receiver.share` in the interpreter")),
    ),
    (
        STD_CHAN_SENDER_SEND,
        Bind::VmFallible(|vm, a| {
            let c = chan(&a[0]);
            loop {
                let it = &mut vm.it;
                if let Some(e) = cancelled(it) {
                    return Ok(Err(e));
                }
                let ch = &mut it.chans[c];
                // A rendezvous channel hands over only to a receiver already waiting.
                let room = if ch.cap == 0 {
                    let waiting = it.waiting_recv.iter().filter(|w| **w == c).count();
                    ch.q.len() < waiting
                } else {
                    ch.q.len() < ch.cap
                };
                if room {
                    ch.q.push_back(std::mem::take(&mut a[1]));
                    it.progress += 1;
                    return Ok(Ok(Value::Unit));
                }
                if !idle_ran(vm)? {
                    return blocked(&mut vm.it, "send", c);
                }
            }
        }),
    ),
    (
        STD_CHAN_SENDER_TRY_SEND,
        Bind::VmFallible(|vm, a| {
            let c = chan(&a[0]);
            let it = &mut vm.it;
            if let Some(e) = cancelled(it) {
                return Ok(Err(e));
            }
            let ch = &mut it.chans[c];
            let waiting = it.waiting_recv.iter().filter(|w| **w == c).count();
            let room = if ch.cap == 0 {
                ch.q.len() < waiting
            } else {
                ch.q.len() < ch.cap
            };
            if !room {
                return Ok(Err(failure("Full")));
            }
            ch.q.push_back(std::mem::take(&mut a[1]));
            it.progress += 1;
            Ok(Ok(Value::Unit))
        }),
    ),
    (
        STD_CHAN_SENDER_SHARE,
        Bind::Plain(|_| unsupported("`std.chan.sender.share` in the interpreter")),
    ),
    (
        STD_RC_SHARE,
        Bind::Plain(|_| unsupported("`std.rc.share` in the interpreter")),
    ),
    (
        STD_RC_STRONG_COUNT,
        Bind::Plain(|_| unsupported("`std.rc.strong_count` in the interpreter")),
    ),
    (
        STD_SYNC_ARC_SHARE,
        Bind::Plain(|_| unsupported("`std.sync.arc.share` in the interpreter")),
    ),
    (
        STD_SYNC_ARC_STRONG_COUNT,
        Bind::Plain(|_| unsupported("`std.sync.arc.strong_count` in the interpreter")),
    ),
    (
        STD_SYNC_ATOMIC_LOAD,
        Bind::Plain(|_| unsupported("`std.sync.atomic.load` in the interpreter")),
    ),
    (
        STD_SYNC_ATOMIC_STORE,
        Bind::Plain(|_| unsupported("`std.sync.atomic.store` in the interpreter")),
    ),
    (
        STD_SYNC_CELL_GET,
        Bind::Plain(|_| unsupported("`std.sync.cell.get` in the interpreter")),
    ),
    (
        STD_SYNC_CELL_SET,
        Bind::Plain(|_| unsupported("`std.sync.cell.set` in the interpreter")),
    ),
    (
        STD_LATTICE_GET,
        Bind::Plain(|_| unsupported("`std.lattice.get` in the interpreter")),
    ),
    (
        STD_LATTICE_MAX_VALUE,
        Bind::Plain(|_| unsupported("`std.lattice.max.value` in the interpreter")),
    ),
    (
        STD_LATTICE_MAX_OF,
        Bind::Plain(|_| unsupported("`std.lattice.max_of` in the interpreter")),
    ),
    (
        STD_LATTICE_PUT,
        Bind::Plain(|_| unsupported("`std.lattice.put` in the interpreter")),
    ),
    (
        STD_EVENTED_SCHED,
        Bind::Plain(|_| unsupported("`std.evented.sched` in the interpreter")),
    ),
];
