# 0021: Error messages

- Status: accepted
- Research: [Yan error messages audit](https://claude.ai/artifact/6catGcMdztyszRmvJLoiRu)
- Rows: R648 to R651

## Summary

The error messages audit ran every conformance fail case through `yan check` at 7dc69a2 and
scored the 115 codes it emits against Elm's and rustc's conventions. Most of the gap is
implementation of R290 to R299, which are already normative. Four items needed a spec change:
an emitted code missing from the catalogue, unsupported constructs that end in a bare stderr line,
a line grammar that prints empty fields, and near-name suggestions.

## Decisions

### D1: register E545 and cross-check cited codes

E545 (a rejected literal regular expression, R484 and R486) is emitted, tested and cited in 17,
but section 25 does not list it. Register it, and make `tools/rows.py check` fail when the spec
cites a code that section 25 does not list.

- Decision: accepted, 2026-11-01
- Row: R648

### D2: a code for unsupported constructs

43 of 278 fail cases end in `yan: not supported yet: X` on stderr, with no record, no span and
the rest of the file unchecked. Options were a new E code, an I code, or no change. A missing
feature is not an internal error, so it gets an E code: E017, reported at the construct, which is
marked as in R291 so checking continues.

- Decision: accepted (new code, keep checking), 2026-11-01
- Row: R649

### D3: empty item and quote in the line format

Syntax errors outside an item and patch rejections without search text print a bare colon and empty backticks
in the R294 line. The grammar now leaves out an empty item or quote with its separators.

- Decision: accepted, 2026-11-01
- Row: R650

### D4: near-name suggestions

E200 and E203 carry a name in scope, a field, a method or a std module member within edit
distance 2 as a `maybe` fix, nearest first, at most three.

- Decision: accepted, 2026-11-01
- Row: R651

## Spec changes

- 18.2: the E017 rule after R291 (D2); the R294 grammar and its sentence on empty fields (D3); the
  near-name sentence in R298 (D4).
- 25.2: E017 (D2). 25.7: E545 (D1).
- `tools/rows.py check`: cited codes must be in section 25 (D1).
