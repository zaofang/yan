//! std.rc, std.sync, std.chan, std.actor and std.lattice: checked by the elaborator and the Yan Core checker.
//! The interpreter runs channels and local actors on one thread and stops with "not supported"
//! at the rest.

use std::collections::VecDeque;
use std::rc::Rc;

use yan_std::registry::{Id, id::*};

use super::bind::{Bind, bad_args};
use super::supervise::{self, Child, Restart};
use super::time::wait_out;
use super::vm::{Clo, Vm};
use super::{Interp, R, Value, panic, unsupported};

fn end(name: &str, chan: usize) -> Value {
    Value::Data {
        name: name.into(),
        fields: vec![("chan".into(), Value::Int(chan as i128))],
    }
}

fn chan(v: &Value) -> usize {
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

/// In a cancelled task a blocking operation fails `Cancelled` (9.4).
fn cancelled(it: &Interp) -> Option<Value> {
    it.cancel.iter().any(|c| *c).then(|| Value::Data {
        name: "Cancelled".into(),
        fields: Vec::new(),
    })
}

/// A local actor (RFC 0056 D5): its state, `None` while a handler holds it, and its mailbox of
/// sent messages, each a closure that makes the handler call.
pub(crate) struct Actor {
    state: Option<Value>,
    capacity: usize,
    mail: VecDeque<Rc<Clo>>,
    /// A queued message is running, so the actor takes no other from its mailbox.
    running: bool,
    /// Stopped by a failed handler or its supervisor, and not restarted.
    stopped: bool,
    child: Child,
}

impl Actor {
    /// Its queue is dropped and its state with it (R813).
    pub(crate) fn stop(&mut self) {
        self.stopped = true;
        self.state = None;
        self.mail.clear();
    }

    pub(crate) fn child(&self) -> &Child {
        &self.child
    }

    /// A restart starts from `state` with an empty mailbox (R813, R958).
    pub(crate) fn restart_with(&mut self, state: Value) {
        self.stopped = false;
        self.state = Some(state);
    }
}

/// A new actor with `state`, a child of the supervisor bound here (R957).
pub(crate) fn spawn(
    it: &mut Interp,
    state: Value,
    cap: i128,
    restart: Restart,
    start: Option<Rc<Clo>>,
) -> R<Value> {
    if cap < 1 {
        return panic("precondition `capacity >= 1` of `spawn` failed");
    }
    let id = it.actors.len();
    let child = supervise::adopt(it, id, restart, start);
    it.actors.push(Actor {
        state: Some(state),
        capacity: cap as usize,
        mail: VecDeque::new(),
        running: false,
        stopped: false,
        child,
    });
    Ok(end("Actor", id))
}

/// A call or send to a stopped actor fails `Stopped` (9.6), which no handler fail type carries
/// yet.
fn stopped<T>() -> R<T> {
    unsupported(
        "a call or send to a stopped actor, before `ActorError` joins the handler fail type",
    )
}

/// Run actor `id`'s oldest queued message if the actor is idle; whether one ran.
fn run_queued(vm: &mut Vm, id: usize) -> R<bool> {
    let a = &mut vm.it.actors[id];
    if a.running || a.state.is_none() || a.stopped {
        return Ok(false);
    }
    let Some(m) = a.mail.pop_front() else {
        return Ok(false);
    };
    a.running = true;
    // The message runs under this std frame, so it cannot park; the caller still may.
    let park_ok = vm.it.park_ok;
    let r = vm.apply(&m, Vec::new());
    vm.it.park_ok = park_ok;
    vm.it.actors[id].running = false;
    vm.it.progress += 1;
    r.map(|_| true)
}

/// Another handler holds actor `id`: wait for it as for a full channel, keyed apart from
/// channels (R959).
fn wait_busy(it: &mut Interp, id: usize) -> R<Value> {
    if it.park_ok && !it.force {
        it.park = Some((false, usize::MAX - 1 - id));
        return Ok(Value::Unit);
    }
    if it.force || it.in_par == 0 {
        return panic("deadlock");
    }
    unsupported("a handler call that waits on a busy actor, in the interpreter")
}

pub(super) static BINDS: &[(Id, Bind)] = &[
    (
        STD_ACTOR_SPAWN,
        Bind::Cap(|it, a| match a {
            [state, Value::Int(cap)] => {
                spawn(it, std::mem::take(state), *cap, Restart::Temporary, None)
            }
            _ => bad_args(STD_ACTOR_SPAWN),
        }),
    ),
    (
        STD_ACTOR_TAKE,
        Bind::Vm(|vm, _, a| {
            let id = chan(&a[0]);
            if vm.it.actors[id].stopped {
                return stopped();
            }
            // A call is a barrier: the actor runs every message queued before it (R813).
            while run_queued(vm, id)? {}
            if vm.it.actors[id].stopped {
                return stopped();
            }
            match vm.it.actors[id].state.take() {
                Some(s) => Ok(s),
                None => wait_busy(&mut vm.it, id),
            }
        }),
    ),
    (
        STD_ACTOR_SEND,
        Bind::Vm(|vm, _, a| {
            let id = chan(&a[0]);
            if vm.it.actors[id].stopped {
                return stopped();
            }
            let Value::Closure(m) = std::mem::take(&mut a[1]) else {
                return bad_args(STD_ACTOR_SEND);
            };
            let full = |it: &Interp| it.actors[id].mail.len() >= it.actors[id].capacity;
            // A full mailbox parks a `block` sender, so the actor runs its oldest message first.
            if full(&vm.it) && !run_queued(vm, id)? {
                return wait_busy(&mut vm.it, id);
            }
            vm.it.actors[id].mail.push_back(m);
            vm.it.progress += 1;
            Ok(Value::Unit)
        }),
    ),
    (
        STD_ACTOR_PUT,
        Bind::Cap(|it, a| {
            it.actors[chan(&a[0])].state = Some(std::mem::take(&mut a[1]));
            it.progress += 1;
            Ok(Value::Unit)
        }),
    ),
    (
        STD_ACTOR_SETTLE,
        Bind::VmFallible(|vm, _, a| {
            let id = chan(&a[0]);
            vm.it.actors[id].state = Some(std::mem::take(&mut a[1]));
            vm.it.progress += 1;
            match std::mem::take(&mut a[2]) {
                Value::Data { name, mut fields } => {
                    let v = fields.pop().expect("a result payload").1;
                    if name == "Ok" {
                        return Ok(Ok(v));
                    }
                    // A failed handler may have left the state half updated, so it stops the
                    // actor; the caller still gets the failure.
                    supervise::crash(vm, id)?;
                    Ok(Err(v))
                }
                _ => bad_args(STD_ACTOR_SETTLE),
            }
        }),
    ),
    (
        STD_ACTOR_SHARE,
        Bind::Plain(|_| unsupported("`std.actor.share` in the interpreter")),
    ),
    (
        STD_CHAN_CHANNEL,
        Bind::Cap(|it, a| match a {
            [Value::Int(cap)] => {
                it.chans.push((*cap as usize, VecDeque::new()));
                let c = it.chans.len() - 1;
                Ok(Value::Tuple(vec![end("Sender", c), end("Receiver", c)]))
            }
            _ => bad_args(STD_CHAN_CHANNEL),
        }),
    ),
    (
        STD_CHAN_RECEIVER_RECV,
        Bind::CapFallible(|it, a| {
            if let Some(e) = cancelled(it) {
                return Ok(Err(e));
            }
            let c = chan(&a[0]);
            match it.chans[c].1.pop_front() {
                Some(v) => {
                    it.progress += 1;
                    Ok(Ok(v))
                }
                None => blocked(it, "recv", c),
            }
        }),
    ),
    (
        STD_CHAN_RECEIVER_SHARE,
        Bind::Plain(|_| unsupported("`std.chan.receiver.share` in the interpreter")),
    ),
    (
        STD_CHAN_SENDER_SEND,
        Bind::CapFallible(|it, a| {
            if let Some(e) = cancelled(it) {
                return Ok(Err(e));
            }
            let c = chan(&a[0]);
            let (cap, q) = &mut it.chans[c];
            // A rendezvous channel hands over only to a receiver already waiting.
            let room = if *cap == 0 {
                let waiting = it.waiting_recv.iter().filter(|w| **w == c).count();
                q.len() < waiting
            } else {
                q.len() < *cap
            };
            if room {
                q.push_back(std::mem::take(&mut a[1]));
                it.progress += 1;
                Ok(Ok(Value::Unit))
            } else {
                blocked(it, "send", c)
            }
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
