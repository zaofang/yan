# Design: `std.sync.atomic` and `Mutex`, through yir and the machine, with `--ub` ordering edges

Source: #1319 (last item: mutex and atomic edges), tracked in #1369, read against main 4adc73e on 2026-11-16. Design only; nothing here is implemented. No rows or codes are assigned (spec freeze, #1025). Items are labelled D1 to D9. Spec questions are listed under Decisions and go to the lead as cards; nothing here edits `spec/`.

## State on main

| Layer | Atomic | Mutex |
|---|---|---|
| Spec | 6.2 (C++20/RC11, explicit orderings, full op set, lock-free only, E418), 6.4 (Send/Sync), 6.5 (HB list, non-Freeze sharing needs `uses nondet`), 6.7 items 1 and 15 | 6.3 (no poisoning, infallible `lock()`, no `Condvar`), R395 (guard across a blocking op is E323), 6.5 (unlock before next lock) |
| std | `crates/yan-std/lib/alloc/sync/atomic.yag`: `load(self)`, `store(self, v)`; no ordering, no constructor, no RMW | none |
| Elaborator | `Atomic` is a known collection type (`yan-elab/src/ty.rs`), E418 width check at parameters (`lower.rs`), not `Wire` | none |
| Interpreter | `load`/`store` stop with "unsupported" (`yan-interp/src/sync.rs`) | none |
| MIR, Cranelift | no shape, no instruction | none |
| Machine | none; `race.rs` has fork/join, task switch, channel and actor clocks | none |

So the surface comes first: there is nothing for `--ub` to attach an edge to.

## D1. Surface

Follow 6.2 exactly; the current `load`/`store` signatures contradict "ordering is always explicit" and are replaced, not kept beside the new ones.

- Construction: `Atomic[T](v)`, `Mutex[T](v)`, the same shape as `Channel[T](cap)`. Both hold a `RawCell` per 6.3, so they are not trivially copyable and always pass by address under `let`; sharing between tasks is a `let` borrow into `par` branches or an `Arc`.
- Orderings (Decision Q1): three enums so an invalid ordering cannot be written: `LoadOrdering` (`Relaxed`, `Acquire`, `SeqCst`), `StoreOrdering` (`Relaxed`, `Release`, `SeqCst`), `Ordering` (all five) for read-modify-writes and fences. `compare_exchange(current, new, success: Ordering, failure: LoadOrdering)`; a failure ordering stronger than success is allowed (C++17, P0418). Orderings are ordinary values; a non-constant ordering lowers to a switch (rare, and what C11 permits).
- Operations: the 6.2 list, one std function each: `load`, `store`, `swap`, `compare_exchange`, `compare_exchange_weak` (returning `Result[T, T]`), `fetch_add`, `fetch_sub`, `fetch_and`, `fetch_or`, `fetch_xor`, `fetch_max`, `fetch_min`, `fetch_update`; `get_mut` under `inout` (plain exclusive access, 6.3); `into_inner`. Integer RMWs wrap. `fence(o: Ordering)` and `compiler_fence(o)` in `std.sync.atomic`.
- `wait(expected, o)`, `notify_one`, `notify_all` only on `Atomic[u32]`/`Atomic[i32]`, as 6.2 says (slice S5).
- `Mutex`: `m.lock()` returns a guard (Decision Q2). The guard is a second-class value under the existing view rules: it cannot be stored, returned or captured, so it cannot outlive its scope or cross a task, and it gives `MutView[T]` access to the protected value. It unlocks at its drop point. E323 (R395) already covers a guard live across a blocking op. `try_lock` needs `uses nondet` (its result depends on other tasks) and is added to the elaborator's nondet table. `get_mut` under `inout`, `into_inner`.
- Yan Agent: `with m.lock() as g:` is the guard scope; atomics are method calls as in Yan.

Checker items (existing rules, new types): `Mutex[T]` is Send and Sync iff `T: Send`; the guard is neither; `Atomic` is both (6.4). Neither is `Wire`. Sharing a non-Freeze value between tasks that may run at once needs `uses nondet` (6.5, E709); verify this rule fires for `Atomic` and `Mutex` and file it under #1242 if it does not.

## D2. Interpreter semantics

