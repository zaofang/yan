# 0086: Native pool for plain `par`

- Status: accepted
- Research: assessment 2026-11-17 (`reports/2026-11-17-yan-core-innovation-assessment.md`), issue #1584
- Rows: filled in when accepted

## Summary

Plain `par` runs sequentially in native code today: MIR lowers each branch inline with `Arm` events
(`yan-mir/src/lower/stmt.rs`), and only `par concurrent` has a scheduler (`yan-rt/src/sched.rs`).
Spec 6.7 and 9.1 say `pool` is fork-join over OS threads and that results never depend on thread
count. This RFC proposes how native code gets a real pool without changing any result, so the
thread-count matrix of #1584 has something to test. The interpreter needs a different design (its
values are `Rc`) and is a separate RFC.

## Decisions

### D1: plain `par` arms are outlined over a heap frame; the runtime decides

`sched` is a runtime value, so MIR cannot choose a form per call site. A pool-safe arm of a plain
`par` is outlined as `g$armK(frame) -> i64` (`yan-mir/src/pool.rs`, a new module, practice 5). The
frame is a heap block with the arms' code addresses, then one word per local of `g`, as in the
coroutine frames of RFC 0043 (`co.rs`). At the fork, `PoolBegin` answers at run time: 0 keeps the
inline arms and their `Arm` events (`sequential`, `seeded`, `explore`, the machine, the
interpreter, and any `par` inside a worker); 1 stores the locals live into the arms, `PoolRun`
runs arm `k` as `resume(frame, k)` on a worker of the `yan-rt` pool and waits for all of them, and
the locals the arms define that the join reads are loaded back. Sharing classes (6.5) make branch
writes disjoint and shared reads immutable, so no merge step is needed; two arms defining one local
the join reads keep the inline path. Branches of a plain `par` do not suspend, so no coroutine
variant of `g` is built.
Recommendation: outline the arms over a frame and keep the inline path as the reference.

- Decision: amended from "outline into functions" to D1' (task region), accepted as recommended (Antonio, 2026-11-17); D1 text amended again in the S1 PR (#1584) to this mechanism, because the `par concurrent` dispatcher is a state machine in one native frame and its locals are not shared memory (lead-owned RFC text, no change of intent)

### D2: fork-join with ordered commit, no stealing in v1

`yan-rt` gets a pool of N workers (default one per core, `pool(threads = n)`, and a process override
for the matrix). Branch 0 runs on the forking thread, branches 1..n are queued, and the join waits
for all. Branch results commit in branch order at the join (sharing classes, 6.5), so a result never
depends on which worker ran what. The heartbeat promotion of R481 is a later optimisation, because
it changes cost, not results. Recommendation: ship the simple form first and keep R481 for the
"fast" stage.

- Decision: accepted as recommended (Antonio, 2026-11-17)

### D3: failure and cancellation follow source order

If several branches fail, the failure of the lowest branch index is the result, as under
`sequential`. A failure sets a cancel flag that later branches poll at the points where
`par concurrent` already checks it; branches that finished their work still drop their locals
before the join returns. Panics abort as they do today. Recommendation: lowest index wins, so the
matrix can compare exit status byte for byte.

- Decision: accepted as recommended (Antonio, 2026-11-17)

### D4: runtime state must be per worker or atomic

`yan-rt` has `thread_local!` state in `abi`, `actor`, `bytes`, `civil`, `fshandler`, `loghandler`,
`regex`, `sched` and `timeabi`, and a counting allocator in `heap.rs`. A worker thread starts with
fresh thread-local state seeded from the forking task (handlers bound by `with` are copied by
reference where they are `Sync`, otherwise the branch is refused at compile time). The allocator
counters become atomic. Recommendation: audit each site before the pool lands and list any site that
cannot be per worker as a numbered gap; a branch that reaches one of them is E-coded, not silently
serial (practice 13).

- Decision: accepted as recommended (Antonio, 2026-11-17)

### D5: only checked-safe branches go to the pool

A branch is eligible when the Send/Sync/Freeze checks already pass for its captures
(`yan-core/src/auto_traits.rs`) and it holds no `@linear` or custom-`Drop` root across the fork.
Everything else runs inline and reports a lint, so the speedup never trades soundness. S1 checks
the MIR instead: an arm with a nested `par`, an exit other than reaching its end or a panic, an
`Rt` call not on `RtFn::pool_safe`, an indirect call, an atomic or an address-taken local, or a
callee that fails the same test, stays inline. The Send/Sync lint and the `?`-exit arms follow. This depends
on INNOV-2 (#1583): native-live and `--ub` sweeps must be clean before shared state is released to
threads.
Recommendation: land the pool behind that gate and keep it off for programs that fail it.

- Decision: accepted as recommended (Antonio, 2026-11-17)

### D6: the matrix is the acceptance test

CI runs the deterministic corpus at thread counts 1, 2, 4, 8 and 16 and compares stdout, stderr and
exit status byte for byte, with repeat runs and no allowlist (folds in #776). A divergence is a bug
in the pool, never an allowlist entry. Recommendation: build the matrix runner before the pool so it
first proves that thread count 1 equals today's sequential output.

- Decision: accepted as recommended (Antonio, 2026-11-17)

### D7: the interpreter is out of scope

`yan-interp` values are `Rc`-based and its VM is single-threaded, so a pool there needs a `Send`
value path. That is a separate RFC; until then `std.sched.pool` stays unsupported in the interpreter
and the matrix covers native only. Recommendation: do not weaken the interpreter's role as the
reference by running it on threads before its values are sound for it.

- Decision: accepted as recommended (Antonio, 2026-11-17)

## Spec changes

6.7 (scheduling semantics) and 9.1 (`pool` row): state outlining, branch-order commit and
lowest-index failure. Diagnostics: one code for a branch refused under `pool` (D4, D5). Rows and
codes are numbered in the writing PR after acceptance.
