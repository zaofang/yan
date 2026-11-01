# 0010: Exact integer solver and guard-aware matches

- Status: accepted (the lead chose "Accept both" on 2026-11-01)
- Research: [Theory and compiler frontier survey](https://claude.ai/artifact/YNj73yYvBWphMVzAdNLABE), items 1 and 2
- Rows: R605, R606

## Summary

The always-on solver of 8.6 decides goals with Fourier-Motzkin elimination, which is exact only over
the rationals, and the `%` and `&` rules add fresh variables not tied to their operands. So some
integer goals stay unproven and become runtime checks with W820. This RFC makes the solver exact for
its fragment and lets `match` use it, so integer and refinement guards count toward exhaustiveness.

## Decisions

### D1: the Omega test decides R345 goals

Replace step 3 of the R345 procedure with the Omega test (Pugh, 1991): exact elimination of
equalities, then real shadows, with dark shadows and splinters where the two differ. `x % c` is
encoded as `x = c*q + r` with `0 <= r < c`, and `x & m` for `m = 2^k - 1` as `x % 2^k`. The
8-case and 10,000-constraint caps stay, splinters count against the cap, and elimination order is
fixed, so results stay a function of the program alone.

Options weighed: keep Fourier-Motzkin (incomplete on integers); an external SMT solver (rejected by
8.6, which requires no external dependency). Recommendation: accept.

- Decision: accepted, 2026-11-01
- Row: R605

### D2: guard-aware exhaustiveness

Check coverage as in *Lower Your Guards* (Graf, Peyton Jones and Scott, ICFP 2020), with the 8.6
solver as the oracle: guards and refinement ranges are facts. A match is exhaustive when the solver
proves the uncovered set empty. E503 reports a witness of an uncovered value, and its fix inserts an
arm for it. An arm whose matched set is empty is a new lint, W853, with a fix that removes it.

Recommendation: accept.

- Decision: accepted, 2026-11-01
- Row: R606

## Spec changes

8.6 (fragment, R345 step 3, new exhaustiveness paragraph); 14.1 (Match); 15 (Patterns); 25 (W853).
New boundary cases in `conformance/solver/` are for the lead to add.
