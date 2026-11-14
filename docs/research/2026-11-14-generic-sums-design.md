# Design: generic sum types, `TryRecv[T]` and native `select`

Issue #1163 (parent #1153, group P1, tracker #1166). Read against main f7e3056, 2026-11-14. Design only; no code, rows or conformance expectations change here.

## Findings

1. **The spec already has generic sums.** Yan Agent `typedef` takes `[tparams]` (13, R423), Yan `typedef` does too (15), Yan Core has `Sum ::= "sum" Path [TParams]` and `Ctor ::= Path ["[" TArg ... "]"] "{" ... "}"` (3.5), 10.1 says user types take type parameters and are "type-checked once, generically, and monomorphized per instantiation", and R772 defines instances. The toolchain rejects them: E017 "generic types" in `yan-elab/src/lib.rs:934`, skipped variants in `yan-check/src/lib.rs:295`, and the `tparams.is_empty()` filters in `yan-mir/src/lower/shape.rs:268-270`. This is implementation debt against existing rows, so **no new spec row is required and the freeze (#1025) is not touched.**
2. **Native `select` does not need generic sums.** The desugar (`yan-elab/src/lower/stmt_form.rs`, `select_stmts`) uses `select_key() -> int`, `select_wait(keys, ats) -> int` and `recv() -> T?`. `Option[T]` already has a native shape. What native select lacks is a `yan-rt` `select_wait` and MIR lowering of `select_key`/`select_wait` (none exist today). The note on #1153 that native select waits on #1163 is wrong; it can proceed now (#1258).
3. **`try_recv`'s type is not fixed by the spec.** 9.5 says only that `try_recv` needs `uses nondet` (R342) and that "`try_send` returns `Full`". The failure shape `T fail Empty | Closed | Cancelled` lives in `crates/yan-std/lib/alloc/chan/receiver.yag` and in the conform case `chan_try_ops.yag`. Moving to `TryRecv[T]` is a std signature change plus a `conform/*` case update, not a row.
4. **`try_send` loses its payload on `Full`.** `try_send[T](var self, take v: T) -> None fail Full | Cancelled` consumes `v` and the failure carries nothing, so the caller cannot retry. Actor sends already solve this with `Undelivered[A, R]` returning `args` (R976). This is the one decision in this design that is Antonio's (card posted with the PR).
5. **Native sum shapes are already structural.** `Shape::Sum(name, variants)` holds concrete payload shapes and `layout.rs` lays out any shape, so per-instance layout needs only substitution before shape construction. Recursive types are E017 natively for all sums (`shape.rs` "recursive type"), generic or not.

## Decisions (recommended, most correct long-term)

- **D1 Syntax.** No new syntax. Yan Agent and Yan: `type TryRecv[T] = Got(value: T) | Empty() | Closed()`; Core: `sum TryRecv[T] { Got { value: T }, Empty, Closed }`. The sum-versus-alias rule of R423 is unchanged; a generic alias (`type Pairs[T] = list[tuple[T, T]]`) and a generic newtype stay E017 until their own task, because they need a separate alias-expansion step.
- **D2 Nominal generics, one path.** Sums and records share one mechanism: a record is a one-variant nominal type for substitution, checking and shapes. Generic records (#1259) reuse T1-T4 unchanged.
- **D3 One substitution.** `yan_core::subst` replaces type parameters in Core types, fields and variants, and `yan_core` defines the canonical instance text (`TryRecv[i64]`), which is also the shape name and the key. The private copies in `yan-interp/src/meta.rs` and `yan-opt/src/mono.rs` move onto it (law 12). Types are not turned into `inst` items: instances are a property of functions (R772), and a type instance is fully determined by its definition plus arguments, so every consumer substitutes on demand.
- **D4 Constructor typing.** Yan Core performs no inference, so elab fixes a constructor's type arguments: first from the expected type (bidirectional, as for `None` and empty literals), then by unifying field arguments with field types. A constructor still unfixed is E241 (14.1, "where inference would need an annotation"). Patterns take the arguments from the scrutinee's type; Core patterns carry no type arguments (3.5).
- **D5 Checking once, generically.** `yan-check` checks generic function bodies over generic sums with parameters abstract (10.1). Exhaustiveness is over variants and never depends on type arguments, so E503/E502 are unchanged and a match on `S[T]` is checked once.
- **D6 Ownership, drops and capabilities per instance.** Copy, Send, Freeze, `@linear`, drop need and `Wire` are computed for an instance from its substituted fields by the same function that serves non-generic sums. `TryRecv[int]` is copy; `TryRecv[Str]` is not and is dropped once on every path (R770); `TryRecv[L]` for a `@linear` `L` is linear (E412 applies). `@derive(Wire)` on a generic sum requires its parameters to be `Wire`. A type argument that a stored field may not hold (a capability, a view) is rejected with the existing field code at the instantiation; no new code.
- **D7 Layout and lowering.** Interpreter: values are already tagged by variant name, so nothing changes beyond elab and check. MIR/native: `shape_in` substitutes and builds `Shape::Sum("TryRecv[i64]", ..)`; drop glue, equality and show are per shape and therefore per instance; `layout.rs` is unchanged. Recursive generic sums stay E017 natively until boxed recursive shapes exist, recorded as a gap item, never a silent fallback (law 13).
- **D8 `TryRecv[T]`.** `pub type TryRecv[T] = Got(value: T) | Empty() | Closed()` in `std.chan`; `try_recv[T](var self: Receiver[T]) -> TryRecv[T] fail Cancelled`. `Empty` and `Closed` stop being failure classes, so `?` no longer silently propagates "nothing yet" as an error and a poll loop is a plain `match`. `Cancelled` stays a failure because cancellation is a failure everywhere else in 9.
- **D9 Native select is decoupled.** Implement `yan-rt` `select_wait` with the interpreter's semantics (`yan-interp/src/sync.rs`, `select_wait`: first ready arm in source order, park on the first receive and the earliest timer, cancellation and deadline) and lower `select_key`/`select_wait` in MIR. Land it before or alongside T1-T5.
- **D10 `Option` and `Result` become ordinary std sums (later).** After generic sums work, the four special cases of `Option`/`Result` (`shape.rs` arms, `failure_union`/`result`, the Core type forms in elab and check) are replaced by std declarations, keeping only the sugar (`T | None`, `?`, `else`, R794 injection) in the front end; the null niche becomes a layout rule for any two-variant sum with one empty variant and a niche payload (5.1). Core text stays as R476. This removes a second representation of the same fact; it is the end state, not a prerequisite.

## Open choice for Antonio: `try_send`'s failure

| Option | Shape | Cost |
| --- | --- | --- |
| A (recommended) | `TryRecv[T]` as D8, and `try_send` fails `Full[T]` carrying `value: T` back, like `Undelivered` (R976) | needs generic records (#1259); 9.5 "`try_send` returns `Full`" still holds, no row |
| B | `TryRecv[T]` only; `try_send` keeps `fail Full` and drops the payload | none beyond D8 |
| C | keep both failure shapes | none; leaves #1163 part 2 undone |

Interim either way: #1257 ships `TryRecv[T]` first; under A, a follow-up task makes `Full` generic after #1259.

## Spec freeze

No row is required by D1-D10: generic sums implement 3.5, 10.1, R347, R423 and R772; `TryRecv[T]` and `Full[T]` are std signatures the spec does not fix. Recommended after the freeze lifts, not now: one 9.5 row stating the try op signatures, so the shape is normative rather than living only in std and a conform case. Cases cite existing rows, so spec debt does not grow.

## Plan (ordered, atomic)

| Task | Issue | Depends on |
| --- | --- | --- |
| Native select: rt `select_wait`, MIR `select_key` | #1258 | none |
| T1 shared Core substitution and instance text | #1252 | none |
| T2 elab: generic sum declarations, constructors, patterns | #1253 | T1 |
| T3 check: generic sums checked once, facts per instance | #1254 | T1, T2 |
| T4 native: shapes, drop glue, show per instance | #1255 | T1, T3 |
| T5 conform cases on both backends | #1256 | T4 |
| T6 `try_recv` returns `TryRecv[T]` | #1257 | T5, card |
| T8 generic records through the same path | #1259 | T4 |
| T9 `Option`/`Result` as std generic sums | #1260 | T4, T8 |
| `try_send` fails `Full[T]` (only under option A) | filed after the card | T6, T8 |

Gap to file when T4 lands: recursive generic sums natively (boxed recursive shapes).