`yan-interp` stays the reference for safe programs and must give the same output as native (CLAUDE.md, practice 1). An `Atomic` is a shared mutable slot like `Cell`; orderings have no observable effect in a single-threaded value interpreter, and every interleaving it runs is sequentially consistent, which is one of the executions the model allows. A `Mutex` is a slot plus a held flag.

Contention cannot happen under a cooperative scheduler: a task switches only at a suspension point, and R395 forbids a guard live across one. So a `lock()` that finds the mutex held is a task locking a mutex it already holds: a deadlock. 6.7 says deadlock is not UB and R396 says a global deadlock panics; the interpreter panics with the deadlock message instead of hanging. The machine and the cooperative native scheduler follow the same rule.

## D3. Layout and yir/MIR lowering

New lowering goes in a new module, `yan-mir/src/lower/sync.rs` (practice 5), called from `lower/call.rs` the way channels are.

- Shape: `Atomic[T]` is `T`'s bytes with alignment equal to size (1, 2, 4, 8 or 16); a padding-free record of at most 16 bytes is one atomic unit. E418 already rejects the rest. `Mutex[T]` is `{ state: u32, value: T }`.
- Instructions: `Inst::Atomic { op, ty, order, failure, addr, args, dst }` with `op` in Load, Store, Swap, Cas, CasWeak, Rmw(Add | Sub | And | Or | Xor | Max | Min, signedness); `Inst::Fence { order, compiler_only }`. Not `RtFn` calls: an atomic is a memory access, and the machine's shadow state and Cranelift both need it in the instruction stream next to `Load`/`Store`. `fetch_update` lowers to a CAS loop in MIR, so no backend sees it.
- Mutex (interim, D8): `RtFn::MutexLock(ptr)` and `RtFn::MutexUnlock(ptr)` into a new `yan-rt` module. Under OS threads (`pool`): CAS fast path, then spin briefly and park on the futex of `state` (bounded because critical sections never suspend). Under the cooperative scheduler: CAS fast path, held means deadlock, panic as in D2.
- Cranelift: `atomic_load`, `atomic_store`, `atomic_rmw`, `atomic_cas` and `fence` are sequentially consistent, which is stronger than every ordering and so correct. Cranelift has no 16-byte atomics: a 16-byte `Atomic` on the Cranelift backend gets the existing unsupported diagnostic (fail loud, law 13) until LLVM, which emits the ordering as written (`cmpxchg16b` / `casp`). Max and min with no hardware instruction lower to a CAS loop.

## D4. Machine execution (checks off)

`yan-machine` executes `Inst::Atomic` and `Inst::Fence` over abstract bytes: an atomic access is a typed access of `size` bytes at `addr` with alignment checked (item 4) and provenance checked (item 2), done in one step so no other task interleaves. A CAS compares the bytes of the value representation; padding is excluded because only padding-free types are admitted. Loads return the latest store in modification order (see D7). `MutexLock`/`MutexUnlock` run the D3 cooperative runtime.

## D5. `--ub` edges for atomics (6.7 item 1)

All in `yan-machine/src/race.rs`, beside the channel `Link` state, following the C++20 model as Miri's data-race detector does (vector clocks, FastTrack-style epochs).

1. **Atomic vs plain shadow.** The per-byte `Cell` gains an atomic record: the last atomic write epoch, the atomic read epochs, and the `(base, size)` of the atomic access. Races between atomics are not UB; a plain access unordered with an atomic access to the same byte is (atomic writes count as writes). An atomic access unordered with an earlier atomic access that overlaps it with a different `(base, size)` is a mixed-size race, item 1.
2. **Release/acquire.** Each atomic location keeps a sync clock. A Release, AcqRel or SeqCst store sets it to the current thread's clock. An Acquire, AcqRel or SeqCst load joins it into the current thread. A read-modify-write keeps the release sequence: its new sync clock is the old one joined with the current clock when it releases (C++20 limits release sequences to RMWs, so a relaxed plain store ends the sequence and resets the clock to the storer's release-fence clock, item 3).
3. **Fences.** Per thread, a relaxed load accumulates the location's sync clock into an acquire-fence clock that `fence(Acquire)` joins; `fence(Release)` saves the thread's clock as the release-fence clock that later relaxed stores and RMWs publish.
4. **SeqCst.** SeqCst operations and fences additionally join and publish one global SC clock. This gives the SC-fence and SC-access edges RC11 requires for the executions the machine runs; it is the Miri approximation and is exact under D7's coherent loads.
5. **Reports.** The existing `who` text, naming the two accesses, their tasks, and "atomic" or "plain".

