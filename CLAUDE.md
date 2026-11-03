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
