//! The faults of a `sim` cluster (RFC 0063 D3, D5): drops, delays and partitions drawn per ordered
//! node pair from the seed, plus the listed cuts.

use super::{Interp, Value};
use crate::time::{mono_now, wait_until};

/// A call through `Remote` that crossed from `from` to `to`, until its reply arrives.
pub(crate) struct Pending {
    pub(crate) from: usize,
    pub(crate) to: usize,
    /// How the request failed; the handler never ran.
    pub(crate) failed: Option<Value>,
    /// The reply could not be encoded.
    pub(crate) mismatch: bool,
}

struct Cut {
    at: i128,
    heal: i128,
    side: Vec<i128>,
}

struct Faults {
    seed: i128,
    epoch: i128,
    nodes: usize,
    drop: f64,
    delay: i128,
    partition: i128,
    heal: i128,
    cuts: Vec<Cut>,
}

fn field(v: &Value, k: usize) -> &Value {
    match v {
        Value::Data { fields, .. } => &fields[k].1,
        _ => unreachable!("a record"),
    }
}

fn int(v: &Value) -> i128 {
    match v {
        Value::Int(n) | Value::Duration(n) => *n,
        _ => unreachable!("an int or a duration"),
    }
}

impl Faults {
    fn of(sim: &Value) -> Faults {
        let f = field(sim, 1);
        let Value::Float(drop) = field(f, 0) else {
            unreachable!("a float")
        };
        let Value::List(cuts) = field(f, 4) else {
            unreachable!("a list")
        };
        let cuts = cuts
            .iter()
            .map(|c| Cut {
                at: int(field(c, 0)),
                heal: int(field(c, 1)),
                side: match field(c, 2) {
                    Value::Set(s) => s.iter().map(|k| int(&k.0)).collect(),
                    _ => unreachable!("a set"),
                },
            })
            .collect();
        let Value::List(ns) = field(sim, 0) else {
            unreachable!("the nodes")
        };
        Faults {
            seed: int(field(sim, 2)),
            epoch: int(field(sim, 3)),
            nodes: ns.len(),
            drop: *drop,
            delay: int(field(f, 1)),
            partition: int(field(f, 2)),
            heal: int(field(f, 3)),
            cuts,
        }
    }

