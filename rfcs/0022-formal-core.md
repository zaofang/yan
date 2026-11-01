# 0022: A formal model of Yong's core in Lean 4

- Status: accepted (D1); D2 open
- Research: [Formal core study](https://claude.ai/artifact/6PKpzbDykX73NCVzr8nU8y)
- Rows: R680

## Summary

Model a small, safe core of Yong text in Lean 4 under `formal/`, check it against the Rust
interpreter by differential testing, and later prove the language's main claims on it: type
safety, no observable aliasing, capability authority, schedule independence of `par`, and replay.
The model never ships and no crate depends on it, so the toolchain stays Rust only (R215). This is
the Cedar arrangement: Rust in production, a Lean model beside it, and random differential testing
between them on every change.

## Decisions

### D1: Admit a Lean 4 model under `formal/`

Yong-K is the fragment of the 3.5 grammar with records, sums, `let`/`var`, assignment, `if`,
`match`, labelled loops, join points, `ret`, `fail`, `?`, `try`, `become`, plain `par`, `assert`,
`unreachable`, the integer, `f64` and `bool` intrinsics, and `std.io.print`. Phases 2 to 4 add views,
closures, capabilities and handlers. `unsafe`, Tree Borrows, atomics, FFI and stage 0 stay out.

Phase 0, delivered with this RFC, is the executable model (`formal/YongK`) and `formal/drt.py`.
Proofs (phases 1 to 4) start after the M1 gate. Lean is a non-shipping, non-build dependency, the
one exception to R215.

- Decision: accepted, phase 0 now, by the lead on 2026-11-01 (card in the study thread)
- Row: R680

### D2: Which definition is normative for the core

Today the checked interpreter is the executable definition and prose disagreeing with it is a spec
bug (6.1). The Lean model adds a third definition. Recommendation: the prose stays normative for
the core; the interpreter and the model are both checked against it, and any disagreement among
the three is a spec bug to file, not a tie to break.

- Decision: open
- Row: none yet

## Findings from phase 0

Building the model and running 1,000 generated programs turned up these questions. Each needs its
own decision; none is written into the spec by this RFC.

1. **Join point fall-through.** 3.5 does not say what happens when a `jp` body ends without `ret`
   or `jump`. The interpreter and the model both continue after the statement that jumped.
2. **`inout` after a failure.** 4.1 says the caller's variable "holds the callee's final value"
   but does not say whether that holds when the callee fails. The interpreter writes back on
   failure; the model does too. A model without write-back differs on 59 of 1,000 generated
   programs, so the choice is observable and should be stated.
3. **`int / int` rounding.** SQ21 makes `div_i64` correctly rounded. The interpreter computes
   `a as f64 / b as f64`, which rounds twice: `div_i64(27021597764222979, 3)` gives
   9007199254740994.0 where the correctly rounded result is 9007199254740992.0. The model gives
   the latter. This is an interpreter bug, not a spec gap.
4. The six gaps in the study: the normative definition (D2), closure capture in 4.6, the
   "may run at once" relation behind R342, shared effects of cancelled `par` branches, R380 as a
   refinement of immediate panics, and the relation between build modes in 8.5.

## Spec changes

- 22.1: a paragraph on the formal model (R680).
- 26.1: the implementation-language row notes the exception.
