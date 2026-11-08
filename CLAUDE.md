# Working on Yan

## Changing the spec

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
(`tools/guard.py`) enforces this.

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
