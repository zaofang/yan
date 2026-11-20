# 0098: The failable integer conversion and the meaning of `trunc_as`

- Status: draft
- Research: #1770 (gap issue, RFC 0096 D3 deferred); RFC 0096 D2 (`checked_as` panics) and D3
- Rows: filled in when accepted

## Summary

RFC 0096 gave Yan three integer conversion policies that trap, wrap or clamp (`checked_as`, `wrap_as`, `sat_as`) and deferred two things (#1770): a conversion that returns `Option[T]` when the value does not fit (Rust's `try_from`), whose spelling is undecided, and `trunc_as`, which spec 3.4 and 14 list but which has no stated meaning and which lowering rejects (`conv.rs`: "`trunc_as` is not supported"). This RFC decides the spelling of the failable form and what `trunc_as` means, so neither stays a name in the spec with no behaviour. Nothing here is implemented; implementation follows acceptance.

## Decisions

### D1: spelling of the failable conversion

The function returns `Some(x)` converted when the value is in `T`'s range and `None` otherwise. Source and target types follow `checked_as` (RFC 0096 D2): integers of at most 64 bits; `i128`, `bigint` and non-integer targets stay fail-loud until their own RFC. It lowers through the same range test that `checked_as_<from>_<to>` uses, so there is one range rule, not two (practice 12).

Options:

- A. `try_as[T](x)`. Completes the family: `wrap_as`, `sat_as`, `checked_as` (trap), `try_as` (`Option`). The name says "may fail, tells you" and does not reuse `checked_`, whose two meanings (arithmetic returns `Option`, `checked_as` traps) are the tension RFC 0096 D2 recorded.
- B. `checked_as_opt[T](x)`. Names its relation to `checked_as`, but is a suffix convention nothing else in the language uses, and keeps `checked_` meaning two things.
- C. Change `checked_as` to return `Option[T]`, matching `checked_add`. Rejected: reverses accepted RFC 0096 D2, breaks the yn `as` equivalence (`checked_as[T](x)` is `x as T` as a function) and puts `else` at every narrowing that must fit.
- D. `T.try_from(x)`. Rust spelling, but Yan Agent is Python-shaped and the other three policies are free functions with `[T]`; a method form would be a second call shape.
- E. `x as? T`. Rejected: yag has no `as` (RFC 0096 D1 B), so it would be Yan only and would still need a function spelling.

Recommended: A. It is the only option that leaves every existing name's meaning untouched, keeps one call shape across the four policies, and works in both surfaces.

- Decision:
- Row:

### D2: what `trunc_as` means

Today the spec lists `trunc_as` beside `wrap_as` and `sat_as` and defines none of them beyond the name; for integer sources, keeping the low bits is already `wrap_as`, so an integer `trunc_as` would be a second name for it.

Options:

- A. Float to sized integer: `trunc_as[T](x)` takes a float and truncates toward zero, panicking with kind `overflow` when the truncated value (or a NaN or infinity) is outside `T`'s range. This is the sized-target form of `int(x)` (spec 14: truncates toward zero, panics on NaN or out of range). An integer source is a type error naming `wrap_as`, `sat_as` and `checked_as`. It fills a real gap: no spelling converts a float to `u8` or `i32` today.
- B. Retire `trunc_as` from the spec. Smallest surface, but leaves float to sized integer without a spelling, and removing a frozen name needs a lead card (practice 15).
- C. Alias of `wrap_as` for integers (keep low bits). Rejected: two names for one behaviour, and the name suggests float truncation.
- D. Keep it specified but unsupported. Rejected: a spec name with no behaviour, against practice 13 in spirit.

Recommended: A. The name already reads as truncation, the behaviour reuses the `int(x)` rule so there is one float-to-integer rounding rule, and it keeps the four-way policy table total: each of integer-to-integer (wrap, sat, checked, try) and float-to-integer (trunc) has a spelling. A failable float form is not proposed here; no program asks for it.

- Decision:
- Row:

## Spec changes

Only after acceptance, with rows and cases in the same PR (CASE WITH ROW). 3.4: one sentence adding `try_as[T](x)` (returns `Option[T]`) and the `trunc_as` definition. 14 (Numbers): the conversion list names `try_as` and states `trunc_as` takes a float. Implementation: `crates/yan-elab/src/lower/call_expr.rs` (route `try_as`) and `conv.rs` (drop the `trunc_as` rejection once defined); `crates/yan-diag/src/habit.rs` hints where they name the family. Interpreter and native outputs of every case attached to the implementing PR.
