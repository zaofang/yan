//! `std.sim` and `std.cluster` for native code (RFC 0063), a port of the interpreter's
//! `cluster.rs`: every node runs in this process, each with its own root supervisor, and the
//! node a task runs on is kept with the actors in [`crate::actor`]. Of a fault plan, only drops
//! run here; delays, partitions and cuts wait for virtual time (#929 4c).

use crate::abi::{List, str_block};
use crate::actor::{Sup, current, home, leave, panic_with, unsupported, with};
use crate::sched;
use std::collections::HashMap;

/// Node `i`'s id: the first 128 bits of BLAKE3 over `"yan.sim.node" || seed || i` (D6), with
/// both as 128-bit integers, as the interpreter hashes them.
fn node_id(seed: i64, i: i64) -> String {
    let mut h = blake3::Hasher::new();
    h.update(b"yan.sim.node");
    h.update(&i128::from(seed).to_le_bytes());
    h.update(&i128::from(i).to_le_bytes());
    h.finalize().as_bytes()[..16]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The drops of the last `sim` built (RFC 0063 D5), drawn per ordered node pair from its seed,
/// and the calls and sends through `Remote` they fault, by task path.
#[derive(Default)]
pub(crate) struct Faults {
    seed: i128,
    drop: f64,
    /// The next message number of each ordered node pair.
    seq: HashMap<(usize, usize), u64>,
    /// Each call through `Remote` that crossed nodes, innermost last: its pair and whether its
    /// request was lost.
    calls: Vec<(Vec<usize>, (usize, usize), bool)>,
    /// The task whose next send was lost.
    sending: Option<Vec<usize>>,
}

impl Faults {
    /// A uniform draw in [0, 1) from the stream named by `tag` and the indices `ix`, as the
    /// interpreter draws it.
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

    /// Sends the next message from `from` to `to`: whether it is lost.
    fn lost(&mut self, from: usize, to: usize) -> bool {
        if from == to {
            return false;
        }
        let seq = self.seq.entry((from, to)).or_insert(0);
        let k = *seq;
        *seq += 1;
        self.draw(b"drop", [from as u64, to as u64, k]) < self.drop
    }

    /// Whether the innermost call of `me` lost its request; that ends the call.
    pub(crate) fn refused(&mut self, me: &[usize]) -> bool {
        match self.calls.iter().rposition(|(p, _, _)| p == me) {
            Some(k) if self.calls[k].2 => {
                self.calls.remove(k);
                true
            }
            _ => false,
        }
    }

    /// The innermost call of `me` ended without a reply.
    pub(crate) fn forget(&mut self, me: &[usize]) {
        if let Some(k) = self.calls.iter().rposition(|(p, _, _)| p == me) {
            self.calls.remove(k);
        }
    }

    /// Whether the send of `me` that `yan_rt_cluster_fault_send` faulted was lost.
    pub(crate) fn lost_send(&mut self, me: &[usize]) -> bool {
        self.sending.take().is_some_and(|p| p == me)
    }
}

/// The request message of a call through `Remote` to actor `id` (D5); `yan_rt_actor_take`
/// fails the call `Unreachable` when it is lost.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_cluster_reach(id: u64) {
    let me = sched::path();
    with(|st| {
        let pair = (current(st, &me), home(st, id as usize));
        let lost = st.faults.lost(pair.0, pair.1);
        st.faults.calls.push((me, pair, lost));
    });
}

/// The message of a send through `Remote` to actor `id`; `yan_rt_actor_send` fails it
/// `Unreachable` when it is lost.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_cluster_fault_send(id: u64) {
    let me = sched::path();
    with(|st| {
        let lost = st.faults.lost(current(st, &me), home(st, id as usize));
        st.faults.sending = lost.then_some(me);
    });
}

/// `admit`: a call whose argument the codec rejected (`mismatch` non-zero) ends here.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_cluster_admit(mismatch: u64) {
    if mismatch != 0 {
        let me = sched::path();
        with(|st| st.faults.forget(&me));
    }
}

