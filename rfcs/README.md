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
