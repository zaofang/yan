//! `std.sim` and `std.cluster` for native code (RFC 0063), a port of the interpreter's
//! `cluster.rs`: every node runs in this process, each with its own root supervisor, and the
//! node a task runs on is kept with the actors in [`crate::actor`].

use crate::abi::{List, str_block};
use crate::actor::{Sup, current, leave, panic_with, unsupported, with};
use crate::sched;

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
    if f64::from_bits(p[0]) != 0.0 || p[1] != 0 || p[2] != 0 || !cuts.data.is_empty() {
        unsupported("the faults of a `sim` cluster (#929 4c)");
    }
    with(|st| {
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
