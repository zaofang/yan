# 0096: `checked_as`, the checked integer conversion function

- Status: accepted
- Research: #1747 (R567 hard rerun 2026-11-19, `hard_rotl8`, `hard_bit_parity`), #1765 (habit hints; "yag has no checked narrowing" left for a decision)
- Rows: R1112

## Summary

Yan has three integer conversion policies: checked (`x as T`, panics with kind `overflow` when the value does not fit), wrapping (`wrap_as[T](x)`) and saturating (`sat_as[T](x)`). Yan spells the checked one as `as`; Yan Agent has no `as` (it is Yan only by design: the printer treats `Cast` as unreachable and the resugarer rejects it), so a yag program has the other two and `int(x)` for widening to `int`, but no checked narrowing to a sized type. Models reach for `uint8(x)` and get a hint naming the wrapping and saturating forms (#1765); a program that wants "fail loudly if it does not fit" has no spelling. The elaborator and both backends already implement it: `int_conv("checked_as", ..)` lowers every yn `as` to the `checked_as_<from>_<to>` intrinsic, and only the call spelling `checked_as[T](x)` is missing, as `wrap_as[T](x)` is. This RFC decides that spelling, its failure mode, and its place among the Python-fidelity habits.

## Decisions

### D1: how yag spells checked narrowing

Options:

- A. `checked_as[T](x)`, a generic function completing the family `wrap_as`, `sat_as`, `checked_as`, in both surfaces. One more call-spelling arm beside the existing two in `call_expr.rs`; no new lowering, intrinsic, MIR or backend work.
- B. `x as T` in yag. Rejected: Yan Agent is Python-shaped and `as` is not an expression in Python; it also adds a second parser path for the same `Cast` node the family already reaches.
- C. `T(x)` constructor spelling (`uint8(x)`). Rejected: it is the habit that fails today, but Python's habits disagree on its meaning: `numpy.uint8(300)` wraps or raises by version, `ctypes.c_uint8(300)` wraps, `bytes([300])` raises. An alias would silently pick one policy for a value that is nearly always a bug when it does not fit. Narrowing must name its policy at the call.
- D. `int.to_uint8(x)` style methods. Rejected: one method per target type, no generic form, and no home for the policy name.

Recommended: A. One fact, one representation (practice 12): the checked conversion keeps one implementation, reached by `as` in Yan and by `checked_as[T]` in Yan Agent, and the three policies sit side by side with the same call shape.

- Decision: accepted as recommended: A, `checked_as[T](x)` in both surfaces (lead, 2026-11-19)
- Row: R1112

### D2: failure mode and semantics

`checked_as[T](x)` has exactly the semantics of `x as T` today: the value is returned unchanged when it is in `T`'s range, otherwise the program panics with kind `overflow` (spec 3.4, "overflow is checked in every build mode"). The source is any integer type of at most 64 bits, the target any integer type of at most 64 bits; widening (`u8` to `u32`, `i32` to `i64`) never fails, and the range solver (8.6) removes its check; `u64` to `i64` fails above `2**63 - 1`; a negative value to an unsigned type fails. A non-integer source (float, bool, str) is a type error that names `int(x)` for floats; `i128`, `bigint` and non-integer targets keep their present fail-loud "not supported" until their own RFC (practice 13). `int(x)` stays the spelling for sized to `int`, and `checked_as[i64]` is equivalent to it from a sized integer.

Options:

- A. Panic, as `as` does. Matches yn today, needs no new intrinsic, and keeps a single failure story for the three policies: wrap, clamp, trap.
- B. Return `Option[T]`, as the arithmetic `checked_*` family does (`checked_add`). Consistent with those names, but a different function from `as`: two representations of "checked conversion" with different failure modes, and the bench habit (a narrowing that must fit) would then need `else` at every call.

Recommended: A for `checked_as`. Tension recorded openly: `checked_*` arithmetic returns `Option`, `checked_as` traps. The name follows the conversion family (spec 3.4, `as` "is checked"), not the arithmetic one. The failable form is D3.

- Decision: accepted as recommended: A, panic with kind `overflow` (lead, 2026-11-19)
- Row: R1112

### D3: the failable conversion, deferred

A conversion that returns `Option[T]` (Rust's `try_from`, Python's range test) is useful and is not the same function. Naming it now would freeze a second spelling for an unmeasured need; no R567 or conformance program asks for it. Recommended: do not add it here; file it as a numbered item in a gap issue (practice 9), and decide the spelling by RFC when a program needs it, with the `checked_*` naming tension from D2 on the table. Deferred, not dropped.

- Decision: accepted as recommended: deferred to a gap item (lead, 2026-11-19)
- Row:

### D4: Python habits

Policy: a real function or a normalization, never a bare unresolved-name error. With D1 A the real function exists, so the habit entries added in #1765 for `uint8(x)`, `u8(x)`, `np.uint8(x)` and `ctypes.c_uint8(x)` name `checked_as[uint8](x)` first (fails if it does not fit), then `wrap_as` and `sat_as`. No lenient-reader rewrite: as in D1 C, the Python habits do not agree on the policy, so a rewrite would choose it silently. `trunc_as` stays specified but unsupported in lowering and keeps its fail-loud diagnostic; it is not decided here.

- Decision: accepted as recommended: hint entries only, no lenient rewrite (lead, 2026-11-19)
- Row:

## Spec changes

3.4: one sentence, "`checked_as[T](x)` is `x as T` as a function; it is the only checked spelling in Yan Agent". 14 (Numbers): `checked_as` joins `wrap_as`, `sat_as`, `trunc_as` in the conversion list. 25 (catalogue): the hint text for the sized-constructor habits changes to name `checked_as`. Rows and cases are added only after acceptance, in the same PR (CASE WITH ROW).

## Slice 1 (after acceptance)

`crates/yan-elab/src/lower/call_expr.rs`: add `checked_as` to the `matches!` that routes `wrap_as | sat_as | trunc_as` to `conv_call`; `conv_call` already passes the kind to `int_conv`. `crates/yan-diag/src/habit.rs`: the D4 entries and unit test. Cases, proposed on `conform/*` branches: a yag and a yn program printing in-range and widening results and one that panics with kind `overflow` (`checked_as[u8](300)`), a `u64` to `i64` boundary pair, and a negative to unsigned panic. Interpreter and native outputs of every case are attached to the PR as the Working evidence; no backend change is expected because both already implement `checked_as_<from>_<to>`.
