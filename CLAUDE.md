# Working on Yan

## Order of work

1. Get it working, 2. get it right, 3. make it fast. Never skip a stage and never start the next while
the previous is red. A PR states which stage it claims.

- Working: runs on interpreter and native; both outputs attached.
- Right: conformance case proposed via `conform/*`, differential run clean (`uv run python -m tools.difftest`; divergences go in `tools/difftest_allow.toml` with an issue), spec rows and code agree.
- Fast: callgrind figure against the committed baseline, recorded in the PR.

Laws and gates: [docs/development-laws.md](docs/development-laws.md).

## Changing the spec

SPEC FREEZE (lead decision 2026-11-12, #1025): no new spec rows or RFCs, except to fix a contradiction or
unblock M1, until rows without a case are at or below 60 percent (`tools.status` spec_debt; 74 percent,
650 of 879, on 2026-11-12). RFCs 0068-0075 (#1004) are paused. The lead lifts the freeze by card.

The spec is `spec/` (one file per top-level section). It changes only for rows the lead accepted, in a
PR the lead merges.

1. Research goes in a study (a Claude Doc artifact), listed the same day in `docs/research/README.md`.
2. A proposal is `rfcs/NNNN-slug.md` from `rfcs/0000-template.md`. Label items D1, D2. Never put an R
   number or a diagnostic code in a draft, a study or project memory.
3. After the lead decides, in the same PR: `uv run python tools/rows.py next N` for row numbers,
   `tools/rows.py next-code E3` (family) for codes, add the rows to `spec/rows.toml`, edit only the
   sections they change, then `tools/rows.py render` and `tools/rows.py check`.
4. If another PR took your numbers first, CI fails or the merge conflicts: merge main and renumber.

Never hand-edit the block between the `rows:` markers in `spec/appendix-a.md`.

## Paths

Agents never edit `tests/` or `conformance/`; propose changes in a separate PR. CI's path guard
(`tools/guard.py`) enforces this for every PR whose author is not the lead, whatever the branch name.

## Practices

1. Backend PRs: run the new program on interpreter and native, attach both outputs. A native feature is not done without a matching interpreter result.
2. Bug fixes: file the reproducing case as a proposed `conform/*` change in the same sitting.
3. Every new E/W code lands with its crate symbol, catalogue entry and one case in the same PR set.
4. Before patching an ownership or span bug, grep the other passes for the same pattern. One issue, one analysis.
5. No new code in `yan-elab/src/lower.rs`, `yan-hir/src/raise.rs` or `yan-mir/src/lower.rs` without a split plan; put new lowering in a new module.
6. Perf claims use callgrind instruction counts, not wall time; record the figure in the PR.
7. Classify a red CI before retrying: infra signature (HTTP 5xx on elan/toolchain fetch) gets one rerun, a code failure never does. Max 2 CI-fix attempts per PR.
8. Number rows and codes last, as the final commit after merging main.
9. A PR that leaves a known hole files it as a numbered item in a gap issue (as #690, #709).
10. Mac threads get the lead's standing merge approval message attached by id.
11. When the queue ends, refresh the progress report (#351) and bench baseline (#350).
12. One fact, one representation: ownership, moves, spans and newtype identity are computed once and
    shared. A fix that adds a second representation is rejected.
13. Fail loud: an unsupported construct gets a diagnostic or a real implementation, never a silent
    fallback or a bare generic error.
14. Every figure in a report or README carries a commit and a date; progress and coverage figures are
    generated, not typed. Stale means older than 7 days.
15. A frozen name changes only by RFC and a lead card. Retired names do not appear outside the archive.
16. Spec debt (rows with no implementation or case) is reported in each progress report and must not grow
    across two consecutive reports without a lead card.
