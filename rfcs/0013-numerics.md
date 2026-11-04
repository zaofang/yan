# 0013: Numerics: accuracy lint, reverse mode, sparse index equations

- Status: accepted (the lead chose "Accept all three" on 2026-11-01)
- Research: [Theory and compiler frontier survey](https://claude.ai/artifact/YNj73yYvBWphMVzAdNLABE), items 11, 12 and 13
- Rows: R613 to R615

## Summary

Strict IEEE floats, a correctly rounded libm and fixed reduction order make Yan's numeric results
reproducible. This RFC builds on that: a lint that finds inaccurate float expressions, reverse-mode
differentiation that works on every evaluator, and sparse operands for `math.ein`.

## Decisions

### D1: accuracy lint

W852 flags a float expression in a pure function whose error, measured on inputs sampled with a seed
derived from the item hash and narrowed by `pre` clauses, exceeds a threshold. Its fix is the
rewrite with the lowest measured error, found by e-graph search, as in Herbie (PLDI 2015).

- Decision: accepted, 2026-11-01
- Row: R613

### D2: reverse mode as linearize then transpose

Reverse mode is derived as in *You Only Linearize Once* (POPL 2023): forward mode, partial evaluation
that splits off the linear part, then transposition. Each primitive needs only its JVP rule, and
`@derivative` defines that rule. The transform runs on checked Yan Core after monomorphization, so the
interpreter, stage 0 and every code generator compute the same gradients.

- Decision: accepted, 2026-11-01
- Row: R614

### D3: sparse operands for `math.ein`

`Array` gains sparse storage formats as type arguments, per dimension dense, compressed or
singleton (`Sparse[Csr, f64]` and friends). `math.ein` specializes at stage 0 into co-iterating
loops over the operands' formats, as TACO and Finch do. Reduction order stays the fixed tree of 9.3.
The formats may ship first in an `@yan` package.

- Decision: accepted, 2026-11-01
- Row: R615

## Spec changes

11.1 (D1), 11.5 (D2), 11.2 (D3), 25 (W852).
