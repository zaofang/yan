# 0092: `?` on a reified `Result` value

- Status: accepted
- Research: R567 hard rerun, #1727 (`bench/r567/results/hard-2026-11-18b/`)
- Rows: R1106, R1107

## Summary

`x?` is lowered only for a call (`Rhs::Propagate` carries the call). A bound `Result[T, E]` value, which spec 14.1 already allows where a `Result` is expected, cannot be propagated: the toolchain answers E017. Models write `ys?` after a `par for` whose body lacks `?`. This RFC decides whether `?` on a value is a real feature.

## Decisions

### D1: `?` on a `Result` value

Allow `v?` where `v` has type `Result[T, E]` (a local, field or any expression), with the same injection and `From` rules as a call (E310, E313). Elaboration adds a `Rhs::PropagateValue` that unwraps the success arm and fails with the `E` arm; the interpreter and native backends share the existing propagate tail. Recommended: it completes the reified-`Result` story (R424) and removes the last E017 on `?`.

Alternative: reject `v?` with a typed diagnostic and a fix pointing at the call. Cheaper, but a bound `Result` then cannot be propagated without `match`.

- Decision: accepted as recommended (standing take-the-recommendation, 2026-11-18)
- Row: R1106

### D2: E017 on `?` of a non-`Result` value

Until D1 lands, and for a value that is not a `Result` or `T?`, report E315-style guidance instead of E017: the operand is not fallible; the habit hint already says to put `?` on the call.

- Decision: accepted as recommended, with E201 as the code (an existing code, no new one); the message says the value cannot fail (2026-11-18)
- Row: R1107

## Spec changes

14.1 (Results): `x?` accepts a reified `Result` value. 25 (catalogue): E201 covers `?` on a value that cannot fail.

## Slice 1

Implemented in elaboration only: `v?` lowers to `match v { Ok(x) => x, Err(e) => fail conv(e) }`, so HIR, MIR, the interpreter and native need no new form. `Option` values stay E017. Native `?` through `Error` wrapping stays unsupported, as for a call's `?`.
