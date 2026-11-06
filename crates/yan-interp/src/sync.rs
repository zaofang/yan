//! std.rc, std.sync, std.chan, std.actor and std.lattice: checked by the elaborator and the Yan Core checker.
//! The interpreter runs channels on one thread and stops with "not supported" at the rest.

use std::collections::VecDeque;

use yan_std::registry::{Id, id::*};

use super::bind::{Bind, bad_args};
use super::time::wait_out;
use super::{Interp, R, Value, unsupported};

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
            _ => unreachable!("a channel end"),
        },
        _ => unreachable!("a channel end"),
    }
}

/// A branch of a `par concurrent` at task level parks and the VM switches to another task (R390).
/// Elsewhere nothing can end the wait: under a deadline it fails with `Timeout` at the deadline
/// (9.4), and otherwise the run stops.
fn blocked(it: &mut Interp, op: &str, chan: usize) -> R<Result<Value, Value>> {
    if it.park_ok && !it.force {
        it.park = Some((op == "recv", chan));
        return Ok(Ok(Value::Unit));
    }
    match wait_out(it) {
        Some(t) => Ok(Err(t)),
        None => unsupported(format!(
            "a `{op}` that waits on another task, in the interpreter"
        )),
    }
}

pub(super) static BINDS: &[(Id, Bind)] = &[
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
