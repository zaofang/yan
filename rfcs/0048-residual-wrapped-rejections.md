# 0048: Residual wrapped rejections from the R567 failure-mix study

- Status: accepted
- Research: R567 failure-mix study (T18), item D3
- Rows: R927 to R929

## Summary

The study lists six first-reply patch rejections behind groups G4 and G5. Re-checking the stored records against
main shows that three of the six already pass, one is a limit, and three need a change. The inner error of Sonnet
mbpp 724 was not the `**` form: it was `parse_int` called on a `char`. The change follows the Python-fidelity policy:
a real function or a numbered normalization, never a bare error.

## Decisions

### D1: `parse_int` and `parse_float` on `char`

`c.parse_int()` and `c.parse_float()` are methods of `std.text` that apply to the one-scalar `str`, as `upper` and
`lower` do on `char`. They return `int | None` and `float | None`. Python's `int(c)` on a digit is the model's
intent. Recommended: accept.

- Decision: accepted; Antonio, standing approval of 2026-11-07
- Row: R927

### D2: `re.finditer`

`re.finditer(pattern, s)` and `compiled.finditer(s)` normalize like `findall` to `find_all`, which already returns
`Match` values in order, so a `for` over it behaves as over Python's iterator. Haiku mbpp 832 died on this name with a
bare E200. Recommended: accept.

- Decision: accepted; Antonio, standing approval of 2026-11-07
- Row: R928

### D3: a suggested fix for option arithmetic

An arithmetic operator on an `int | None`, `float | None` or `str | None` operand keeps E201 (Python raises on
`None + 1`, so no silent normalization), and carries a `choice` fix that wraps the operand as `(x else 0)`, `(x else 0.0)`
or `(x else "")`. Use after move (E413) already carries an `auto` fix `x.copy()`; no change. Regex look-behind stays a
limit (E545). Sort on a `char` iterator needs no change: `sorted(s.chars())` and `s.chars().sorted()` are accepted.
Recommended: accept.

- Decision: accepted; Antonio, standing approval of 2026-11-07
- Row: R929

## Spec changes

14.4 (N34, N35), 17 (D1), 25 (D3). Conformance cases are proposed in a separate PR.
