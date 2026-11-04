# 0012: New facts in the header

- Status: accepted (the lead chose "Accept all as normative" on 2026-11-01)
- Research: [Theory and compiler frontier survey](https://claude.ai/artifact/YNj73yYvBWphMVzAdNLABE), items 8, 9 and 10
- Rows: R610 to R612

## Summary

A reviewer should be able to trust agent code from its header (R210). This RFC adds three facts a
reviewer still had to read the body for: how much work and allocation a function does, whether it is
timing-safe on secrets, and why it needs a capability, failure or panic it declares.

## Decisions

### D1: cost bounds

A `cost` clause in a function header, `cost steps <= e1, alloc <= e2`, with each bound a linear
expression over parameters and `len` of parameters. `steps` counts R487 stage-0 steps on the
abstract machine; `alloc` counts `alloc` operations before optimization (R385 makes the real count
unobservable). Bounds are checked by automatic amortized resource analysis (Hoffmann et al.;
RaRust, 2025), reduced to a linear program. A refuted bound is E618 with a witness input; an unproven
one is W851 and compiles to a step and allocation counter that panics with kind `contract` in
`checked-all`, following R211. Callees' `cost` clauses are their specifications, so law 6 holds.
Yan Core, Yan Agent and Yan all spell the clause `cost`. The lead admitted the Yan Agent spelling on 2026-11-01
ahead of the R566 probe ("Yan Agent syntax is fine if it's worth it"): it is a contract line, so a Yan Agent
program with contracts removed is still Python (law 10). Tools show inferred bounds as inlays.

- Decision: accepted as normative, 2026-11-01
- Row: R610

### D2: constant-time secrets

`@constant_time` on a function makes every value computed from `expose()` inside it
secret-derived. A branch, a `match`, an index, a division or remainder, a variable-time intrinsic or
an early exit that depends on a secret-derived value is E424. std adds `ct_select`, `ct_eq` and
`ct_lt`. Release code generation lowers them to LLVM's constant-time selection support; Cranelift
builds call `yan-rt` helpers. Sources: FaCT (PLDI 2019), Jasmin.

- Decision: accepted, 2026-11-01
- Row: R611

### D3: `yan why` for item facts

`yan why <item> <fact>`, where the fact is a capability, a `fail` member or a panic kind, prints
the shortest chain of statements and callees that makes the item need it, one line and span per step
(thin slicing, PLDI 2007). An empty chain means the declaration is unneeded, and the fix removes it.
With one argument `yan why` keeps its dependency meaning (19). MCP adds `why_needs`.

- Decision: accepted, 2026-11-01
- Row: R612

## Spec changes

3.5, 8.4, 13, 14 and 15 (D1); 17 (`Secret[T]`, D2); 18.1 and 21.4 (D3); 25 (E618, W851, E424).