## D6. `--ub` edges for Mutex (interim)

A per-mutex clock in `race.rs` keyed by the mutex address: `MutexUnlock` joins the current clock into it and ticks; `MutexLock` joins it into the current thread. The `state` word is runtime-owned and is not shadowed. This is the 6.5 edge "unlock before the next lock" and has the same shape as the channel `Link`. D8 deletes it.

## D7. Weak memory (later, own issue)

6.9 asks for store-buffer weak memory. D4's loads always return the newest store, a subset of the allowed executions: race reports stay sound for the runs taken, but outcomes only weak memory allows (store buffering, load buffering through relaxed loads) are never produced. The full form keeps a bounded store buffer per location and lets a load pick any store that coherence, the thread's view and its happens-before allow, chosen by the run's seed so `yan replay` reproduces it. This lands after S4 and does not change any clock in D5.

## D8. Long-term Mutex: std over `RawCell` and `Atomic[u32]`

6.3 says std builds `Mutex` on `RawCell`. Once `RawCell` (L6) and std-in-Yan unsafe code exist, `Mutex` becomes std source: lock is an Acquire CAS on `state` plus `wait`, unlock is a Release store plus `notify_one`. The machine then sees only atomics, and D5 supplies the mutex edge with no mutex-specific state; `explore` (6.9) sees the same operations. That removes `RtFn::MutexLock`/`MutexUnlock`, the D3 runtime module and D6's clock: one representation of lock ordering (practice 12). The interim exists because neither `RawCell` nor any non-intrinsic std body exists today (all 576 std items are `@intrinsic`). `RwLock`, `Once` and `Lazy` follow the same path and are not in this plan.

## D9. Tests

All cases are proposed via `conform/*`; agents do not edit `conformance/` on `claude/` branches.

| Case | Expect | Shows |
|---|---|---|
| `ub/atomic_release_acquire_orders` | clean | plain write, `store(true, Release)`; other task `load(Acquire)` sees true, reads the plain value |
| `ub/atomic_relaxed_race` | ub(1) | same with `Relaxed`: the flag is seen but the plain read is unordered |
| `ub/atomic_rmw_release_sequence` | clean | release store, relaxed `fetch_add` in a third task, acquire load still synchronizes |
| `ub/atomic_fence_orders` | clean | relaxed store after `fence(Release)`, relaxed load before `fence(Acquire)` |
| `ub/mixed_atomic_plain_race` | ub(1) | atomic store in one task, plain `Ptr` read in another, unordered |
| `ub/mixed_size_atomic_race` | ub(1) | `Atomic[u32]` and `Atomic[u8]` views of the same bytes, unordered |
| `ub/mutex_orders_tasks` | clean | write under the lock in one task, read under the lock in the other |
| `ub/mutex_unordered_race` | ub(1) | one side writes outside the lock |
| `yn/pass` run: atomic counter | output | `par for` with `fetch_add(1, Relaxed)` and `uses nondet`; same output on interpreter and native |
| `yn/pass` run: mutex counter | output | the same with `Mutex[int]` |
| `yn/pass` run: self-deadlock | panic | second `lock()` in one task panics with the deadlock message |
| `yag/fail`: guard escape | existing view code | returning a guard |
| `yn/fail`: `try_lock` without `uses nondet` | E709 | |

Tasks in the `ub/` cases use `par concurrent` under `sequential()`, as `ub/unbuffered_unordered_race` does, so the order is source order and each case has one outcome. Unit tests in `race.rs` cover each D5 rule directly.

## Status (2026-11-16)

S1 is split: #1374 qualified variants (merged, #1381), #1375 S1a atomics surface and interpreter, #1376 S1b guard and scope-exit infrastructure, #1377 Mutex surface and interpreter. Orderings are written qualified (`LoadOrdering.Acquire()`, R799). S1a omits `get_mut`, `fetch_update` and `Atomic` Send/Sync checks; the interpreter wraps integers as `i64` whatever the element type (fixed with S2).

