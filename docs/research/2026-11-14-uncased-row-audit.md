# Uncased spec row audit, 2026-11-14

Issue #1025 (spec freeze gate), tracker #1136 (P3). Evidence at `2d72926`, 2026-11-14, from `tools.status` spec_debt logic: 634 of 879 R rows have no conformance case. The gate needs 527 or fewer (60 percent of 879). Antonio chose "audit rows first, then sweep". This study changes no spec row and no threshold; it proposes treatments for a lead card.

Regenerate the figures and the per-row CSV (`2026-11-14-uncased-row-audit.csv`) with `uv run python -m tools.uncased_audit`; do not edit them by hand.

## Method

Each uncased row was judged by one question: can a Yan program, run through `yan`, ever produce an output that a conformance case could assert? Rows were grouped by the topic text in `spec/rows.toml` and, for the groups below, by the row's body text in `spec/`. Only groups where the answer is clearly no (exempt), clearly a duplicate or part of another row (merge), or not normative for 1.0 (defer) are listed. Everything else stays uncased and needs a real case (about 503 rows); that includes bundle rows R898 to R907, which are observable but each need several cases, and mixed entries such as R467 to R473 and R474 to R476.

## Candidates

| Group | Rows | Reason | Proposed treatment |
|---|---|---|---|
| Licensing, governance, trademark (26.5) | 16 | Legal and process rules about the project, not program behaviour. R356 (license_policy enforced by yan check) stays, it is observable. | Exempt: checked by repo files (LICENSE, CODE_OF_CONDUCT, DCO bot), not by conformance. |
| Names, marks, release scope, design bases | 8 | Naming, trademark, release-scope and probe-timing decisions; no program output depends on them. | Exempt: tools/names_lint.py and the roadmap carry them. |
| Toolchain-internal structure | 14 | Crate layering, registry generation, suite layout, unsafe confinement, yan-flow placement, egglog backend choice. Checked by repo tooling (cargo, lint scripts, tools/conform.py), never by a Yan program. | Exempt: a CI lint or cargo test is the evidence. |
| Implementation language and self-hosting | 5 | Which language the compiler is written in, S0 to S5 stages and the LLVM choice. Tracked by #851, gated by byte-equality, not by a conformance case. | Exempt: self-host stage gates (#852) are the evidence. |
| Panic payload internals | 4 | Payload fields (RFC 0031, 0042) live in the record-and-replay buffer; a program sees only the terminal message, which omits them (spec 8.3). | Exempt unless the lead wants a replay-trace case (tool-observable, not program-observable). |
| Origin and spelling marks (RFC 0034) | 29 | HIR trivia marks. Not observable alone; their only contract is the raise round trip, already stated by N7 and law 6 (R996). | Merge into R996 as clauses; one round-trip case family cites R996. |
| Superseded implementation language | 1 | Replaced by R966 (spec says so). | Retire R215, point at R966. |
| Research track and niche surfaces, post-1.0 | 48 | Spec marks them not normative for 1.0 or post-1.0. | Defer: count after 1.0, or exclude from the 1.0 gate by lead card. |
| Yan Meta, M4 | 6 | Third surface built at M4 (R516). | Defer to M4. |

Totals: exempt 47, merge 30, defer 54.

## Projected uncased count

Two readings of "exempt" give different ratios. Removing a row from the denominator as well as the numerator moves the ratio less than counting it as cased, so both are shown.

| Treatment | Uncased | Percent | At or below 60% |
|---|---|---|---|
| Today | 634 of 879 | 72.1% | no |
| Exempt, rows leave numerator only | 587 of 879 | 66.8% | no |
| Exempt, rows leave both | 587 of 832 | 70.6% | no |
| Exempt + merge, numerator only | 557 of 879 | 63.4% | no |
| Exempt + merge, both | 557 of 802 | 69.5% | no |
| Exempt + merge + defer, numerator only | 503 of 879 | 57.2% | yes |
| Exempt + merge + defer, both | 503 of 748 | 67.2% | no |

Reading: exemption and merge alone (77 rows) do not reach the gate under either reading. Reaching 527 needs 107 fewer uncased rows if exempt rows count as cased. The remainder must come from cases (the sweep) or from deferring the post-1.0 rows by lead card.

## Caveats

- Panic payload rows (R796, R883 to R885) are tool-observable through `yan replay`; exempting them is a judgement call.
- R5 and R8 to R22 (old process and tooling rows) were not judged; their bodies are not in the spec text and need a read before any decision.
- Exempting rows changes what "spec debt" means; practice 16 forbids growth across reports, so the progress report must state the rule used.
- Merging rows renumbers nothing; it removes ids, which needs a spec PR the lead merges, after the freeze rules allow it (contradiction fix or lead card).
