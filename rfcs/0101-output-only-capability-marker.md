# 0101: The `@output_only` capability marker

- Status: accepted
- Research: #1933 (decision issue); part of #413 (D3 of the checker design)
- Rows: pending

## Summary

`replay(trace, live=[...])` may name only capabilities whose operations return nothing the program reads (spec 7.6). The checker enforces this with a hard-coded list, `{log, metrics}`. A user capability, or a std capability added later, has no way to qualify. This RFC adds a declared property on the capability trait and makes the E316 check read it, so the list lives next to each declaration and the checker holds no names.

## Decisions

### D1: an `@output_only` attribute on `@capability` traits

`@output_only` is an attribute on a `@capability` trait, in the same registry as `@splittable` and `@shared` (spec 10). It states that no operation of the trait returns a value the program can observe: every operation returns `unit` and none declares `fail`. `Log` and `Metrics` carry it in std; the hard-coded list is deleted.

Options weighed:

1. Attribute on the declaration (chosen). One fact in one place; user capabilities qualify; composes with `@splittable`.
2. A closed list in the compiler, extended per release. Keeps the current defect: user capabilities never qualify and each addition is a compiler change.
3. A separate trait `OutputOnly` that capabilities implement. A second representation of the same fact, and a trait cannot constrain operation signatures.

- Decision: accepted (Antonio, card cmsg_014jqsmRQfVdtFkGwF8cBScEPQghJJBLhMLgbAa67vRVAA, option A, 2026-11-27)
- Row: pending

### D2: the marker is checked against the operations

A `@capability` trait marked `@output_only` whose operation returns a non-`unit` type or declares `fail` is a compile error at the declaration, with a new E3 code. E316 then fires for any `live:` name whose capability is not marked. A capability that gains a returning operation fails at the declaration, never silently at a replay site.

- Decision: accepted with D1 (the marker means nothing unless enforced)
- Row: pending

## Spec changes

- 7.6 (Trace format and divergence): replace "(`log`, `metrics`)" with "marked `@output_only`".
- 7.1 capability table: `log` and `metrics` rows note `@output_only`.
- 10.5 attribute table: add `@output_only`.
- 25 diagnostics catalogue: E316 text, and the new D2 code.
- Rows, codes and cases are numbered in the implementation PR (`tools/rows.py next N`, `next-code E3`), each row with its conformance case.