    /// A uniform draw in [0, 1) from the stream named by `tag` and the indices `ix`.
    fn draw(&self, tag: &[u8], ix: [u64; 3]) -> f64 {
        let mut h = blake3::Hasher::new();
        h.update(b"yan.sim.fault");
        h.update(tag);
        h.update(&self.seed.to_le_bytes());
        ix.iter().for_each(|i| {
            h.update(&i.to_le_bytes());
        });
        let b: [u8; 8] = h.finalize().as_bytes()[..8].try_into().expect("8 bytes");
        (u64::from_le_bytes(b) >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Whether a seeded or listed partition separates `a` and `b` at `t` since the binding.
    fn cut(&self, a: usize, b: usize, t: i128) -> bool {
        let apart = |side: &dyn Fn(usize) -> bool| side(a) != side(b);
        if self
            .cuts
            .iter()
            .any(|c| c.at <= t && t < c.at + c.heal && apart(&|n| c.side.contains(&(n as i128))))
        {
            return true;
        }
        if self.partition <= 0 || self.nodes < 2 {
            return false;
        }
        let mut end = 0i128;
        for k in 0u64.. {
            let gap = (self.draw(b"gap", [k, 0, 0]) * 2.0 * self.partition as f64) as i128;
            let (start, stop) = (end + gap, end + gap + self.heal);
            if start > t {
                return false;
            }
            if t < stop {
                let mut side: Vec<bool> = (0..self.nodes)
                    .map(|n| self.draw(b"side", [k, n as u64, 0]) < 0.5)
                    .collect();
                // Both sides are non-empty.
                if side.iter().all(|s| *s) || side.iter().all(|s| !*s) {
                    side[(k as usize) % self.nodes] ^= true;
                }
                return side[a] != side[b];
            }
            end = stop;
        }
        unreachable!("the schedule passes `t`")
    }
}

/// Message `seq` from `from` to `to`: its delay in nanoseconds and whether it is lost.
fn message(f: &Faults, from: usize, to: usize, seq: u64, t: i128) -> (i128, bool) {
    let ix = [from as u64, to as u64, seq];
    let delay = (f.draw(b"delay", ix) * f.delay as f64) as i128;
    let lost = f.cut(from, to, t + delay) || f.draw(b"drop", ix) < f.drop;
    (delay, lost)
}

/// Sends one message over the pair; the failure a caller sees, if any (D5).
fn send(it: &mut Interp, from: usize, to: usize) -> Option<Value> {
    let sim = it.sim.clone()?;
    let f = Faults::of(&sim);
    let seq = it.seq.entry((from, to)).or_insert(0);
    let k = *seq;
    *seq += 1;
    let now = mono_now(it);
    let (delay, lost) = message(&f, from, to, k, now - f.epoch);
    if delay > 0 {
        let due = now + delay;
        if it.deadline.is_some_and(|d| due > d) {
            let d = it.deadline.expect("checked");
            return Some(wait_until(it, d));
        }
        if let Some(c) = it.clocks.last_mut() {
            *c += delay;
        } else {
            std::thread::sleep(std::time::Duration::from_nanos(delay as u64));
        }
    }
    lost.then(|| crate::sync::failure("Unreachable"))
}

/// A call through `Remote` leaves its caller's node for the actor's: the request message.
pub(crate) fn request(it: &mut Interp, path: &[usize], from: usize, to: usize) {
    let failed = if from == to { None } else { send(it, from, to) };
    it.remote.push((
        path.to_vec(),
        Pending {
            from,
            to,
            failed,
            mismatch: false,
        },
    ));
}

/// The request's failure when the call never reached the handler; it ends the call.
pub(crate) fn refused(it: &mut Interp, path: &[usize]) -> Option<Value> {
    let k = it.remote.iter().rposition(|(p, _)| p == path)?;
    it.remote[k].1.failed.as_ref()?;
    it.remote.remove(k).1.failed
}

/// The call ended without a reply to make; forget it.
pub(crate) fn forget(it: &mut Interp, path: &[usize]) {
    if let Some(k) = it.remote.iter().rposition(|(p, _)| p == path) {
        it.remote.remove(k);
    }
}

/// The reply message of the call `outcome` answers: its failure when the reply is lost or late.
pub(crate) fn reply(it: &mut Interp, path: &[usize], outcome: Value) -> Value {
    let Some(k) = it.remote.iter().rposition(|(p, _)| p == path) else {
        return outcome;
    };
    let p = it.remote.remove(k).1;
    let err = if p.mismatch {
        Some(crate::sync::failure("SchemaMismatch"))
    } else if p.from == p.to {
        None
    } else {
        send(it, p.to, p.from)
    };
    match err {
        Some(e) => Value::Data {
            name: "Err".into(),
            fields: vec![("error".into(), e)],
        },
        None => outcome,
    }
}

/// The reply of the innermost call of `path` through `Remote` cannot be encoded.
pub(crate) fn mismatch(it: &mut Interp, path: &[usize]) {
    if let Some(k) = it.remote.iter().rposition(|(p, _)| p == path) {
        it.remote[k].1.mismatch = true;
    }
}

/// The call's failure when an argument could not be encoded; it ends the call.
pub(crate) fn admit(it: &mut Interp, path: &[usize]) -> Option<Value> {
    let k = it.remote.iter().rposition(|(p, _)| p == path)?;
    if !it.remote[k].1.mismatch {
        return None;
    }
    it.remote.remove(k);
    Some(crate::sync::failure("SchemaMismatch"))
}
