# 0051: @access binding syntax

- Status: draft
- Research: RFC 0016 D3 (Fern, PLDI 2025)
- Rows: filled in when accepted

## Summary

RFC 0016 D3 names the regions an `@access` claim can declare but not how a region binds to a
parameter or the result. This fixes the binding syntax so the parser, checker and `yan audit` agree.

## Decisions

### D1: named bindings, one region per operand

`@access(name: region, ...)` has one entry per array parameter, in any order, and the reserved
name `out` for the result. A region is `elementwise`, `rows(axis)`, `cols(axis)`, `tile(M, N)` or
`whole`, where `axis`, `M` and `N` are integer literals. Examples:

    @access(a: rows(0), b: whole, out: elementwise)
    extern fn scale(a: Array[f64], b: Array[f64]) -> Array[f64]

Options weighed: positional regions (breaks when parameters are reordered, and the result has no
position), and one attribute per operand (repeats the attribute name). Named bindings are checked
against the signature, so a misspelled parameter is an error rather than a silent no-claim.
Recommended.

Rules: every array parameter and a non-unit result MUST appear exactly once; non-array parameters
MUST NOT appear; `rows(axis)` and `cols(axis)` need an axis below the operand's rank, and `tile(M, N)`
needs positive M and N. A malformed claim is a syntax-level error before any body is read.

- Decision: (lead's answer, date)
- Row: (R number once accepted)

## Spec changes

Section 11.2 (the access patterns paragraph) gains the binding grammar and rules; section 10.6
attribute table entry for `@access` links to it; the diagnostics catalogue gains one code for a
malformed claim.
