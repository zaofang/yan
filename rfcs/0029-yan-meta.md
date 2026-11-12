# 0029: Yan Meta, the lab and canary surface

- Status: accepted (the lead accepted every decision below on 2026-11-03)
- Research: Yan Notation: names and real features
- Rows: R776 to R786

## Summary

The third surface (RFC 0001, section 16.6) was named Yan Notation 言符 (`.ynt`) by R774. The lead
renamed it Yan Meta 言元 (`.ymt`) and gave it a charter: the lab, the kitchen sink and the canary of
the language, admitting every language feature the study lists (its sections 2, 5 and 6, both
scenarios) and no tooling. This RFC records the name (D1), a lifecycle that keeps a lab surface from
becoming a pile (D2), the new forms that need no core change, which join RFC 0001's reference (D3),
and the features that need a core change, which join the research track (D4). Nothing here changes
the timing: no Yan Meta build before M1 (R516, R563).

## Decisions

### D1: Yan Meta 言元, `.ymt`

Yan Meta (言元; 元语言, "metalanguage") replaces Yan Notation 言符 in R774's third-surface clause.
Files use `.ymt`. Code identifiers follow the extension, as R774 does for the other surfaces:
the print-only view is `yan view --as ymt`, the planned crate is `yan-ymt`, the K4 printer is
`ymt(elaborate(F), spelling(F))`, and RFC 0001's diagnostic slugs are `ymt-...`. RFC 0001 moves to
`rfcs/0001-yan-meta.md`.

Options weighed in the study: keep Yan Notation (honest if the job is array spelling), Yan Lab 言验
`.ylb` (fits the canary job), Yan Meta 言元 `.ymt` (fits a surface that owns metaprogramming and
feature trials). `.ynml` was rejected because `-ml` reads as markup. The brand kit needs a folded 元
seal in place of the 符 seal; that is brand work, outside the spec.

- Decision: accepted, 2026-11-03 (lead: "Adopt Yan Meta .ymt")
- Row: R776

### D2: Charter and canary lifecycle

Yan Meta is where Yan and Yan Core try syntax and features before committing to them. It keeps K1 to
K5 (R517) and law 1: a Yan Meta form adds syntax over Yan Core or over an accepted extension, never
semantics of its own. What the charter adds is a lifecycle, so that trials end.

1. **Stages.** Every RFC 0001 entry carries a stage: `experimental`, `stable`, `graduated` or
   `retired`. The forms accepted on 2026-11-01 (RFC 0001 sections 9 and 10) start `stable`. A new
   form enters `experimental`.
2. **Opt-in per module.** A module that uses an experimental form names it in a header line,
   `@lab(trains, defer)`. Use without the line is E001, which already covers previews (20.6); the
   message names the form and the line to add. `@lab` is a spelling note (16.5), so it is
   hash-neutral. An experimental form is a preview in the sense of 20.6: outside the compatibility
   promise, and a published package at 1.0 or later may not contain a `@lab` line (E002).
3. **Promotion to stable.** By a row the lead accepts, with this evidence: the form appears in the
   std or examples corpus, and law 6 of 16.5 holds for it over the whole suite with and without
   spelling notes.
4. **Graduation.** A stable form graduates when Yan (15) or Yan Agent (13, 14) adopts its spelling,
   or when the extension behind it lands in core for every surface. Evidence, beyond step 3: a probe
   on the R566 harness shows models read and write it, and a reviewer reads its `yan view --as yn`
   print without the note. Yan Meta keeps reading a graduated form.
5. **Retirement.** A retired form follows the preview window of 20.6: it is withdrawn within four
   minors. Until then it reads as before and `yan fmt` rewrites each use to the default spelling of
   the same tree, so no item hash changes.

Options weighed: no lifecycle (RFC 0001 as is; open admission grows without exit); a separate
`--lab` build flag (a second language mode, which law 1 and one release channel rule out); a new
diagnostic family (E001 and E002 already say exactly this for previews). Recommendation: the five
steps above.

- Decision: accepted, 2026-11-03 (card "Accept RFC 0029's D2 to D4 details as written")
- Row: R777 (charter, stages, promotion, graduation, retirement), R778 (`@lab` opt-in, E001, E002)

### D3: Features that need no core change join RFC 0001 as experimental

Each lowers to Yan Core today, under K1 to K5. They are written into RFC 0001 section 10 as entries
with stage `experimental` (section "Yan Meta additions"). Study features already in RFC 0001 are not
repeated: trains, reductions, scans, inner product, `ein` literals, unit literals, pipelines,
`defer`, for-else, shadowing, file-local fixity, active and regex patterns, trailing `do` blocks,
interpolation, stepped ranges, comprehensions, refinements on signatures, Datalog rules, doctests,
actors, channels, typed holes.

