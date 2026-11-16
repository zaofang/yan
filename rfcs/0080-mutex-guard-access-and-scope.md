# 0080: Mutex guard access, early unlock and escapes

- Status: draft
- Research: [atomics and Mutex design, 2026-11-16](../docs/research/2026-11-16-atomics-mutex-design.md) (Q2)
- Rows: filled in when accepted

## Summary

6.3 gives `Mutex[T]` a second-class `MutexGuard[T]` that unlocks at the end of its block and is read
and written through `get`, `set` and `replace`. Decision Q2 also asked for `MutView[T]` access, but 4.5
defines views over a contiguous part of a value, not over a whole `T`. Three holes remain (#1429):
what access through a guard means for a scalar or record `T` (item 2), how a critical section ends
before its block does (item 1), and which escapes of a guard are checked (item 4). This RFC settles
them with the machinery the spec already has: a projection (4.4, subscripts), the existing `with`
statement, and the view escape analysis. It adds no new type and no new runtime operation.

## Decisions

### D1: access through the guard is a place, not a view

`MutexGuard[T]` has one access path: the protected value as a place, `g.value`, defined in std as a
subscript (4.4) that yields the lock block's value word. It follows 4.4 unchanged: valid for one
statement, a read binds the guard as `let`, an assignment target or `&` argument binds it as `inout`,
and exclusivity (4.3) applies, so `g.value.f = 1` and `f(&g.value)` work for any `T`, and
`g.value[a..b]` is an ordinary slice, a `MutView` `from g` under 4.5 when `T` is a `List`, `Bytes` or
`Vec`. `get`, `set` and `replace` stay as the existing methods over the same place (`get` needs `Copy`,
`replace` is the move-out); nothing about them changes. 4.5 is not edited: `MutView[T]` keeps meaning a
borrowed contiguous part, and where `T` has parts, a guard yields one.

A whole-value `MutView[T]` is not needed because a place already passes to a callee by `inout`, which
is what a view over a whole value would be for. It is checked by the same exclusivity rule, so there is
one representation of "borrowed write access" (practice 12). The access is a plain access to the
lock block, so the machine's race detector checks it against the acquire and release edges of lock and
unlock: a protected access after the guard is dropped is a reported race, not an unchecked one.

Alternatives, not recommended:

- (b) A whole-value `MutView[T]` (`g.view()`). Extends 4.5 from "part of a value" to "any value",
  adds a type whose only job is what `inout` already does, and needs its own source tracking.
- (c) Keep `get`, `set` and `replace` only and drop `MutView` from Q2. Works for scalars, but a
  record or list `T` can be edited only by copying it out and back or by `replace`, which moves the
  whole value in and out of the lock.
- (d) Closure-only `m.with(f)` (Q2's alternative). Rejected by Q2, and it changes the guard wording of
  6.3 and 7.

- Decision: (lead's answer, date)
- Row: (R number once accepted)

### D2: early unlock is a `with` scope, not last-use drop

A guard bound by `with` lives to the end of the `with` block and no further. Yan: `with g = m.lock():`
over the existing `withstmt` production (15), which already binds a name for a block; Yan Agent:
`with m.lock() as g:` (6.3's design). A bare `g = m.lock()` keeps its meaning: held to the end of the
enclosing block. The `with` form is a rewrite to a nested block that starts with the binding, so drop
elaboration (5.3), the unlock on every path out (return, `fail`, `break`, `continue`) and the guard's
live range for E323 are the existing ones, applied to the smaller block. A `with` over a `Mutex`
guard is the only new case of the statement; handler and cancel-scope uses are unchanged.

Why this over last-use drop: the guard has a custom `Drop`, and 5.3 says early drop applies only to
types without one. Making an exception moves an observable event, the unlock, to a point the reader
cannot see, which changes where E323 reports, what another task may observe in `par` and what a replay
trace shows. A `with` block puts the critical section where the reader and `yan audit` can see it.

Alternatives, not recommended now:

- (b) Last-use drop for guards. Invisible critical sections, an exception to 5.3 and a liveness
  analysis the guard checks would also need.
- (c) A consuming `g.unlock()`. Flexible on conditional paths, but a guard passed `sink` is a
  second-class value passed on (4.5), so it needs a carve-out, and it needs drop flags where the unlock
  is conditional. It composes with this RFC and can be added later by its own RFC if a program needs
  it; nothing here prevents it.

- Decision: (lead's answer, date)
- Row: (R number once accepted)

### D3: a guard is second-class by the view rule, so escapes need no new check

A `MutexGuard[T]` is treated as a view for escape analysis: second-class, with its source the `Mutex`
place it locks. The one analysis of 4.5 then covers the three escapes #1429 item 4 lists, by the
sentences already there:

- stored in a record or container: E414 (4.5, "a type that contains a view is second-class");
- captured by an escaping closure: E406 (4.6 names views; it names guards too);
- sent on a channel or passed to an actor: not `Send` (6.4), so E415; borrowed by two `par` branches:
  not `Sync`, E417.

A non-escaping closure may capture a guard, as it may a view, so a helper closure can touch the
protected value. The guard stays live for the whole block, so E323 (6.3) already applies at every
call in range; a closure body counts at the point it is called. Calling a closure through a function
value is the existing gap of item 6, not widened here.

Alternative: a separate guard-escape pass with its own diagnostics. Two representations of "does not
outlive its source" (practice 12), rejected.

- Decision: (lead's answer, date)
- Row: (R number once accepted)

## Spec changes

6.3 (access through `value`, the `with` form), 4.5 (a guard is a source-carrying second-class value),
4.6 (the escaping-closure list), 15 and 13 (`with` over a guard, one sentence each; no new production in
Yan), 5.3 (the `with` block is a scope for drop elaboration).

## Conformance cases

`yag/run` and `yn/run`: record and list `T` edited through `g.value`, a `with` block releasing before a
later `lock` of the same mutex (no `deadlock` panic), unlock on `return`/`fail` inside `with`.
`yag/fail` and `yn/fail`: a guard in a record, a container and an escaping closure (E414, E406), a guard
sent on a channel (E415), a guard live across a wait in a `with` block (E323). `ub/`: a protected access
after the `with` block is a reported race.

## Implementation

D1: a `value` projection on `Shape::Guard(T)` lowering to the block's value word (`lower/mutex.rs`),
all backends. D2: parser sugar to a nested block in `yan-syntax`; no new MIR. D3: add `MutexGuard` to the
`lower/borrows.rs` source map and to the E406 capture check; no new pass. Closes items 1, 2 and 4 of
#1429.
