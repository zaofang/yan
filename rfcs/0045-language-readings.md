# 0045: Language readings assumed by the implementation

- Status: accepted
- Research: none; starts from the language group of `SPEC_QUESTIONS.md` and issue #407
- Rows: R898 to R907

## Summary

The implementation assumes a reading wherever the spec is silent. This note takes the language group of
`SPEC_QUESTIONS.md` (Yan Core text, canonical layout, `uses` lists, the checker, Yan Agent elaboration,
Yan lexical and grammar, stage 0, and the probe gaps) and settles each as an accepted rule, a changed
rule, or an explicit "keep assumed". Std, toolchain and FFI readings are in RFCs 0046 and 0047.

## Decisions

### D1: Yan Core text lexical rules

Options: (a) accept the assumed rules (names `[A-Za-z_][A-Za-z0-9_]*`, contextual keywords, decimal/hex/octal/binary
integers with `_`, float fraction and exponent, no type suffix, the escape set with `\u{hex}`, labels as `'` name,
columns as 1-based UTF-8 byte offsets); (b) add type suffixes; (c) Unicode names.

Recommendation: (a). A suffix duplicates the expected-type rule and Unicode names belong to the surfaces, which
project through Names (16). Byte-offset columns are what the patch protocol and diagnostics already use.

- Decision: as recommended, taken under the lead's standing rule for choices not worth a card; Antonio, 2026-11-07
- Row: R898

### D2: canonical layout

Options: (a) accept: four-space indent, one statement per line, contracts on their own lines before the body,
records and sums on one line, one blank line between items, only the leading comment lines kept, round trip is
`print(parse(print(parse(x)))) == print(parse(x))`; (b) a line-width wrapper.

Recommendation: (a). A width-dependent layout makes the item hash depend on a setting. Comments stay outside the IR.

- Decision: as recommended; Antonio, 2026-11-07
- Row: R899

### D3: `uses` inside lists

Options: (a) a comma continues the capability set unless the next name is followed by `:`, `[` or `.`; (b) require
parentheses around the capability set inside a list.

Recommendation: (a) as the grammar, with (b) accepted as the unambiguous spelling the printer never needs.

- Decision: as recommended; Antonio, 2026-11-07
- Row: R900

### D4: checker readings

Accept: scalars are copied (a `sink` argument does not move them; `become` with `let` of a scalar is not E420); E602
covers direct and mutual recursion in function bodies, and contract calls are not edges; E401 is flow-sensitive and a
branch that ends in `ret`, `fail`, `jump`, `become`, `break`, `continue` or `unreachable` carries no moves forward; a
type the checker cannot work out is unknown and never E201 (integer literals fit any numeric type, float literals any
non-integer numeric type); `yan run` checks first and exits 1 with the diagnostics. The unknown-type rule is a
temporary gap: it shrinks as inference lands and never becomes a rule that forbids stricter checks.

- Decision: as recommended; Antonio, 2026-11-07
- Row: R901

### D5: elaboration, calls and bitwise operators

E210 exempts the arguments of `become` (14.1's list gains it) and is reported once per call, at the first argument
needing a keyword. Bitwise `&`, `|`, `^` on integers are `and_<int>`, `or_<int>`, `xor_<int>` and cannot overflow.
Shifts stay unsupported: their overflow rule is a language decision (count at or above the width, bits shifted out)
and is left for its own RFC. `_ = e` evaluates and discards.

Recommendation: accept all but the shifts, which are an explicit "keep unsupported".

- Decision: as recommended; Antonio, 2026-11-07
- Row: R902

### D6: `par`, `&` elements and lambdas

`par [e for x in xs]` runs sequentially in the interpreter; the observable contract is result order and the
lowest-index failure (9.2). `&xs[i]` and `&r.f` pass a copy stored back after the statement, and two `&` elements of
one list in one call get `exclusive_check` first (4.3). Lambda parameter types come from the expected callable type, or
from the first call when unannotated; lambda arguments are lowered after the other arguments. A lambda captures locals
by copy, `var` parameters `inout` (E406 if it escapes) and capabilities by name (E306 if it escapes). A lambda that
captures a reassigned name is not lowered yet: kept as a known gap.

- Decision: as recommended; Antonio, 2026-11-07
- Row: R903

### D7: Yan lexical and grammar

Accept: signature continuation by `fail`/`uses` lines; E134 for signed grid elements next to non-literals; SIGN and
SPLPAREN read as binary operator and call inside an index; time literals need seconds standalone; `#` applies to the
whole postfix chain; a labelled inout argument, a bare value name as a type and a negative literal pattern are
grammar; E112 allows a one-letter first word before a lower-case letter; Yan is checked strictly (lenient reader off).
`x.len()` is the builtin `len(x)` until a user type's own `len` needs it; unannotated `self` has the impl's type (`Self`
in a trait). Option for `x.len()` when a user type defines `len`: (a) the method wins; (b) the builtin always. Recommend
(a) once the method set exists; recorded as the target.

- Decision: as recommended; Antonio, 2026-11-07
- Row: R904

### D8: stage-0 readings

Accept: top-level `let x: T = #e` evaluates as a nullary function and becomes the constant; steps count executed
calls; a function with one `Nat` parameter is a template with instances `f__k`, `#if` must fold, `#for` runs over a
list literal of constants, errors inside an instance are E272 (except `compile_error`, E270) with a note naming the
instantiation; `embed(path)` resolves against the module's directory. Changed: the package root is the nearest
ancestor with a `package.yag` (the manifest of RFC 0044), not `yan.toml`, else the module's directory. Loop
iterations and allocations stay uncounted until a limit needs them.

- Decision: as recommended; Antonio, 2026-11-07
- Row: R905

### D9: optionals, narrowing and truth tests

A `T` where `T | None` is expected is `Some` of it. `x is None` and `x is not None` narrow through `and`, `or`, `not`,
`while` and an exiting `if` branch; assignment ends narrowing, and a call passing `&x` ends it too (changed: the
implementation does not yet, which is a bug to fix, not a reading). N15 truth tests: numbers `!= 0`, `str` `!= ""`,
collections `len > 0`, optionals `is not None`, other types E201, every condition E201 in strict mode.

- Decision: as recommended; Antonio, 2026-11-07
- Row: R906

### D10: slices, bigint, unions, `post`, literals, ordering

Accept: `s.split()` and `s.chars()` as assumed; `xs[a:b:c]` clamps as `slice.indices`, step 0 panics with kind `value`,
`str` slices unsupported (E504 strict); a `bigint` literal is built from pieces of at most 18 digits; `fail A | B` is an
anonymous union without repeats, `?` injects a member, matching on members is deferred; each `post` is checked at
every return with `out`, failing with kind `assert`, `old(p)` copies on entry, a parameter named `out` is E230; an
out-of-range integer literal is E201 "does not fit" with no dedicated code; `<`, `<=`, `>`, `>=` need `Ord` (E221).

- Decision: as recommended; Antonio, 2026-11-07
- Row: R907

## Spec changes

3.5 (D1, D2), 15.1 and 15.2 (D3, D7), 14.1 (D5, D9, D10), 4.3 and 9.2 (D6), 10.4 (D8), 8.1 and 8.4 (D10). The rows in
`spec/rows.toml` carry the rules; conformance cases for each are proposed in a separate PR.
