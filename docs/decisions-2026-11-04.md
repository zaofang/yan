# Open decisions answered 2026-11-04

Answered by Antonio on question cards. No spec rows yet; rows are allocated in the PR that implements each.

1. Prelude `sum` accepts `list[float]` and uses the full 9.3 reduction order (R382): blocks of 4096, 64 lanes, fixed tree, identity -0.0.
2. Drop elaboration (R770) stays accepted but unbuilt until after M1.
3. The handler in a `with` expression is type-checked now.

Untrusted-text marking was decided earlier (syntax Q3): label propagation plus `Untrusted[T]`, RFC after M1.
