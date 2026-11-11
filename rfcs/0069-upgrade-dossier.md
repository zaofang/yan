# 0069: Upgrade dossier: `yan update --dossier`

- Status: draft
- Research: [Yan tooling study: borrow and champion](https://claude.ai/artifact/SAaQdxrqitundhjM7DoBxz),
  champion C4; tracker #1004, under #449.
- Rows: filled in when accepted

## Summary

`yan update` moves locked versions and prints yank reasons and cool-off bypasses (19.4). Whether the
move is safe for this project is left to the reader: `yan audit --diff` (18.1, 19.4) shows capability
and identity changes between lockfiles, and nothing says which changed code the project reaches, whether
a changed item behaves differently at the project's call sites, or which of its tests are affected.
Dependabot reports a version number; Socket reports package metadata. Neither can say what the
project reaches, because neither has content hashes of items, a call graph in a query database and a
verdict cache keyed by reached hashes (R19).

This RFC adds `yan update --dossier`: for the planned move it writes one report with four parts,
(1) authority gained, (2) changed item hashes the project reaches, (3) behavioral divergences (RFC
0068) at the project's call sites, (4) the project's tests whose verdict keys reach a changed hash. It
is a read-only plan: without `--apply` it writes neither `yan.lock` nor the manifest.

At 4fbc37b (2026-11-11) `yan update` exists (`crates/yan-cli/src/deps.rs`) and prints no dossier; `yan
audit`, `yan why` and `yan semver` have no CLI. The dossier composes those, so its parts arrive as
they do. Every part states `unknown` when its input is missing, never neutral.

RFC 0066 D2 fixes the trace container that C4 would read to extend part (3) to items that use
capabilities (R305). This RFC decides nothing about traces.

## Decisions

### D1: what the dossier is, and what it is not

Proposal: `yan update [pkg] --dossier` resolves the update exactly as `yan update` would, with the same
cool-off and yank handling (19.4), then reports on the difference between the current lock and the
resolved one. It is the dry-run form of `update`: no write to `yan.lock`. `--dossier --apply` writes the
lock after printing the report; the lock write rules (R896) are unchanged. `yan audit --diff` stays the
two-lockfile comparison and is called by part (1), not duplicated.

Options: (a) a flag on `update`; (b) a new command `yan dossier`; (c) extend `audit --diff` with
reachability. (b) adds a command whose resolve step duplicates `update` (practice 12). (c) makes an
audit command resolve versions. Recommended (a).

- Decision:
- Row:

### D2: reached changed items

Proposal: for each package that moves, compare its item hashes at the locked and the resolved version.
An item is reported when its reached hash (item hash plus every hash it reaches, the key of R19) differs
and the project's call graph (R267) reaches it from a root of the project. The report lists, per
package, reached changed items, then the count of unreached changed items without listing them. The
same reachability marks an advisory as affecting the build (19.4); it is computed once and shared.

Options: (a) the above; (b) list every changed item of every moved package; (c) report only the
changed `pub` items. (b) buries the answer under code the project never calls. (c) misses a private
helper changed under an unchanged `pub` signature, which is the usual patch release. Recommended (a).

- Decision:
- Row:

### D3: authority gained

Proposal: part (1) is the `audit --diff` result restricted to the moving packages: capabilities gained
or lost per item the project reaches (the `uses` of reached items, not of the whole package), unsafe and
FFI sites gained, publisher identity change (E005) and new approvals required. An update that gains an
unconfined capability or changes identity blocks as 19.4 says; the dossier explains the block, it does
not lift it.

Options: (a) the above; (b) package-level capability totals. (b) reports gains the project's call sites
never reach, which makes a safe update look dangerous and trains readers to skip the page. Recommended
(a), with the package-level total printed on one line for context.

- Decision:
- Row:

### D4: divergences at call sites

Proposal: for each reached changed item whose interface hash is unchanged, run the RFC 0068 check
(base is the locked version, head the resolved one) with the project's own call arguments as the first
inputs, taken from the project's `ex` clauses and tests, then the 0068 generated inputs. A divergence is
printed with its 0068 witness. Items with a non-empty `uses` or `nondet` are `not_comparable`, as in 0068
D3. Items whose interface hash differs are listed with the 20.6 bump the diff implies and are not run.
If `yan semver` is not built, part (3) prints `unknown: behavioral check unavailable` and nothing else
changes.

Options: (a) the above; (b) run only generated inputs; (c) omit the part until traces exist. (b) never
tests the inputs the project actually passes, which are the ones that matter to it. (c) drops the
feature the dossier is for. Recommended (a).

- Decision:
- Row:

### D5: affected tests

Proposal: part (4) lists the project's tests whose verdict key (R19) changes under the resolved lock.
A test whose key is unchanged keeps its cached verdict and is not listed. `--run` runs the listed tests
against the resolved versions in a scratch copy of the lock, then prints pass or fail per test; it does
not write the real lock. Without `--run` no test runs and the part states the count that would.

Options: (a) the above; (b) always run affected tests; (c) list only. (b) makes a planning command
slow and effectful. (c) leaves the reader to run them. Recommended (a).

- Decision:
- Row:

### D6: verdict line and exit status

Proposal: the report opens with one line, `ok`, `review` or `blocked`. `blocked`: the update is refused
by 19.4 (identity change, unconfined capability, malicious mark). `review`: a reached changed item, a
divergence, an affected test or an `unknown` part exists. `ok`: every part was computed and none of the
above holds. `ok` is never printed when any part is `unknown`. Exit status 0 for `ok`, 1 for `review`,
2 for `blocked`, so a bot or script composes it. The verdict is advice, not an approval: nothing here
satisfies an approval gate (19.4).

Options: (a) the above; (b) a numeric risk score. (b) hides which fact decided it and cannot be audited.
Recommended (a).

- Decision:
- Row:

### D7: output

Proposal: text by default, ordered as D1 lists the parts, each part capped at 20 lines with a count of
the rest; `--format json` emits one record per finding (package, item path, locked and resolved hashes,
part, witness if any) and a verdict record, the shape 20.6 uses for findings. Report text from a
package (advisory text, yank reasons) is control-character-stripped and cut at 200 characters as 19.4
already requires. `--pr-body` renders the same data as Markdown for a dependency bot.

Options: (a) the above; (b) JSON only. (b) leaves humans with a tool to pipe. Recommended (a).

- Decision:
- Row:

### D8: determinism and caching

Proposal: the dossier is a pure function of the two locks, the project's hashes and the toolchain
hash, so it is cached in the query database by that key and a repeat run costs a lookup. A registry or
network read happens only in the resolve step, which `--offline` forbids as for `update`. The report
carries no timestamps.

Options: (a) the above; (b) recompute each time. (b) makes a weekly update bot pay for the same check
twice. Recommended (a).

- Decision:
- Row:

### D9: conformance

Proposal: `conformance/update/` gains a fixture registry with one package moving through three
versions: a patch that changes only an unreached item (verdict `ok`), a patch that changes a reached
item with a hidden divergence (`review`, witness expected) and a release that gains an unconfined
capability (`blocked`). The cases are proposed as `conform/*` changes after acceptance; agents do not
write `conformance/` directly.

Options: (a) the above; (b) no fixtures. (b) leaves the false `ok`, the dangerous result, untested.
Recommended (a).

- Decision:
- Row:

## Spec changes

- 18.1: `yan update` row gains `--dossier [--apply] [--run] [--format json] [--pr-body]`, M2 (as `audit`).
- 19.4: dossier paragraph after Identity pinning (D1 to D3, D6); reachability shared with Advisories (D2).
- 20.6: D4 cross-reference to the divergence check.
- 24 (conformance suite): `update/` fixtures (D9).

Implementation order: the reached-hash and call-graph facts the dossier reads (shared with advisory
reachability), part (1) over `audit --diff`, then parts (2) and (4) over verdict keys, then part (3)
once RFC 0068's check exists, then the verdict and output. New code goes in a new module of
`yan-cli` and `yan-pkg`, not in the lowering files of practice 5.
