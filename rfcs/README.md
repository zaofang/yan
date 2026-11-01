# RFCs

An RFC is one batch of spec decisions: what needs one, and the pipeline, are in
[CONTRIBUTING](../CONTRIBUTING.md#what-needs-an-rfc). Research behind an RFC is listed in
[docs/research](../docs/research/README.md). Decisions made before 2026-11-01 have no RFC file; the
research index links their studies and rows.

## Writing one

1. Copy `0000-template.md` to `NNNN-slug.md`, with the next free number from the index below, and open
   a draft PR. If another PR takes the number first, renumber on rebase.
2. Label each proposed decision D1, D2, and so on. Never write an R number or a diagnostic code into a
   draft.
3. The lead decides each item. Record each answer and its date in the file.
4. In the same PR, take row numbers from `uv run python tools/rows.py next N`, add the accepted items
   to `spec/rows.toml`, edit the spec sections, and run `tools/rows.py render` and `check`. New codes
   take the next free code in their section 25 family.
5. Set the status (accepted, rejected, deferred) and merge. Rejected and deferred RFCs are merged too.

## Index

| RFC | Title | Status | Rows |
| --- | --- | --- | --- |
| [0001](0001-chang.md) | Chang, the third surface | accepted | R516 to R535 |
| [0002](0002-niche-surfaces.md) | Niche surfaces | accepted | R536 to R550 |
| [0003](0003-core-gaps-from-surfaces.md) | Yong gaps found by the surface study | accepted | R551 to R557 |
| [0004](0004-architecture.md) | Architecture for one core and many surfaces | accepted | R558 to R565 |
| [0005](0005-std-json.md) | std.json | accepted | R569 to R575 |
| [0006](0006-engine-layering.md) | Where niche surface engines sit | accepted | R576 |
| [0007](0007-std-encoding.md) | std.encoding and `bytes` | accepted | R579 to R585 |
| [0008](0008-std-path.md) | std.path | accepted | R587 to R592 |
| [0009](0009-std-time.md) | std.time | accepted | R593 to R604 |
| [0010](0010-exact-solver.md) | Exact integer solver and guard-aware matches | accepted | R605, R606 |
| [0011](0011-proof-and-test-power.md) | Proof and test power | accepted | R607 to R609 |
| [0012](0012-header-facts.md) | New facts in the header | accepted | R610 to R612 |
| [0013](0013-numerics.md) | Numerics: accuracy lint, reverse mode, sparse index equations | accepted | R613 to R615 |
| [0014](0014-agent-loop.md) | The reference agent loop | accepted | R616 to R621 |
| [0023](0023-yong-review.md) | `yong review`, a report for agent PRs | draft | |
