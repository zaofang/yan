# 0078: A closure argument's capabilities must be covered by the callee

- Status: accepted
- Research: [capability security review, 2026-11-16](../docs/research/2026-11-16-capability-security-review.md) (finding F3, #1390)
- Rows: R1022

## Summary

4 lets a non-escaping closure argument use any capability of its scope, "monomorphized per
closure", and 7.2 makes `uses none` a purity claim that caching, `yan audit` and the agent gate rely
on. A `uses none` function that calls a closure argument capturing `fs` performs I/O, so the claim is
false. This RFC makes the monomorphized instantiation carry the closure's capabilities: they must be
covered by the callee's `uses`. Lead decision 2026-11-16 (card on #1390): check per instantiation,
E301 at the call.

## Decisions

### D1: closure capabilities against the callee's `uses`

A closure passed as an argument to a function of the package captures capabilities by name (4.6).
Each captured capability MUST be declared in the callee's `uses`; otherwise the call is E301, at the
closure argument, naming the capability and the callee. Escaping closures cannot capture capabilities
(E306), so every capturing closure is non-escaping and checked here. No syntax is added: the check is
the callee being instantiated per closure, as 4 already says. Calls into std higher-order functions
are unchanged, since std items are checked as written.

Options weighed: (a) an effect set in the `Callable` type (`Callable[[], T] uses fs`), explicit across
`dyn` and traits but new type syntax and checker work; (b) per-instantiation check, no syntax. The
recommendation, accepted, is (b): escaping closures already cannot hold capabilities, so (b) is sound
and complete for every closure that can capture one.

- Decision: accepted as recommended, per instantiation, E301 (lead, card, 2026-11-16)
- Row: R1022

## Spec changes

4 (the non-escaping closure rule) and 7.2 (E301 sentence).

## Conformance cases

`yag/fail/caps` case: a `uses none` function applied to a closure capturing `fs`, E301 at the
closure; and the pass case with the callee declaring `uses fs` (separate `conform/*` PR).

## Implementation

`yan-check`: bound closures record the capabilities they capture; a call to a module function checks
each closure argument against the callee's capability parameters. Closes #1390.
