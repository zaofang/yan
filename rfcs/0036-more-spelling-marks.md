# 0036: More spelling marks for law 1

- Status: draft
- Research: none; `docs/reports/2026-11-05-law1-ledger.md` and a read of `crates/yan-elab/src/lower.rs`,
  `crates/yan-hir/src/raise.rs` on 2026-11-05
- Rows: filled in when accepted

## Summary

RFC 0034 closed the mark set at eleven kinds and said adding one is a row. Of the 34 law 1 failures in the
ledger, 19 to 20 are not raise bugs: elaboration erases a spelling and no mark records it. Source items have
a name-table entry, so raise reads an absent mark as "not written that way" and cannot guess. This RFC adds
the marks those cases need: math constants, unary minus, module-qualified constructors, lenient aliases,
inserted conversions, raw and char literal forms, and the lambda negation form. The pass-count impact is
estimated from the ledger, not measured: T04 6, T07 4, T08 3, T08b 2, part of T05 (about 5 of 8).

## Decisions

### D1: a `Const` mark for inlined std constants

Elaboration inlines `math.pi`, `math.e`, `math.inf` and `math.nan` as a float literal or `1e999 - 1e999`.
Add `Const` on the statement or atom the inlined expression becomes; the std constant's path comes from the
node, not from the mark. Raise validates by re-evaluating the constant and comparing to the node's value, and
drops the mark on mismatch (as for literal spelling). Options: (a) a mark, (b) extend literal trivia to hold
`math.pi` text (rejected: nan is an expression, and trivia stores source text, which D2 of RFC 0034 forbids).
Recommend (a).

- Decision:
- Row:

### D2: a `Negated` mark for unary minus

Unary `-x` on a float lowers to `-1.0 * x`. Add `Negated` on the multiplication. Raise validates that the
left operand is the literal `-1.0`. Recommend adding.

- Decision:
- Row:

### D3: a `Qualified` mark for module-qualified constructors

`math.Up()` lowers to `Up()`. Add `Qualified` on the constructor call; raise validates that the variant
belongs to a std sum type with a module. An alternative, always printing the qualified form for std
variants, would break sources that import the bare name. Recommend the mark.

- Decision:
- Row:

### D4: a `Lenient` mark for N-code aliases

N39 and the other lenient readers rewrite a Python spelling (`math.abs`, `math.isnan`, `int(math.floor(x))`,
`math.log(x, b)`) to the canonical call. Add `Lenient(code)` on the rewritten call; the closed set grows by
one variant carrying the N-code, never source text. Raise prints the alias only if the code's rewrite applied
to the alias reproduces the node. Recommend adding. It also serves the n36, n37, n38 and n40 cases.

- Decision:
- Row:

### D5: a `Converted` mark for inserted conversions

`b: bigint = 2` lowers to `bigint(2)`; `half(n)` to `half(float(n))`. Elaboration already writes an origin
mark for inserted nodes (R565) but raise removes it only for kinds it recognizes. Options: (a) extend origin
removal to widening conversions, (b) a new `Converted` mark. Recommend (a), since origin marks already mean
"inserted, not written"; this needs no new mark kind.

- Decision:
- Row:

### D6: raw and char literal forms

`r"\2/\g<a>"` and `c'a'.to_str()` lose their form in `literal`. Extend the existing literal trivia (18.5)
to non-integer literals, validated by parsing back to the node's value. No new mark kind. Recommend.

- Decision:
- Row:

## Spec changes

16.5: the mark table gains the accepted kinds; the literal trivia sentence covers float, string and char
literals. The plan of RFC 0034 gains one step per accepted item, one thread each.
