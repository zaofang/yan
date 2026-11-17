# 0086: Native pool for plain `par`

- Status: draft
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

### D1: outline branches only under `pool`

Under `sequential`, `seeded` and `explore`, branches stay inline: they are what the machine and the
differential run compare against. Under `pool`, MIR outlines each branch of a plain `par` into a task
function that takes a captured-frame block, reusing the thunk shapes of `yan-rt/src/actor.rs`
(rt never calls closures; generated code runs the thunk and the rt returns a work status).
Recommendation: outline in a new module (practice 5), selected per call site by the bound `sched`
type, with the inline form kept as the fallback for every other handler.

- Decision: (lead's answer, date)

### D2: fork-join with ordered commit, no stealing in v1

`yan-rt` gets a pool of N workers (default one per core, `pool(threads = n)`, and a process override
for the matrix). Branch 0 runs on the forking thread, branches 1..n are queued, and the join waits
for all. Branch results commit in branch order at the join (sharing classes, 6.5), so a result never
depends on which worker ran what. The heartbeat promotion of R481 is a later optimisation, because
it changes cost, not results. Recommendation: ship the simple form first and keep R481 for the
"fast" stage.

- Decision: (lead's answer, date)

### D3: failure and cancellation follow source order

If several branches fail, the failure of the lowest branch index is the result, as under
`sequential`. A failure sets a cancel flag that later branches poll at the points where
`par concurrent` already checks it; branches that finished their work still drop their locals
before the join returns. Panics abort as they do today. Recommendation: lowest index wins, so the
matrix can compare exit status byte for byte.

- Decision: (lead's answer, date)

### D4: runtime state must be per worker or atomic

`yan-rt` has `thread_local!` state in `abi`, `actor`, `bytes`, `civil`, `fshandler`, `loghandler`,
`regex`, `sched` and `timeabi`, and a counting allocator in `heap.rs`. A worker thread starts with
fresh thread-local state seeded from the forking task (handlers bound by `with` are copied by
reference where they are `Sync`, otherwise the branch is refused at compile time). The allocator
counters become atomic. Recommendation: audit each site before the pool lands and list any site that
cannot be per worker as a numbered gap; a branch that reaches one of them is E-coded, not silently
serial (practice 13).

- Decision: (lead's answer, date)

### D5: only checked-safe branches go to the pool

A branch is eligible when the Send/Sync/Freeze checks already pass for its captures
(`yan-core/src/auto_traits.rs`) and it holds no `@linear` or custom-`Drop` root across the fork.
Everything else runs inline and reports a lint, so the speedup never trades soundness. This depends
on INNOV-2 (#1583): native-live and `--ub` sweeps must be clean before shared state is released to
threads.
Recommendation: land the pool behind that gate and keep it off for programs that fail it.

- Decision: (lead's answer, date)

### D6: the matrix is the acceptance test

CI runs the deterministic corpus at thread counts 1, 2, 4, 8 and 16 and compares stdout, stderr and
exit status byte for byte, with repeat runs and no allowlist (folds in #776). A divergence is a bug
in the pool, never an allowlist entry. Recommendation: build the matrix runner before the pool so it
first proves that thread count 1 equals today's sequential output.

- Decision: (lead's answer, date)

### D7: the interpreter is out of scope

`yan-interp` values are `Rc`-based and its VM is single-threaded, so a pool there needs a `Send`
value path. That is a separate RFC; until then `std.sched.pool` stays unsupported in the interpreter
and the matrix covers native only. Recommendation: do not weaken the interpreter's role as the
reference by running it on threads before its values are sound for it.

- Decision: (lead's answer, date)

## Spec changes

6.7 (scheduling semantics) and 9.1 (`pool` row): state outlining, branch-order commit and
lowest-index failure. Diagnostics: one code for a branch refused under `pool` (D4, D5). Rows and
codes are numbered in the writing PR after acceptance.
