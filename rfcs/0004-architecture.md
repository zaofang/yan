# 0004: Architecture for one core and many surfaces

- Status: accepted (the lead accepted D1 to D7, and Q1 as D8, on 2026-11-01)
- Research: [Yong architecture plan: one core, many surfaces](https://claude.ai/artifact/NpTvDsHWcBnPtCmWvLtqFC)
- Rows: R558 to R565

## Summary

Yong now has three accepted surfaces (Jian, Wen, Fei) and thirteen planned niche surfaces (RFC 0002),
but the implementation was laid out for one surface. HIR follows Jian's grammar, printers only print
their own syntax trees, nothing computes the item hash of 3.6, `yong-elab` depends on `yong-jian` to
load std, and std natives live inside the interpreter. This RFC makes the shared HIR the one pivot
every surface talks to, makes every step between text and Yong reversible, and fixes the crate
boundaries, std layout and law checking that follow. The plan behind it, with the work order, is the
research study linked above.

## Decisions

### D1: HIR is the pivot

Every surface crate (Jian, Wen, Fei and each niche surface) lowers to `yong-hir` and resugars from it,
and depends on nothing below it. HIR is surface-neutral: a form is in HIR only when two or more
surfaces spell it; other sugar stays in the surface's own AST and is recorded as a spelling note. This
extends admission rule N6 (R536) to the three main surfaces.

Options weighed: keep growing HIR as the union of every surface's forms (cheap now, unworkable with
sixteen surfaces); let surfaces lower straight to Yong (each would reimplement inference, capability
passing and copy insertion). Recommended: the pivot.

- Decision: accepted, 2026-11-01
- Row: R558

### D2: One printer per surface, one shared raise

Each surface has exactly one canonical printer, over its own AST, used by both `yong fmt` and
`yong view`. Raise, from checked Yong back to HIR, is written once in `yong-hir`: it restores written
names from the name table and removes what elaboration inserted. `fmt F` is
print(abstract(parse(F))); `view --as S X` is print(resugar(raise(elaborate(X)))). The round-trip laws
of 16.5 and N7 are statements about these compositions.

- Decision: accepted, 2026-11-01
- Row: R559

### D3: Crate boundaries

Add to the 22.2 table:

- `yong-names`: word-list names (16.1), foreign names (16.2), the projection table (16.3).
- `yong-surface`: the `Surface` trait (parse, abstract, lower, subset, resugar, print, optional
  lenient reader, grammar export, message rendering), the trivia and spelling-note side table, and
  the generic law harness.
- `yong-fei`: the Fei surface.
- `yong-mir`: the mid-level IR between `yong-opt` and the code generators (per-instance control-flow
  graph, explicit drops, intrinsic ids). The interpreter stays on checked Yong.
- `yong-ide`: the analysis API shared by `yong-cli`, `yong-mcp` and `yong-lsp`.

`yong-db` loads std; `yong-elab` depends on no surface crate. Dependencies run one way, by layer:
base, surfaces, pivot, core, lowering, backends, runtime, tools.

- Decision: accepted, 2026-11-01
- Row: R560

### D4: One intrinsic registry

Each std intrinsic has an entry in `yong-std` (id, signature, stage, purity) and is resolved to its id
at elaboration. The interpreter and every code generator bind the same ids to the same
implementations in `yong-rt`: a direct call from the interpreter, a C-ABI symbol or inline lowering
from native code. Name-based dispatch goes away, and the M1b differential tests compare one std
implementation on both sides.

- Decision: accepted, 2026-11-01
- Row: R561

### D5: Std sources in tiers and in two surfaces

`yong-std/lib/` has `core/`, `alloc/` and `std/` directories matching the 17.1 tiers; module paths do
not change, and the checker rejects an import from a higher tier. A module's public API is written in
Jian when it is inside the Jian subset; bodies that need L4 to L6 (`unsafe`, `Ptr`, `RawCell`, extern
blocks) go in a `.wen` file of the same module. 22.2 now says std sources are `.jian` only, which
cannot express those bodies.

- Decision: accepted, 2026-11-01
- Row: R562

### D6: Fei print-only view in M3

`yong view --as fei` moves from M4 to M3, beside the Wen printer, with every Yong item printing as Fei
with no spelling notes (law 6, second half). The Fei reader, lenient reader and spelling-note round
trip stay in M4. The print-only view is the first test of the surface kit on a surface that is not
Jian or Wen, and it is how R550 already ships niche surfaces.

- Decision: accepted, 2026-11-01
- Row: R563

### D7: Law checking over the whole suite

`tools/conform.py --laws` runs every applicable law of 16.5 (and N7 for niche surfaces) on every
conformance program, with no new case files. CI reports it once printers from Yong exist and gates on
it at M3. `conformance/roundtrip/` stays for cases the lead writes to pin hard spellings.

- Decision: accepted, 2026-11-01
- Row: R564

### D8: Origin marks (was Q1)

Raise must tell an inserted copy, move, capability argument or conversion from one the author wrote.
Each node elaboration inserts carries an origin mark, a sixth kind of hash-neutral trivia in 16.5.
For Yong text with no marks, raise removes a node of an inserted kind only if elaborating the result
inserts it again at the same place.

- Decision: accepted, 2026-11-01 (option "Accept all, plus Q1 as D8")
- Row: R565

## Open questions

- Q2. When do golden item hashes enter `conformance/`? The lead's call.

## Spec changes

- 1: the system overview diagram (D1, D2).
- 16.4, 16.5: projection as compositions; law checking; origin marks and raise (D2, D7, D8).
- 16.6, 23: Fei print-only view in M3 (D6).
- 17.1: tier directories, std sources in Jian and Wen (D5).
- 22.2, 22.3, 22.4: crates, pivot, intrinsic registry (D1, D3, D4, D5).
- 24.2: the `--laws` mode (D7).
