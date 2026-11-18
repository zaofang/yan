# 0089: `Atomic.wait` and `notify`, and the check for UB item 15

- Status: accepted
- Research: code reading for #1664 (no study; the facts are cited below)
- Rows: numbered with the cases of the first slice (CASE WITH ROW), as the final commit after merging main

## Summary

Spec 6.7 item 15 is "`Atomic.wait` on non-atomic or misaligned storage" and 6.2 lists `wait`,
`notify_one` and `notify_all` on 32-bit atomics. None of it exists. `crates/yan-std/lib/alloc/sync/atomic.yag`
has no `wait`, `notify_one`, `notify_all` or `fetch_update`; `AtomicOp` in `yan-mir` has no wait or
notify; `yan-machine` has no parking; the interpreter's task scheduler (`yan-interp/src/sync.rs`)
parks only for channels, actors and handler calls; no backend lowers a futex. So no program can
reach item 15, and #1664 (hole 6 of #1242) cannot be closed by a checker alone: a case needs a
`wait` to flag. This RFC defines the operation with its scheduler semantics, then the check, so the
hole closes with the feature instead of a check for dead surface. Stage claimed: working, then
right.

## Decisions

### D1: where `wait` can go wrong, and what "non-atomic" means

`Atomic[T]` is a typed value, so a safe program cannot `wait` on non-atomic or misaligned storage:
the type fixes size and alignment, and E418 fixes lock-freedom. Item 15 is therefore reachable only
through a raw form: `Ptr[Atomic[U32]]`-style access in unsafe code (`Ptr.cast` of arbitrary bytes),
foreign code, or a wrong `unsafe impl`. The check is on the address operand of `wait`: UB item 15
if it is not 4-aligned, not inside a live allocation, or not a word the program ever accessed with
an atomic instruction (the machine already shadows atomic words, `race.rs`). Recommended.

- Decision: accepted as recommended (Antonio, 2026-11-18)
- Row: numbered in the first slice

### D2: surface, 32-bit only, deterministic by default

`Atomic[U32]` and `Atomic[I32]` (and `Bool` widened to its 32-bit cell is NOT included) gain
`wait(self, expected: U32, o: LoadOrdering) -> None`, `notify_one(self) -> None` and
`notify_all(self) -> None`, as 6.2 says. Whether the value still equals `expected` and who wakes
depend on other tasks, so `wait` and both notifies require `uses nondet` (E709), like `try_lock`
and `Arc.strong_count`; replay records the wake as a choice (7.6). `wait` is a cancellation point
under the enclosing deadline (9.4) and returns `fail Timeout` or `fail Cancelled` through its fail
type, so it is not infallible. Alternatives: a spurious-wake-free `wait_while` closure form
(rejected as the primitive; std can build it on `wait`), and no `wait` at 0.1.0 with 6.2 and item 15
removed (viable, listed as the deferral option; the spec text then needs the rows reverted).
Recommended: implement, because a futex is the base of every std lock beyond `Mutex` and 6.2 already
promises it.

- Decision: accepted as recommended (Antonio, 2026-11-18)
- Row: numbered in the first slice

### D3: parking in the machine, interpreter and native

`Inst::AtomicWait { ptr, expected, order }` and `Inst::AtomicNotify { ptr, count }` in `yan-mir`.
Interpreter and machine: `wait` reads the word with `order`; if it differs from `expected` it
returns at once, otherwise the task parks on the address in the existing wait list, counted by the
deadlock detector (R396); `notify` wakes up to `count` tasks parked on the address, in FIFO order
under the seeded handler. Native: Linux `futex`, macOS `os_sync_wait_on_address`, Windows
`WaitOnAddress` (as 6.2 names), through `yan-rt`. The interpreter's wake order is the executable
definition; native may wake any waiter, which is not UB (item 1 excludes races between atomics).
Recommended: one MIR instruction pair, no std-level emulation, so interpreter and native agree on
the item 15 check site.

- Decision: accepted as recommended (Antonio, 2026-11-18)
- Row: numbered in the first slice

### D4: slices

S1 std surface, MIR ops, interpreter parking, deadlock accounting and the `--ub` item 15 check, with
`conformance/ub/atomic_wait_misaligned.yn` and a clean case. S2 native lowering on all three
targets, with interpreter and native outputs attached. S3 `fetch_update` (the other 6.2 operation
that is missing; independent, listed so 6.2 and std agree). #1664 closes with S1; S2 gets its own
issue.

- Decision: accepted as recommended (Antonio, 2026-11-18)
- Row: numbered in the first slice

## Spec changes

6.2 (the `wait` line gains the signature, `uses nondet` and fail type), 6.7 item 15 (names the three
conditions), 6.9 (`--ub` reports item 15), 9.4 (`wait` is a cancellation point), 17 (std atomic
table).

## Amendments

- 2026-11-18, slice 1 (#1704). D3 names `Inst::AtomicWait` and `Inst::AtomicNotify`. Slice 1 lowers them as `RtFn::FutexWait` and `RtFn::FutexNotify` (`Inst::Rt`) instead, because that reuses the channel blocking path (suspend, retry, cancellation status, `par concurrent` scheduling) that a new instruction would have to duplicate. The check site is unchanged: the machine's handling of `FutexWait`. Backends and the interpreter agree on it as D3 intends. The surface types are `u32` and `i32` (spelled `uint32` and `int32` in Yan Agent).
- 2026-11-18, slice 1 (#1704). The `ub(15)` cases are deferred to #1705 (no surface can yet spell the program). The row for the item 15 check is numbered with those cases (CASE WITH ROW); until then the spec cites this RFC (D1). Native `wait` outside a cooperative task, `Timeout` under a deadline outside the interpreter, and `fetch_update` are in #1703.