| Entry | Form | Lowers to |
| --- | --- | --- |
| O6 | Index notation: stencils, non-sum reductions, scatter (Tullio) | a loop nest with inferred ranges; unproven bounds become checks (8.6) |
| O7 | Static-rank lifting at a call site, `f each x` | an inline `map` or `for` at the site, no new item |
| L7 | Literal polymorphism: `1` takes the element type from context | an explicit typed literal; the spelling note keeps `1` |
| L8 | `fmt"..."`, `json"..."` prefixed literals | stage-0 `@literal` calls, as `re` and `sql` |
| B11 | Record punning `.{x=, y=}` | `.{x: x, y: y}` |
| F19 | Default arguments `step: int = 1` | a typed `@defaults` annotation on the item; each call site passes the value |
| F20 | Mixfix operators, `mixfix "[|" _ "|]" := f` | a call to `f`; the form is kept in the name table |
| P6 | View patterns `(f -> P)` | a call and a nested `match` |
| P7 | Pattern synonyms `pattern Origin := .{x: 0, y: 0}` | a P2 extractor plus a constructor function, both named by the author |
| C15 | `opt` and `res` blocks (do-notation for `Option` and `Result`) | nested matches, one per `?` in the block |
| C16 | Labelled breaks with a value, `break @outer e` | a join point with one parameter |
| C17 | `guard c else e`, with `e` diverging | `if not c` then `e` |
| X8 | `assume e` and `assert` blocks | checked `assert`s; the solver uses the fact after the check, nothing is trusted |
| T14 | Refinement aliases `type Pos := int{_ > 0}` | the T1 contracts at each signature that uses the alias |
| S1 | Compact stage 0: `#for f in fields(T)`, field access `x.#f`, `#record{...}`, `#sum{...}` | `comptime` loops, reflection and the reifiers of 10.4 |
| S2 | Derive and attribute authoring, `derive Show for T := ...`, `attr name(d) := ...` | `derived` bodies and additive `@attribute` functions (10.4) |
| S3 | Relational rules with `fresh` and `==` unification (miniKanren style) | a stage-0 DSL over `Table`, as M1 |
| K11 | Reactive cells `cell x := e` | the `Cell[T]` library (R528), when it exists |
| K12 | Protocol spelling for multiparty session types | the `protocol` item (RFC 0018) |

Out, as the lead decided: tooling (notebook cells, typed-hole filling beyond X4, replay, semantic
diff, per-reader view). Also out: probabilistic models beyond RFC 0001's M4 library form, which the
planned prob surface owns, and multi-shot continuations (RFC 0001 section 11).

- Decision: accepted, 2026-11-03 (lead: "all language features, no tooling"; card as D2)
- Row: R779

### D4: Features that need a core change join the research track (26.6)

These amend Yan Core, so every surface gets them once adopted (R768). Each is a research-track row:
not normative for 1.0, adoption by its own RFC. Yan Meta spells each one first, as an experimental
form that K1 rejects with RFC 0001's needs-extension slug until the extension lands. A research
extension is never switched on early for Yan Meta alone: that would give one surface semantics the
core lacks (law 1).

Already on the research track, now listed as Yan Meta's core-change set: R522 (one-shot handlers),
R523 (information-flow labels), R524 (graded types, which ghost code needs), R525 (typed staging and
macros), R527 (user grammars), R530 (positional type parameters), R531 (`Sym`), R532 (`Any`), R533
(panic recovery), R534 (STM), R535 (`eval`). New rows:

1. **Rank polymorphism.** A shape-row kind: `Array[...B, n, T]` with `...B` an axis-list variable,
   and a lifting rule for functions written over cells (Remora, Dex). Amends 3 and 11.2.
2. **Size arithmetic.** Linear (Presburger) constraints over `Nat` size variables, so `concat` gives
   `n + m` and stencils carry `n - 1`; goals left undecided follow the 8.6 rule. Amends 11.2.
3. **Body reflection.** `info(f)` at stage 0 returns a read-only view of a function body; a stage-0
   query hashes every body it reads. Amends 10.4 (reflection sees signatures only).
4. **Rewriting attributes.** An `@attribute` function may wrap or replace the body it annotates;
   `yan expand` prints the result for review. Amends 10.4 and E266 (additive only).
5. **Proof tactics.** Stage-0 scripts that discharge 8.6 goals through goal reflection and a tactic
   API (LCF). Amends none.
6. **Row-polymorphic and anonymous records.** Record types with a row variable, `.{x: int | r}`, and
   anonymous record types outside stage 0. Amends 3.
7. **GADTs.** Sum constructors that refine the sum's type parameters. Amends 3.

- Decision: accepted, 2026-11-03 (card as D2)
- Row: R780 to R786

## Spec changes

- 1, 2, 16.5, 16.6, 22, 23, 26.1, 26.2, README: Yan Notation becomes Yan Meta, `.ynt` becomes
  `.ymt`, `chang` identifiers become `ymt` (D1).
- 16.6: the charter and lifecycle (D2) and a pointer to the new entries (D3).
- 20.6: experimental Yan Meta forms are previews (D2).
- 25: E001 covers experimental Yan Meta forms (D2).
- 26.6: R780 to R786 (D4).
- RFC 0001: renamed, stages on every entry, section "Yan Meta additions" (D3).
