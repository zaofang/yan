# 0081: Mutex try_lock, owned access, auto traits and the wait check

- Status: draft
- Research: [atomics and Mutex design, 2026-11-16](../docs/research/2026-11-16-atomics-mutex-design.md)
- Rows: filled in when accepted

## Summary

RFC 0080 closed items 1 to 4 of #1429. Items 5 to 7 remain: `try_lock`, `get_mut` and `into_inner` are
not built; the 6.4 rule "`Mutex[T]` is Send and Sync iff `T: Send`" is not enforced (Send is a list of
type names in `yan-elab/src/lower.rs::not_send`, and no check of E417 exists); the E709 rule for
sharing a non-Freeze value is not applied to a `Mutex` borrowed into `par` branches; E323 recognizes a
wait only by the name of a user function, not through function values or trait methods; and the Yan
surface has cases only for `with` and `g.value`. Each decision below uses a mechanism the spec already
has.

## Decisions

### D1: `try_lock` returns an `Option` that may only be matched in place

`m.try_lock() -> Option[MutexGuard[T]] from self` and it needs `uses nondet` (E709): whether the lock is
free depends on other tasks (6.5). It returns `None` when another task holds the lock and also when
the calling task already holds it, where `lock()` panics with `deadlock` (R396); `try_lock` never
panics. Replay records the result as a nondeterministic choice (7.7). The type `Option[MutexGuard[T]]`
is second-class by 4.5 (it contains a view-like value). It may be the scrutinee of `match` or `if let`
(Yan Agent) and its Yan equivalents, where the `Some` binding is the guard, live to the end of that arm
and unlocked there on every path out. Naming the option (`r = m.try_lock()`), returning it, passing it
or storing it is E414. So the guard-never-live-across-a-wait rule (R395) and the unlock-at-block-end
rule (R1022) apply to the arm, and `None` holds nothing. A `with` over `try_lock` is not defined.

Alternatives, not recommended now:

- (b) Allow naming the option as a second-class local. The guard's live range then starts at the call
  even on the `None` path, so E323 over-reports, and the destructuring is a rebinding that R1024
  forbids today.
- (c) A typed failure `WouldBlock` instead of `Option`. Failure types are for faults; contention is an
  ordinary outcome.
- (d) No `try_lock` at 1.0. Without `Condvar` there is no retry loop that needs it, but 6.3 and #1429
  list it, and `std.sync.atomic` spin patterns need a non-blocking probe.

- Decision:
- Row:

### D2: `get_mut` is exclusive access, no lock and no nondet

