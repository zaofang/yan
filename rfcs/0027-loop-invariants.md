# 0027: Inferred loop invariants for the bounds solver

- Status: accepted
- Research: the learnability study behind RFC 0024 (bug 14: binary search keeps its bounds checks)
- Rows: R760 to R763

## Summary

The 8.6 solver forgets everything it knows about a variable when that variable is assigned. As a result, a loop that updates its indices, such as binary search or `i += 1` over a list, cannot prove any index in bounds. Each such index keeps a runtime check and, in Xin, raises W820. This RFC makes four changes, all deterministic: kills keep the facts they imply, `x = x + d` updates its facts, joins keep the facts that every path implies, and a `while` loop infers the invariants that its body preserves. A program still has one verdict in every implementation (R345).

## Decisions

### D1: kills keep implied facts

An assignment that is not covered by D2, or an `inout` use, eliminates the variable from the linear facts by one Fourier-Motzkin step, instead of dropping every fact that mentions it. For example, after `lo = mid + 1`, the fact that `mid >= 0`, which was derived through `lo`, survives. An elimination that would derive more than 64 facts drops instead.

- Decision: accepted (owner, card, 2026-11-01)
- Row: R760

### D2: `x = x + d` updates facts

`x = x + d`, `x += d` and `x -= d`, where `d` is linear and does not mention `x`, replace `x` by `x - d` in every fact. Facts about `x` then follow it through the loop.

- Decision: accepted (owner, card, 2026-11-01)
- Row: R761

### D3: joins keep implied facts

Where paths merge, a linear fact holds after the merge when it holds on one path and the facts of every other path imply it under the step 3 decision. Today the solver keeps only facts that appear verbatim on every path.

- Decision: accepted (owner, card, 2026-11-01)
- Row: R762

### D4: inferred `while` invariants

The candidates are the entry facts that mention a variable the body assigns. The body is analyzed from these candidates and the loop condition, and any candidate the end of the body does not imply is dropped. This repeats until no candidate is dropped. The remaining candidates hold at the loop head, in the body and after the loop. A body that can `continue` to this loop or to an outer loop infers nothing. An inferred invariant is proven, not trusted, so it holds under `--level release`, unlike a written `invariant` (R340).

Options weighed: keep 8.6 as is (binary search stays checked), or trust only written invariants (R340 already does this in `check`). The recommendation, which the owner accepted, was inference, because it adds capability without new syntax and keeps verdicts deterministic.

- Decision: accepted (owner, card, 2026-11-01)
- Row: R763

## Spec changes

8.6 step 2 (kills), with new joins and loop invariants paragraphs after it. One conformance case, `conformance/solver/invariant_untrusted_in_release.xin`, becomes provable under D4. It needs an entry value the solver cannot bound to keep testing R340. That change is proposed in a separate PR.
