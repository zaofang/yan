# Yan development laws

Source: study "Yan development laws and phased plans", 2026-11-08.
Rules summarised in [CLAUDE.md](../CLAUDE.md); gate issues are tracked from parent issue #770, linked from #449 and #752.

The order is fixed: 1. get it working, 2. get it right, 3. make it fast. The laws make that order checkable.
They are named DL to keep them apart from the spec's design laws. Status: Gated = a check exists; Partial =
a check exists with a hole; None = prose only, a gate issue exists.

## Laws

| Law | Rule | Gate | Status |
| --- | --- | --- | --- |
| DL1 Working, right, fast | A change reaches stage N+1 only with stage N proven. No optimization PR while its subject has an open interpreter/native diff or a known soundness hole. | PR template with a stage field; perf PRs link a green differential run (needs DL5) | None |
| DL2 Spec first | Behaviour changes go study, RFC, rows, in that order. Code with no row is not done. Spec debt is counted in every progress report and may not grow across two consecutive reports without a lead card. | `tools/rows.py check`; drift gate #755 | Partial |
| DL3 Numbers last | Row numbers and codes are assigned in the final commit after merging main. Drafts, studies and memory carry only D-labels. The rendered block in `spec/appendix-a.md` is never hand-edited. | `rows.py check` and `render` in CI; check that the rendered block equals fresh output | Gated |
| DL4 The lead owns the oracle | Agents never edit `tests/` or `conformance/`. Every bug fix files its reproducing case as a proposed `conform/*` change in the same sitting. | `tools/guard.py` plus CODEOWNERS. Guard keys on PR author (not the lead), Bot type or agent branch prefix; canary and selftest cover a non-`claude/` agent branch | Enforced |
| DL5 One semantics, differential | The interpreter is the oracle. Interpreter, register VM and native agree on every conformance, bench and example program. | Differential harness #753 as a required check | None |
| DL6 One fact, one representation | A fact (ownership, move, span, newtype identity) is computed once and shared. Before patching such a bug, grep every other pass; one issue, one analysis. | Shared analysis #754, spans and identity #756 | None |
| DL7 Fail loud | An unsupported construct yields a numbered diagnostic or a real implementation, never silent acceptance, a fallback or a bare generic error. | Catalogue plus one case per code; fallback grep in review | Partial |
| DL8 New code lands whole | Every new diagnostic lands with its crate symbol, catalogue entry and one case in the same PR set. | Drift gate #755 | None |
| DL9 Definition of done | Works = runs on interpreter and native, both outputs attached. Right = conformance case exists, differential clean, spec row and code agree. Fast = callgrind figure against a recorded baseline. A PR states its stage. | PR template checklist; stage checked by the lead at merge | None |
| DL10 Perf claims | Only callgrind instruction counts, recorded in the PR, against a committed baseline. Wall time is advisory. A Rust-class claim needs a measured native figure against Rust on named programs. | `bench-summary` job; baseline refresh #759 with a regression budget | Partial |
| DL11 Capabilities and determinism are not traded | No optimization or backend may change observable results under deterministic handlers. `unsafe` stays in std and `ffi` modules. | `ub` and semver families; `formal/drt.py`; repeat-run and thread-count matrix in CI | Partial |
| DL12 Frozen names | A frozen name changes only by RFC and a lead card. A retired name may not appear outside an allowlisted archive. | Names lint with a retired-name list | None |
| DL13 Seams | No new code in the three giant lowering files without a split plan; new lowering goes in a new module. | Line-count ratchet per file; split #757 | None |
| DL14 Dated facts | Every number in a report or README carries a commit and date. Progress and coverage figures are CI-generated. Older than 7 days is stale. | `tools/lint_reports.py` in CI: commit and date on report heads and README figures, `conformance/COVERAGE.md` equals fresh output, stale reports listed | Gated |
| DL15 Red CI is classified | Infra signature (HTTP 5xx on toolchain fetch) gets one rerun; a code failure never does. Never skip, quarantine or weaken a test to get green. At most two fix attempts per PR. | Classifier step in `ci.yml`; CI robustness #761 | None |
| DL16 Review and merge | One issue per PR, closed on merge. Lead-owned paths need the lead. Everything else merges only on green CI, as a merge commit; there is no merge on red. A PR that leaves a known hole files it as a numbered gap item. | CODEOWNERS and the `main` ruleset (`.github/rulesets/main.json`: six required checks, no bypass); issue-link check on PR body | Partial |

## Plans

Gates come before features. Targets marked (proposed) are estimates for the lead to accept or change.

### Short term: 2026-11-09 to 2026-11-22

Goal: finish the queue without adding spec debt; turn the top process failures into report-only checks.

1. Finish in-flight queue (#769, #690, #767); each leftover filed as a numbered gap item (DL16).
2. Lead merges PR template (stage field, DoD checklist) and author-based guard (DL1, DL4, DL9).
3. Differential harness, report-only (#753): interpreter, VM and native over all conformance, bench and example programs.
4. Drift gate, report-only (#755); count recorded as the spec-debt baseline.
5. CI failure classifier and one-rerun rule (#761).
6. Bench rebaseline with callgrind figures, native included (#759, #350); no wall-time figure in the baseline.
7. Shared move/drop analysis designed as an RFC (#754); no code yet.

Exit: no new spec rows lacking a drift-report entry; differential and drift reports on every PR; progress report (#351) regenerated with commit and date on every figure; lead decisions taken on the failure-class pass and the data-profile scope card (#663).

### Mid term: to about 2027-02-08

Goal: remove the recurring bug class; make gates blocking.

1. Shared dataflow move/drop analysis replaces name-keyed linearity and textual next-use (#754); old checks deleted.
2. Spans and newtype identity kept until MIR (#756).
3. Split the three giant lowering files; ratchet on (#757, DL13); size cap proposed 3,000 lines.
4. Native parity (#758): differential harness blocking with zero unexplained diffs.
5. Conformance backfill for empty families (semver, ub, pkg) and untested codes (#762); target (proposed) 95 percent of active codes have a case.
6. Drift gate blocking (#755).
7. Failure-class pass, then eval rerun on the same paired protocol (D4 follow-up).
8. M1 exit audit re-run from scratch.
9. Session and protocol hardening (#763).

Exit: every DL gated; no native fix without the interpreter result; giant-file count 0; a two-week window with no renumbering merge and no infra/code misclassification.

### Long term: to about 2027-11

Goal: earn the speed claim, then polish.

1. Capability- and determinism-aware optimizer (#760); each pass has a callgrind gain and a clean differential run.
2. Native against Rust on a named program set (#759); the speed claim is whatever the table says.
3. Release-quality native path (M2).
4. Formal model beyond phase 0 (`formal/`).
5. Package manager crate and registry path (#764).
6. Go public (#461, #462, #423).
7. 1.0 candidate under the spec's evidence-gated-change law.

Exit: every gap issue closed or owned and dated; every figure generated or dated; diagnostics pass; two consecutive releases with no unexplained differential diff and no regression beyond budget.