`m.get_mut()` takes the `Mutex` by `inout` and yields the protected value as a place, the same std
subscript form as `g.value` (RFC 0080 D1): valid for one statement, bound `let` when read and `inout`
when assigned or passed `&m.get_mut()`, under exclusivity (4.3). No lock is taken and no guard exists:
`inout` of the `Mutex` proves no other task can reach it, which is what 6.2 already says ("`inout`
access is plain exclusive access (`get_mut`)"). It needs no capability. A `Mutex` reached through an
`Arc` is not `inout`, so `Arc.get_mut` (which needs `uses nondet`, 6.4) and then `get_mut` is the
path. Under `--ub` the access is a plain access to the lock block, ordered by the same release and
acquire edges as `g.value`, so a `get_mut` concurrent with a `lock` is a reported race, not an
unchecked one.

Alternatives: (b) a guard-returning `get_mut` (takes the lock needlessly and pays for it); (c) omit it
and require `lock()` everywhere (the 6.2 sentence would become false).

- Decision:
- Row:

### D3: `into_inner` consumes the mutex and takes no lock

`m.into_inner() -> T` takes the `Mutex` by `take`, moves out the value and frees the lock block. No
lock, no guard, no `nondet`; ownership alone proves no other task holds it. It is not poisoned, since
`Mutex` has no poisoning (6.2). A `Mutex` inside an `Arc` reaches it through `Arc.try_unwrap` or
`Arc.into_inner`, which already need `uses nondet`.

Alternative: none worth keeping; a locking `into_inner` would panic on a task that still holds a guard
and would not be a move.

- Decision:
- Row:

### D4: Send and Sync are computed once, structurally

6.4 states the rule: `Mutex[T]` is Send and Sync iff `T: Send`; a `MutexGuard` is neither (R1037);
`Rc`, `Weak`, `Cell`, `RawCell` and `Ptr` are not; auto traits follow the captures of a closure
(R436). Today `Send` is the name list in `not_send` and `Sync` has no check, so E417 cannot fire. The
recommendation is one function `auto_traits(ty) -> (Send, Sync)` over the checked type, consulted by
every E415 and E417 site (channel and actor payloads, `par` borrows, closure captures, `Arc`) and
replacing `not_send`. A user struct is Send or Sync iff all fields are. The primitive facts live in one
table in std metadata: the negative entries above, `Atomic` both, and `Mutex[T]` as
`Send iff T: Send`, `Sync iff T: Send`. When `Mutex` moves to std over `RawCell` and `Atomic[u32]` (S7
of the design study), the table row becomes `unsafe impl[T: Send] Send for Mutex[T]` and the same for
Sync, which 6.4 allows at L6 and `yan audit` lists, with no change to the check. That is one
representation of the fact (practice 12). Diagnostics are unchanged: E415 and E417 now fire for
`Mutex[Rc[int]]` (not Send, so not Sync) and for a `Mutex` of a non-Send value moved to a channel.

Alternatives: (b) extend the name list to `Mutex` and add a `not_sync` list (a second representation
that drifts, and no field-wise rule); (c) opt-in marker traits written in each std type now, before S7
(the language has no way to say a negative `impl`, so it needs a new construct).

- Decision:
- Row:

### D5: sharing a `Mutex` between tasks is the E709 site, `lock` is not

6.5 requires `uses nondet` to share a non-Freeze value between tasks that may run at once. A `Mutex`
holds a `RawCell`, so it is non-Freeze, and lock order is observable. The check sits where the sharing
happens: borrowing or moving a non-Freeze value into two `par` branches, a `par` item or a spawned task,
or `share` of an `Arc` holding one. It does not sit on `lock`, `get`, `set` or `replace`, which a task
may call on its own `Mutex` with no capability. `try_lock` needs `nondet` by D1 because its result,
not its sharing, is the observable. `Mutex[T]` with `T` itself Freeze is still non-Freeze, so there is
no exemption; there is no commutative-writes exemption either (compare `Lub`, R454), since lock order
is visible in what each critical section reads. The conform cases already pass `nondet` in the sharing
function; they gain a negative one for a `par` without it.

Alternatives: (b) require `nondet` on every `lock` (every protected access becomes nondet, including
single-task use, and `get_mut`'s absence of a capability would be inconsistent); (c) exempt `Mutex`
the way `Lub` is exempt (no argument that lock order commutes).

- Decision:
- Row:

### D6: E323 follows the effect signature, not names

A call may wait iff its callee may. Today E323 knows this for named user functions. For a call through
a function value or a trait method the callee is unknown, so the recommendation is that "may wait" is
a computed bit of the function type: closures and functions infer it (a fixpoint over the call graph
as now), a function-typed parameter or field carries it in its type (`fn(int) -> int` is no-wait,
`fn(int) -> int waits` may wait, spelled by one new type-level marker), a trait method carries it in
its signature, and an impl may not exceed its trait. The bit joins `uses` as part of the effect
signature (7), so it is one representation of what a function may do, shown by `yan audit` and by
reflection. Under a guard, a call whose bit is set is E323; an unmarked function-value parameter is
no-wait, so a callee that waits is a type error at the call that passes it (E201), not a silent hole.

Alternatives, not recommended now: (b) treat every indirect call as waiting (a safe and simple
interim, rejects the non-escaping helper closure RFC 0080 D3 allows, and is what to ship first if D6 is
deferred); (c) leave the gap (E323's name rule can be bypassed, so the R395 guarantee stays unproven
for indirect calls).

- Decision:
- Row:

## Spec changes

6.2 (`get_mut` on `Mutex`), 6.3 (`try_lock`, `get_mut`, `into_inner`; the `Option` form), 6.4 (the
auto-trait function and its table), 6.5 (the sharing site of a `Mutex`), 7 (the wait bit in the effect
signature, with D6), 17 (std listing), 25 (no new code; E414, E415, E417, E709, E323 gain cases).

## Conformance cases

`yag/run`: `try_lock` free and held by the same task (`None`, no panic) inside `match`; `get_mut` on an
owned `Mutex`; `into_inner` of a `Mutex[list[int]]` (value moved out, block freed). `yn/run`: the same
three in Yan (the `with`/`g.value` case exists), plus `try_lock` in a `match`. `yag/fail`:
`try_lock` without `nondet` (E709); `r = m.try_lock()` named, returned or stored (E414); `Mutex[Rc[int]]`
sent on a channel (E415) and borrowed by two `par` branches (E417); a `Mutex` shared into `par` without
`nondet` (E709); `get_mut` through a `let` or `Arc` (exclusivity); a guard live across a call through a
function value or trait method that waits (E323, D6). `ub/`: `get_mut` concurrent with `lock` is a
reported race.

## Implementation

D1: `try_lock` intrinsic in `lower/mutex.rs`, an `Option` guard shape matched in place, E414 on naming;
all backends and the machine. D2/D3: projection and move intrinsics beside `lock`, lowering to the
value word and a free of the block. D4: `auto_traits` in `yan-elab` replacing `not_send`, plus the
E417 check at `par` borrows. D5: the non-Freeze check at `par` borrow, task move and `Arc` share. D6:
the effect bit in `yan-elab` function types and the trait signature check. Closes items 5 to 7 of
#1429.
