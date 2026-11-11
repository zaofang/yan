# 0067: Semantic no-op certification in `yan review`

- Status: draft
- Research: [Yan tooling study: borrow and champion](https://claude.ai/artifact/SAaQdxrqitundhjM7DoBxz),
  champion C2; tracker #1004, under #449.
- Rows: filled in when accepted

## Summary

`yan review` (21.5, R665 to R671) already compares items on elaborated Yan Core, so a reformat, a
comment or a docstring does not show as an edit. It does not say so: a change with no behavior change
produces an empty finding list, which a reader cannot tell from a review that looked at nothing. This
RFC makes the review state a verdict, `behavior: neutral` or `changed`, and account for every changed
path, so a 400-line agent refactor reviews as "0 behavior changes" and a human reads nothing.
difftastic and similar tools compare syntax trees; none can certify that two revisions mean the same
program, because none has a canonical, content-hashed core form to compare.

The lead's build order (study D1, 2026-11-11) puts this second, after record, replay and trace diff
(RFC 0066). It needs nothing from that work.

At 4fbc37b (2026-11-11) three gaps stand between the current review and a certificate:

1. `crates/yan-cli/src/review.rs:449` compares items by printed Yan Core text with written local
   names. R519 says renaming a local never changes an item hash, but the review lists it as an edit.
   The review and the hash compute item sameness twice, and disagree (practice 12).
2. An item renamed or moved between modules is reported as one item removed and one added, and every
   caller as edited.
3. The review reads only module files and `tests.lock`. A change to `yan.toml`, `yan.lock`, a guide
   page or a data file the package reads is not reported at all (practice 13).

## Decisions

### D1: what "unchanged" means

Proposal: an item's *own text* is its canonical text from 3.6 before references are replaced by
hashes: locals named by binding order (R519), docstrings and the name table dropped as trivia (16.5),
and references to other items kept as paths. Edited means the own text differs. Affected (R666) means
the own text is equal and the identity hash differs, so the change came from a dependency. `yan-core`
computes the own text once, beside the identity hash it already derives from it, and the review reads
it instead of printing its own.

Options: (a) the own text; (b) printed text with written names, as now; (c) the identity hash alone.
(b) reports local renames as edits and keeps a second definition of sameness beside the hash. (c)
reports every caller of an edited item as edited, which is what R666's "affected" exists to separate.
Recommended (a).

- Decision:
- Row:

### D2: renames and moves

Proposal: pair a removed item with an added item when their own texts are equal once the item's own
name, and every reference to a removed or added item, is blanked. A pair is kept only when the match
is one to one; it is then confirmed by comparing the texts with references rewritten under the full
set of pairs, the same blank-then-confirm scheme 3.6 uses to order recursive groups. A confirmed pair
is reported as `renamed` (same module) or `moved`, with the old and new keys and no body. An item
whose own text differs only by references rewritten under the pairs is `reference_renamed`: counted,
not listed, as affected items are.

A private rename or move is neutral. A `pub` item renamed or moved changes the package's interface and
sets `attention`, as R667 does for `pub` lost.

Options: (a) exact pairing as above; (b) leave renames as a removal and an addition; (c) pair by a
similarity score, as `git diff -M` does. (b) makes the most common agent refactor the most expensive
to review. (c) is a guess, and a certificate cannot rest on a threshold. Recommended (a).

- Decision:
- Row:

### D3: the verdict and its reasons

Proposal: the review opens with one verdict.

- **neutral**: every changed path is classified (D4), and no item is added, removed or edited, apart
  from D2 private renames and moves.
- **changed**: any item is added, removed or edited, or any `pub` item renamed or moved. Affected
  items follow from an edit, so they never decide the verdict alone.
- **unknown**: a changed path is unclassified (D4), or a module does not elaborate at BASE or HEAD.

Each changed module file gets the reasons its items stayed equal, from what differs between the two
revisions: `format` (source text only), `docs` (doc trivia, R310), `names` (the R519 name table),
`rename` (D2), `surface` (the file moved between `.yag` and `.yn`, which R666 already treats as no
change). A file can have several. The line form starts
`behavior: neutral (14 files; format 9, docs 3, names 1, rename 1)` or
`behavior: changed (3 items edited, 7 affected)`, lists neutral files one per line with their
reasons, then the findings as now. `--format json` adds a `behavior` record (verdict, counts, files
with reasons, unclassified paths) and `--format md` a heading line.

Options: (a) verdict, reasons and file list; (b) a count of changed items only. (b) leaves the reader
to infer that an empty list means "looked and found nothing" rather than "did not look".
Recommended (a).

- Decision:
- Row:

### D4: every changed path is accounted for

Proposal: the review lists the changed paths under the package root from git and classifies each:
module files (by item, D1 to D3), `tests.lock` (R668), `.yan/provenance.jsonl` (metadata, neutral).
Any other changed path, including `yan.toml`, `yan.lock`, `docs/*.md` (their fences run as tests,
R316) and files the package reads, is listed as `path_unclassified` and makes the verdict unknown. It
does not set `attention` by itself, so R671's exit status is unchanged. Paths outside the package
root are out of scope, and the report names the root it covered.

Options: (a) the above; (b) ignore files the review cannot read, as now; (c) classify `yan.toml` and
`yan.lock` semantically now. (b) certifies a dependency upgrade as neutral. (c) needs references
across packages hashed, which `crates/yan-core/src/hash.rs` leaves open (its SPEC-Q keeps such paths
as written). Recommended (a), with (c) filed as a numbered item in a gap issue for when cross-package
hashing lands.

- Decision:
- Row:

### D5: a gate for changes that claim no behavior change

Proposal: `yan review --neutral [BASE[..HEAD]]` exits 0 when the verdict is neutral, 1 when it is
changed or unknown, and 2 on a tool failure, independent of `attention`. It is for refactor PRs, `yan
fmt` and `yan fix` commits, and agent patches declared as no-ops.

Options: (a) a flag on `yan review`; (b) a separate `yan certify` command; (c) no gate, read the JSON.
(b) is a second comparison engine (practice 12). (c) makes every caller re-derive the verdict.
Recommended (a).

- Decision:
- Row:

### D6: the verdict is a report, not an attestation

A verdict could be signed (19.4) and carried with a patch, so a reviewer trusts it without rerunning.
That is champion C7's subject (proof-carrying patches), which also covers verdict keys.

Options: (a) a report only here, signing left to C7; (b) sign the verdict now. (b) fixes an
attestation format before C7 weighs it against verdict keys. Recommended (a).

- Decision:
- Row:

### D7: tools that must certify neutral

With D1 to D4, "this tool does not change behavior" becomes checkable. Proposal: for every
conformance program, `yan review --neutral` MUST pass between the program and each of: its `yan fmt`
output, its `yan view --write` conversion to the other surface, and a copy with every local renamed
(R519). The cases are proposed as `conform/*` changes after acceptance; agents do not write them into
`tests/` or `conformance/` directly.

Options: (a) the law above; (b) no law. (a) turns a formatter or converter bug that changes meaning
into a red conformance run. Recommended (a).

- Decision:
- Row:

## Spec changes

- 3.6: one sentence defining the own text (D1), beside the identity hash.
- 21.5: R666 restated on the own text (D1); D2 renames and moves; D3 verdict and reasons; D4 path
  accounting; D6 one sentence.
- 18.1: `yan review` row gains `--neutral` (D5), M1.
- 24 (conformance suite): D7.

Implementation order: own text in `yan-core` (D1) and the review switched to it, then path accounting
(D4), then the verdict and reasons (D3) and `--neutral` (D5), then rename pairing (D2). New code goes
in a new module beside `crates/yan-cli/src/review.rs`, not in the lowering files of practice 5.