S2 and S3 land together for integers and `bool` (#1369). Shape: `Atomic[T]` is one heap word holding `T` at its start, since MIR slots are words; `Inst::Atomic` carries `op`, `ty`, `order`, `failure`; Cranelift runs every ordering SeqCst, a narrow `Add` or `Sub` through a compare-exchange loop that wraps; the machine executes it as one step on the word. Open, in order: `Ptr` and record elements and the 16-byte case (refused: `an Atomic of a type that is not an integer or bool`); `Atomic[bool]` `fetch_max`/`fetch_min` (refused); `get_mut`, `fetch_update`, Send/Sync checks.

S4 (2026-11-16): `race.rs` shadows atomics per D5 items 1 to 4 (atomic and plain records per byte, per-word release clock, release sequences through read-modify-writes, fences, one SeqCst clock); `Inst::Fence` and a callable `fence` exist. Mixed-size races do not arise yet because every `Atomic` is one word. The `ub/atomic_*` cases of D9 wait on yn: it cannot spell `StoreOrdering.Release` (E017, type fields) and yag has no `Ptr`, so the machine tests in `crates/yan-machine/tests/atomic_race.rs` build MIR by hand.

S1b (#1376) landed in the interpreter: the guard is not a new mechanism. `lock()` returns an owned `MutexGuard[T]` local, and drop elaboration (R770) already drops owned locals at the end of their block and before every early exit, so `Op::DropGuard` unlocks there. E323 is a Core pass (`yan-elab/src/guard.rs`): a guard is live from its binding to its block's end, and a std wait, a call to a function that may reach one, or a `par` in that range is reported. A guard is second-class (E414 on passing, returning or rebinding it). Access is `get`, `set` and `replace`; `MutView[T]` access is not built (open: 4.5 defines views over contiguous parts, not over a whole value). Early unlock needs a smaller scope, a function, until a `with` form exists. Native refuses `Mutex` until S2.

## Slices

Each is one PR and its own issue under #1369, in order; S1 to S4 close #1319.

| Slice | Scope | Depends on |
|---|---|---|
| S1 | D1 std signatures, types and checks; D2 interpreter | Q1, Q2 |
| S2 | D3 shape, `lower/sync.rs`, instructions, Cranelift; mutex runtime | S1 |
| S3 | D4 machine execution | S2 |
| S4 | D5, D6; `ub/` cases via `conform/*` | S3 |
| S5 | `wait`/`notify` (park and wake in the scheduler, no edges, 6.5); item 15 on misaligned or non-atomic storage | S3 |
| S6 | D7 store buffers | S4 |
| S7 | D8 Mutex in std; delete the interim | `RawCell` (L6), Q3 |

Interim steps, stated separately: D6's mutex clock and D3's mutex runtime calls exist only until S7; D4's newest-store loads only until S6; the 16-byte Cranelift refusal only until LLVM.

## Decisions (cards to the lead)

No spec row changes are required for S1 to S6: every rule used is already in 6.2 to 6.7, R395 and R396. These are std public API and architecture choices, which are gated:

- **Q1. Ordering types.** Recommended: three enums (`LoadOrdering`, `StoreOrdering`, `Ordering`) so invalid orderings do not type-check, with no new diagnostic or panic. Alternative: one `Ordering` as Rust has, with invalid combinations a panic or a new code.
- **Q2. Mutex guard.** Recommended: `lock()` returns a second-class guard with `MutView[T]` access that unlocks at drop. Alternative: closure-only `m.with(f)`, which 6.3's and R395's "guard" wording would then need changed.
- **Q3. Long-term Mutex.** Recommended: D8, std over `RawCell` and `Atomic[u32]`, with the interim runtime deleted in S7. Alternative: keep `Mutex` a runtime intrinsic permanently, with its own edges in the machine and in `explore`.

When the freeze lifts, 17's std table should list the ordering types and the guard, and 6.9 should name `yan run --ub` beside `yan test --ub` (as noted in the 2026-11-14 design).
