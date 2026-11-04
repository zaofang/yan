# 0023: `yan review`, a report for agent PRs

- Status: accepted (the lead chose "Accept all (D1 to D7)" on 2026-11-01)
- Research: [What Yan is and how it fits together](https://claude.ai/artifact/HtZ81sCkbF8LnYJRNrGeVU), gap 2
- Rows: R665 to R671

## Summary

Yan's claim is that a reviewer can trust agent code from its header without reading the body
(R210). Nothing yet shows the header facts of a change in one place. `yan review` compares two
revisions of a package item by item and prints what a reviewer must look at: which items changed,
which gained or lost authority, which contracts and tests changed, whether approvals moved, and the
verdicts of the tests that reach the changed items. It is the first slice of gap 2 in the product
study, scheduled for M1. It differs from `yan audit --diff` (M2), which compares dependency
lockfiles; `yan review` compares the package's own items.

## Decisions

### D1: the command

`yan review [BASE[..HEAD]] [--format line|json|md]`. BASE and HEAD are git revisions; HEAD left out
means the working tree, and BASE left out means `HEAD`, so a bare `yan review` shows uncommitted
changes. The toolchain reads both trees through `git` and writes nothing to either. The modules are
those `yan test` finds. In CI an agent PR runs `yan review origin/main..HEAD --format md` and
posts the output as a comment or check summary.

Options weighed: two directory arguments (simpler, but every CI caller rebuilds the checkout); a
separate GitHub app (out of scope for M1). Recommended: git revisions.

- Decision: accepted, 2026-11-01
- Row: R665

### D2: item changes

Items are compared by key within each module, on elaborated Yan Core, so a file converted between Yan Agent and
Yan shows no change. An item is *added*, *removed* or *edited*; edited means its canonical Yan Core text
without docstrings differs. An item whose own text is the same but whose identity hash (3.6, R348)
changed is *affected*: a callee changed. Affected items are counted, not listed, and they decide
which tests D5 reports. Each added or edited item prints its header (signature, capabilities,
`fail`, contracts; no body) old and new in Yan. Per-reader spelling (`--as yag|yn`, R526) follows
when the item-level view lands in M3.

- Decision: accepted, 2026-11-01
- Row: R666

### D3: authority diff

For each function, generator and extern block, the report lists capabilities gained and lost (the
capability parameters of the elaborated signature), `fail` types gained and lost, `unsafe` gained or
lost, `pub` gained or lost, and every extern block added, removed or edited. Gains are listed before
everything else. Because the checker enforces `uses` (7.1), the declared set is the item's
authority, and no body needs to be read for it.

- Decision: accepted, 2026-11-01
- Row: R667

### D4: contracts, tests and approvals

The report lists each `pre`, `post`, `ex`, `invariant` and `decreases` line added or removed, each
test function added, edited or removed, and every change to `tests.lock`. Whether a test is approved
is judged by BASE's `tests.lock`, never HEAD's, so a PR cannot approve its own tests. An edit to an
approved test or to a contract clause of an item that has one is marked *frozen*: 21.1 forbids it in
an implement step.

- Decision: accepted, 2026-11-01
- Row: R668

### D5: test verdicts and the verdict cache

`yan test` gets the verdict cache 18.4 already allows. The key is BLAKE3 over the test hash, the
identity hashes of every item the test reaches in its module, and the toolchain hash, which is
BLAKE3 of the `yan` binary. Verdicts are stored in `.yan/cache/verdicts.json`; a test containing
`with` is never cached. `--no-cache` forces a run, and records say `cached: true` when reused.
`yan review` runs HEAD's tests through the cache and lists each test that reaches an added, edited or
affected item, with its verdict, approved or proposed, and cached or run. Tests that reach nothing
changed are counted.

- Decision: accepted, 2026-11-01
- Row: R669

### D6: provenance

When `.yan/provenance.jsonl` (21.5) exists, each listed item carries its author from the record for
its HEAD identity hash: `human`, `model(id, version)` or `unknown`. `yan agent` writing these records
is a follow-up to RFC 0014.

- Decision: accepted, 2026-11-01
- Row: R670

### D7: exit status

Exit 0 when nothing needs a human, 1 when something does, 2 on a tool failure. Needing a human means:
a capability, `fail`, `unsafe` or `pub` gain; an extern block change; a frozen edit; a `tests.lock`
change; or a failing test from D5. Every finding in JSON has a `kind` and an `attention` flag, so CI
can gate on the exit status and a bot can render the rest.

- Decision: accepted, 2026-11-01
- Row: R671

## Spec changes

- 18.1: a `yan review` row, milestone M1.
- 18.4: the verdict-cache key and file of D5.
- 21.5: a paragraph on `yan review`, after `yan audit`.
