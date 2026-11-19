# 0095: Handler-bound `ex` lines and `par for` statements

- Status: accepted
- Research: R567 hard rerun 2026-11-19 (`bench/r567/results/hard-2026-11-19/compare.md`), #1746, #1749
- Rows: pending

## Summary

Two forms the spec already describes are not implemented, and models reach for both. An `ex` line cannot bind a capability handler (7.2, 17), so a function that declares `uses` cannot carry an example; the toolchain rejects the call at the `ex` line. A `par for` whose body has type unit (9.3) is rejected in `.yn` as not supported, because elaboration lowers only the `let ys = par for` form. PR #1756 added habit hints and primer lines as a stopgap. This RFC decides the real features, implementation deferred to later slices.

## Decisions

### D1: `ex with handler:` in `.yag` and `.yn`

Allow `ex with h: f(a) == r` (`.yn`) and `ex with h f(a) == r` (`.yag`), where `h` is a handler expression such as `fake_net(...)`, `fake_fs(...)` or `fake_model(...)`. The `with` binds the capability for that one example only, so `uses` functions can be exemplified under test handlers. A real handler in an `ex` makes it an I/O test, as the runner rules already say. Recommended: the spec already allows it for models, it removes the largest first-reply failure class in the hard suite, and it keeps `ex` the single place examples live.

Alternative: keep `ex` capability-free and let `uses` functions carry only `pre` and `post`. No implementation cost, but their behaviour then has no example the agent can write.

- Decision: accepted as recommended: `ex with handler:` binds a capability for one example (lead, 2026-11-19). Implementation deferred to #1746.
- Row:

### D2: `par for` as a statement

Accept a unit-bodied `par for` in statement position in `.yn`, lowered as the streaming form of 9.3 (`limit`, `concurrent`), with the same failure rule as the spec states. Elaboration reuses the existing `par for` lowering and drops the result. The native side stays with #1632. Recommended: it is the specified form and models write it unprompted.

Alternative: require a binding (`let _ = par for ...`) and make the statement form a typed diagnostic. Cheaper, but it contradicts 9.3.

- Decision: accepted as recommended: unit-bodied `par for` statement in `.yn` (lead, 2026-11-19). Implementation deferred to #1749; native side stays with #1632.
- Row:

## Spec changes

7.2 and 17: handler binding inside `ex`. 9.3: statement position for `par for`. Catalogue: none expected.
