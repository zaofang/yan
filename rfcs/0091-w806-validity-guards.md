# 0091: W806 on validity guards

- Status: draft
- Research: bench/r567/results/hard-escape-2026-11-18/summary.md; issue #1690
- Rows: filled in when accepted

## Summary

W806 (R675) flags any branch whose condition compares a parameter with a literal that an `ex`
line passes for it. In the R567 hard suite, 5 Sonnet runs wrote a path validator with `ex f("") ==
False` and a guard `if name == "": return False`, and W806 rejected the guard, which is a real
branch and not a lookup of the example. This RFC narrows W806 to branches that are the only source
of the example's result.

## Decisions

### D1: flag a branch only when removing it changes the example

W806 fires on a branch matching an `ex` input only if evaluating that `ex` against the body with
the branch removed (its arm replaced by fall-through) no longer yields the stated result. A guard
whose result the remaining body already produces is not special-casing. Lookups keyed by an `ex`
literal (`case lit`, dict literal, `.get`) keep the existing rule.

Options weighed: D2 exempt empty values (`""`, `[]`, `0`): cheap, but misses non-empty guards and
still lets `if n == 0: return 1` hard-code a base case; D3 keep: models keep dropping guards or
burn repair rounds. Recommended: D1.

- Decision: accepted by the owner 2026-11-18 ("accept all most correct long term options")
- Row: assigned in the implementing PR

## Spec changes

21.1 (W806 definition), R675 wording; catalogue W806 text. The implementation is a separate PR
with its row and paired conformance cases.
