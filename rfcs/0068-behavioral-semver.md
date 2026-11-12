# 0068: Behavioral semver

- Status: draft
- Research: Yan tooling study: borrow and champion,
  champion C3; tracker #1004, under #449.
- Rows: filled in when accepted

## Summary

20.6 enforces versions from signatures, `uses`, `fail`, contracts and `ex` results. Its behavioral
check runs only the base version's own `ex` clauses and property tests against the new code. A body
change that keeps the interface hash and every declared example, the table's `patch` row, can still
return different values for inputs nobody wrote an example for, and nothing reports it.
cargo-semver-checks and Elm `bump` are signature-only; no toolchain runs old and new on generated
inputs, because none derives a generator for every type (R301), guarantees the same input gives the
same result (determinism), and hashes items so it knows which ones changed.

This RFC adds a differential check to `yan semver`: for every `pub` item whose reached hash changed
while its interface hash did not, generate inputs, run the base and the head, and report each
divergence as a shrunk, reproducible case. It needs nothing from record and replay (RFC 0066) and
reuses the `tools.difftest` idea (old oracle, new subject, unexplained mismatch is a finding) at the
package-version level.

At 4fbc37b (2026-11-11) no crate implements `yan semver`: `grep -ril semver crates/*/src` finds
nothing. The interface-hash digest of 20.6 is built first, as the spec already requires; this RFC
sits on top of it and adds nothing to that digest.

## Decisions

### D1: which items are compared

Proposal: an item is compared when it is `pub` (directly or by re-export), exists at BASE and HEAD
with equal interface hash, and its reached hash (item hash plus every hash it reaches, the key of the
R19 verdict cache) differs. Items with an unchanged reached hash are skipped, so an unrelated edit
costs nothing, and an item that changed only through a dependency is still compared. Items whose
interface hash differs are already major or minor by the table and are not run.

Options: (a) the above; (b) every `pub` item; (c) only items whose own text changed. (b) spends the
whole budget on code that cannot differ. (c) misses a change that arrives through a callee, which is
the common way a patch release changes behavior. Recommended (a).

- Decision:
- Row:

### D2: input generation and determinism of the check

Proposal: inputs come from the `Arbitrary` generators of R301 for the parameter types, and from the
base version's `ex` clauses and property-test examples as fixed first inputs. The seed derives from
the old reached hash, the new reached hash and the toolchain hash, so the check is a pure function of
its inputs and its verdict is cacheable (R19). The default budget is 100 cases per item, as R301;
`--cases N` overrides. Refinement parameters generate inside their range; a `requires` clause that
rejects an input discards it, with the same 10-times discard limit as `assume`.

Options: (a) the above; (b) a seed chosen per run; (c) enumerate small values exhaustively. (b) makes
a divergence appear and vanish between runs. (c) does not reach large or structured types. Recommended
(a).

- Decision:
- Row:

### D3: what is compared

Proposal: for items with an empty `uses`, the result value by structural equality, the `fail` value,
and the panic kind. Items that are `nondet` or `@unordered` at either revision, and items with a
non-empty `uses`, are reported `not_comparable` with the reason and never as neutral. Comparing
capability requests and replies needs the trace container RFC 0066 D2 fixes; this RFC leaves it to a
follow-up once that container exists, so no format is decided twice.

Options: (a) the above; (b) run `uses` items under `std.sim` handlers now and compare only the result;
(c) wait for traces and compare capability sequences. (b) compares a result produced by a handler
script the new code may no longer follow, so a divergence is ambiguous. (c) leaves the large class of
pure items unchecked meanwhile. Recommended (a), extended to `uses` items after RFC 0066 lands.

- Decision:
- Row:

### D4: what a divergence does to the bump

A confirmed divergence is a changed result for an input that the old version accepted. A body change
that fixes a bug diverges on purpose, and the 20.6 table says such a change is a patch. Blocking every
divergence would turn each bug fix into a major.

Proposal: a divergence is a finding, not a bump. It is listed with its witness (D5), recorded in
release notes and `audit --diff` as `publish --behavior-change` already is, and does not change the
minimum bump. `yan semver --fail-on-divergence` exits 1 on any divergence, for projects that want a
gate. The existing rule is unchanged: a failing `ex` clause or property test of the base still needs
a major or `--behavior-change item`. The new diagnostic is a warning whose code is assigned on
acceptance (practice 3).

Options: (a) the above; (b) require `--behavior-change item` for every divergence; (c) raise the bump
to minor. (b) and (c) contradict the table's patch row and make bug fixes expensive. Recommended (a),
and to keep `publish --behavior-change` as the way to mark an intended divergence so the release notes
say so.

- Decision:
- Row:

### D5: the witness

Proposal: each divergence reports the input shrunk by the R301 passes, the old and new results, and a
generated Yan Agent program in the 20.6 witness form that passes against BASE and fails against HEAD.
The choice sequence is stored under `.yan/examples/` beside the property-test ones, so the next run
replays it first. `--to-test` writes the witness as a proposed `ex` clause on the item as an ordinary
18.3 patch, never applied by the tool.

Options: (a) shrunk input, both results and a witness program; (b) the raw input and results only.
(b) leaves the reader to rebuild the case and breaks the rule that a finding without a witness is
possible, not confirmed. Recommended (a).

- Decision:
- Row:

### D6: running two versions in one process

Proposal: the harness is a generated Yan Agent program that imports the base version as a second
package and the head as the first, compares the two items and runs in the checked interpreter. Item
hashes make the two copies distinct identities, so they coexist without renaming. `--base v` selects
the version as for the rest of `yan semver`; the default is the last published one. A BASE that does
not resolve (offline, yanked) makes the check `unknown`, not neutral.

Whether `yan-pkg` can load two versions of one package into one query database is not verified here,
and is the first thing the implementation settles. If it cannot, the fallback is two processes
exchanging the generated inputs and results as Wire values, with the same report.

Options: (a) one process, two package identities; (b) two processes, Wire exchange; (c) rebuild the
head against the base's tests only, as now. (c) is the check this RFC extends. Recommended (a) with (b)
as the fallback.

- Decision:
- Row:

### D7: consumers and conformance

C4 (upgrade dossier) needs the divergences at a dependent's call sites, so `--format json` carries
the item pair of hashes, the witness and the verdict per item, in the shape 20.6 uses for findings. A
dependent's call site is found from the call-graph facts, not from this check.

Proposal: `conformance/semver/` gains, for the table's `patch` row, one pair
with a hidden divergence and one without, with the expected finding. The cases are proposed as
`conform/*` changes after acceptance; agents do not write `conformance/` directly. Cost: the check
runs inside the release train's existing `yan semver` pass on std (20.7).

Options: (a) the above; (b) no conformance pair. (b) leaves the check untested where it matters, the
false negative. Recommended (a).

- Decision:
- Row:

## Spec changes

- 20.6: Behavioral check restated as two parts (declared examples as now; generated inputs, D1 to
  D3); a divergence paragraph (D4); witness (D5) added to Witnesses.
- 18.1: `yan semver` row gains `--cases` and `--fail-on-divergence`, M2 (as the row now).
- 25 (catalogue): the new warning, once numbered.
- 24 (conformance suite): `semver/` gains the D7 pairs.

Implementation order: the interface-hash digest and findings of 20.6 first, as the spec requires;
then reached-hash selection (D1), generation and the cached verdict (D2), the comparison (D3) and
report (D4, D5), then the two-version harness (D6) if it forces the fallback. New code goes in a new
module in `yan-pkg`, not in the lowering files of practice 5.
