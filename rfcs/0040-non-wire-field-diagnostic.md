# 0040: Diagnostic for a field that is not `Wire`

- Status: draft
- Research: none; the gap is the check dropped from `crates/yan-elab/src/decorators.rs` in commit de6e6c1, 2026-11-06
- Rows: filled in when accepted

## Summary

`@derive(Wire)` on a record or sum whose field type is not `Wire` (a `Callable`, a capability, a built-in
handle, a class with no `Wire` derive) must be rejected at compile time. The schema builder in
`crates/yan-elab/src/wire.rs` already finds the offending field and writes a message, but the elaborator no
longer reports it. The check was removed because the code it used, the generic derive-misuse code ("derive
of a trait without a `derived` body, or of an auto trait"), does not describe this failure, and a new code
needs a spec row. Without the check the failure surfaces only when a codec (`json`, `csv`, actor transport)
first reads the schema. This RFC decides the code and the span and text of the diagnostic. The same failure
for an actor handler crossing a machine boundary already has its own code and is out of scope.

## Decisions

### D1: which code reports it

Options: (a) a new code in the derive-misuse family (section 25, beside the unsupported-derive code),
dedicated to "derived `Wire` has a field that is not `Wire`"; (b) widen the existing derive-misuse code to
cover it, as the removed check did, which keeps one code but makes its row a list of unrelated cases with
different fixes; (c) a family-wide code for "derived trait has a field that does not implement it", which
would also serve `Eq`, `Ord`, `Hash` and `Show` if their derives report field failures. Recommend (a): a
dedicated code is cheap to document and test now, and (c) can replace it later if the other derives show the
same gap.

- Decision: option (c), a family-wide code for "derived trait has a field that does not implement it", chosen by the lead on 2026-11-06. The code sits in the derive-misuse family (section 25).
- Decided 2026-11-06: the same check applies to `Eq`, `Ord`, `Hash` and `Show` derives. Today the elaborator has no field check for them; the failure appears only at a use site (E221). The writing PR adds the check to each of these derives under the same code.
- Row: (R number once accepted)

### D2: span and text

Options: (a) report at the `@derive(Wire)` decorator and name the first offending field, as the dropped
check did; (b) report at the field's type annotation, one diagnostic per offending field, so every fix site
is underlined; (c) report at the decorator and add a note per further offending field. A second choice
inside each option: whether a field whose type is a user class says "does not derive `Wire`" (the message the
schema builder already writes) or only "not `Wire`", and whether a capability value is called out as never
`Wire`, since section 17.4 says so. Recommend (b) with the two existing message forms plus the capability
note: one diagnostic per field means one fix per diagnostic, and the span is where the change is made.

- Decision: option (b), one diagnostic per offending field at its type annotation, chosen by the lead on 2026-11-06.
- Open within D2: the message form for a user-class field ("does not derive `Wire`" or "not `Wire`") and whether a capability value gets its own note. Not decided.
- Row: (R number once accepted)

## Spec changes

Section 25 (one catalogue row for the code chosen in D1), section 10.4 (the derives paragraph names the new
failure beside the unsupported-derive failure), section 17.4 (the `Wire` decoding paragraph states that a
field that is not `Wire` rejects the derive at compile time, with the span from D2). Code: restore the check
in `decorators.rs` and the self-test removed in de6e6c1. The conformance case under
`conformance/yag/fail/derives/` comes in a separate PR, since agents do not edit `conformance/`.