/// `arrive`: the reply message of the innermost call of the calling task; 1 when it is lost.
/// A reply the codec rejected (`mismatch` non-zero) is never sent.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_cluster_arrive(mismatch: u64) -> u64 {
    let me = sched::path();
    with(|st| {
        let Some(k) = st.faults.calls.iter().rposition(|(p, _, _)| *p == me) else {
            return 0;
        };
        let (_, (from, to), _) = st.faults.calls.remove(k);
        u64::from(mismatch == 0 && st.faults.lost(to, from))
    })
}

/// `sim(nodes, seed, faults)`: the cluster's index. `plan` holds a `FaultPlan`'s slots: `drop`,
/// `delay`, `partition`, `heal`, `cuts`.
///
/// # Safety
/// `plan` holds those five slots, `cuts` a `List` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_sim_new(nodes: i64, seed: i64, plan: *const u64) -> u64 {
    if nodes < 1 {
        panic_with(
            "precondition `nodes >= 1` of `sim` failed",
            crate::abi::panic_message(""),
        );
    }
    // SAFETY: the caller's contract.
    let p = unsafe { std::slice::from_raw_parts(plan, 5) };
    // SAFETY: as above.
    let cuts = unsafe { &*(p[4] as *const List) };
    if p[1] != 0 || p[2] != 0 || !cuts.data.is_empty() {
        unsupported("the delays, partitions and cuts of a `sim` cluster (#929 4c)");
    }
    with(|st| {
        st.faults = Faults {
            seed: seed.into(),
            drop: f64::from_bits(p[0]),
            ..Faults::default()
        };
        st.sims.push((0..nodes).map(|i| node_id(seed, i)).collect());
        (st.sims.len() - 1) as u64
    })
}

/// `with cluster = sim`: a scope whose nodes 1 to n-1 each get a root supervisor (D1). Its
/// result is the lower of `low` and the open scope count before it, as `yan_rt_sup_begin`'s.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_sim_begin(low: u64, sim: u64) -> u64 {
    with(|st| {
        let depth = st.open.len() as u64;
        let n = st.sims[sim as usize].len();
        let first = st.sups.len();
        st.sups.extend((1..n).map(|_| Sup::root()));
        let mut scope = Sup::root();
        scope.nodes = Some((sim as usize, (first..first + n - 1).collect()));
        st.sups.push(scope);
        st.open.push(st.sups.len() - 1);
        low.min(depth)
    })
}

/// The node ids of the innermost bound cluster.
fn bound(st: &crate::actor::State) -> &[String] {
    st.open
        .iter()
        .rev()
        .find_map(|s| st.sups[*s].nodes.as_ref())
        .map_or(&[], |n| &st.sims[n.0])
}

/// `cluster.nodes()`: a `List[Node]` in view order.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_cluster_nodes() -> *mut List {
    let ids: Vec<u64> = with(|st| bound(st).iter().map(|n| str_block(n) as u64).collect());
    crate::abi::leak(List {
        words: 1,
        data: ids,
        limit: -1,
    })
}

/// `cluster.self_node()`: the node the calling task runs on (D1).
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_cluster_self_node() -> *mut String {
    let me = sched::path();
    with(|st| match bound(st).get(current(st, &me)) {
        Some(n) => str_block(n),
        None => unsupported("`cluster.self_node()` outside `with cluster`"),
    })
}

/// The calling task runs on node `node` until [`yan_rt_cluster_leave`]; a start there is
/// adopted by that node's root supervisor (D2).
///
/// # Safety
/// `node` is a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_cluster_place(node: *const String) {
    let me = sched::path();
    // SAFETY: the caller's contract.
    let node = unsafe { &*node };
    with(|st| {
        let Some(k) = bound(st).iter().position(|n| n == node) else {
            panic_with(
                "`On(node)` names a node outside this cluster",
                crate::abi::panic_message(""),
            )
        };
        if current(st, &me) != k {
            st.placed = Some(k);
        }
        st.nodes.push((me, k));
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_cluster_leave() {
    let me = sched::path();
    with(|st| {
        leave(st, &me);
        st.placed = None;
    });
}
