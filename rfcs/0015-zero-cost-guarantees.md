# 0015: Guaranteed zero-cost abstractions

- Status: accepted (the lead chose "Accept all three" on 2026-11-01)
- Research: Deep frontier survey: speed and capability, picks 1, 4 and 7
- Rows: R622 to R624

## Summary

Closures, iterator chains and data-parallel loops are fast in Yan only when the optimizer happens to
see through them. This RFC turns three of those optimizations into guarantees that every backend
keeps, in line with the rule that builds never change behavior.

## Decisions

### D1: lambda set specialization

After monomorphization, infer for each function-typed value the set of lambdas that can reach it
(Brandon et al., PLDI 2023, as shipped in Roc). A value with a closed set is a tagged record of its
captures, called directly or through a switch on the tag. When its captures are not heap-backed it
MUST NOT allocate or be called indirectly, so it passes `@forbid(alloc)`. Values that pass through
`dyn` or recursive containers stay boxed. This replaces the env block and `call_indirect` lowering of #90.

- Decision: accepted, 2026-11-01
- Row: R622

### D2: complete fusion of std adapter chains

A chain of std adapters, `zip` over `flat_map` included, consumed by a `for`, a reduction or
`collect`, MUST compile to one loop with no allocation beyond `collect`'s result and no indirect call
(Kiselyov, Kobayashi and Palladinos, Complete Fusion for Stateful Streams, 2024). This extends R480
from comprehensions to all of `std.iter`.

- Decision: accepted, 2026-11-01
- Row: R623

### D3: SIMD lanes for `par for` and `@vectorize`

A `par for` whose body is `uses none` MAY run its iterations in SIMD lanes with divergent branches
masked (Moll and Hack, Partial Control-Flow Linearization, PLDI 2018). Results are identical to scalar
execution. `@vectorize` on a function makes this a proof obligation, with E900 for each blocking line.

- Decision: accepted, 2026-11-01
- Row: R624

## Spec changes

5.5 (D1), 13 and 17 (D2), 5.7, 9.3, 10.6 and 11.3 (D3).
