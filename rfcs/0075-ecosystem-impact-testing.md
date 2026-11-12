# 0075: Ecosystem impact testing: rerun only the dependents' tests a release changes

- Status: draft
- Research: Yan tooling study: borrow and champion,
  champion C10; tracker #1004, under #449.
- Rows: filled in when accepted

## Summary

Before a library ships a release, its author wants to know what the release does to the packages that
depend on it. Rust answers with crater: build and test every crate in the registry against the change,
which takes days of cluster time per run and is run only for the compiler. Everything else answers with a
semver check, which reads signatures and says nothing about the dependents' own code (20.6).

Yan has what crater lacks. A pure test is a function of the hashes of every item it reaches (18.4), so a
dependent's test whose key does not change under the new release has the same verdict as before and need
not run. The registry index lists versions, requirements and archive digests (19.3), archives are
deterministic and hermetic (no install or build scripts), and a test that binds no real capability cannot
touch the machine. So a release can be tried against dependents cheaply and safely: substitute the
candidate, recompute each dependent test's key, run only the tests whose key changed.

This RFC adds `yan publish --impact`, a read-only report that does this and prints which dependents'
tests newly fail, which are unchanged by key, and which could not be judged. It feeds the author's
decision to bump a major or use `--behavior-change`, and does not decide it.

At 94f1c1a (2026-11-11) `yan publish` has only `--check` (`crates/yan-cli/src/publish.rs`, the manifest
refusals), the registry client reads `list`, `.info` and archives (`crates/yan-cli/src/registry.rs`), the
verdict cache exists (`crates/yan-cli/src/test.rs`), and `yan semver` and `yan why` have no CLI. The
registry (M2b) and its dependents list do not exist; D2 says what runs before it. The study marks this
champion `L` and speculative for that reason.

This RFC does not read traces and does not cite the trace container of RFC 0066: the inputs are item
hashes, verdict keys and the index.

## Decisions

### D1: command surface

Proposal: `yan publish --impact [--dependents SRC] [--max N] [--format json|md]`, run from the package
about to be released. It resolves the version `yan version` would write, builds the candidate in the query
database, runs the analysis below and exits without publishing. `--impact` and the real publish are
exclusive in one invocation; the publish is a second, separate command, so a report is never a side
effect.

Options: (a) a flag on `publish`; (b) a new command `yan impact`; (c) a registry-side job run on every
staged publish. (b) adds a name for a step whose first half (resolve and build the candidate) `publish`
already owns (practice 12). (c) needs the registry builder and a place to show results; it is the right
end state but not buildable now, and it can reuse the engine of (a). Recommended (a), with (c) left to
its own RFC after M2b. A new command name would be an RFC and a lead card in any case (frozen names).

- Decision:
- Row:

### D2: which dependents

Proposal: the candidate set is the dependents that would use the release, not every dependent. Version
selection is minimal version selection over minimum requirements (19.1), so a dependent that requires
`1.2` keeps resolving `1.2.0` after `1.2.1` is published; it meets the release only when it raises its
requirement or the lock is updated. The question the author needs answered is what happens when
dependents move, so the set is: for each dependent package, its latest version per major whose
requirement admits the candidate's version, from the registry's reverse-dependency data.

Three sources, in order of availability:

1. Workspace members and `--dependents DIR`, a directory of unpacked packages or archives with their
   lockfiles. Needs nothing new and works today for a monorepo.
2. A static index path listing, per package, its dependents and their requirement ranges, served beside
   `list` and `.info` and mirrorable (19.3). The search snapshot has only a dependents count (R645), which
   is not enough to enumerate. New index data; the spec edit is in "Spec changes".
3. The registry service later (D1 (c)).

Options: (a) the above; (b) every version of every dependent (most are dead releases whose tests nobody
reads); (c) only direct dependents' latest version. (c) loses a dependent that is two majors behind but
still depended on. Recommended (a), counting a dependent only once per major.

- Decision:
- Row:

### D3: which tests run

Proposal: for each dependent, substitute the candidate for the package in that dependent's own lock, in
memory (nothing is written, and `overrides` are not involved). For every test of the dependent, compute
the 18.4 key twice, with the locked version and with the candidate. Then:

- keys equal: the verdict is the old one, marked `carried`, not rerun;
- keys differ and the test binds no real capability: run it in the checked interpreter under its
  deterministic step budget, and report `pass` or `fail`;
- the test binds a real capability with `with` (an I/O test, never cached): not run, reported
  `unjudged` with the reason. The author's machine does not give third-party code a network or a disk.

A test already failing at the locked version is `pre-existing`, never blamed on the candidate. The
dependent's `dev_dependencies` come from its own lock, since tests need them and they are outside its
capability ceiling (19.1). Doc fences and `ex` clauses count as tests as `yan test` counts them (R315).

Options: (a) the above; (b) rerun every test of every dependent (crater precision, crater cost, and the
answer is the same for every test whose key is equal); (c) rerun only tests that call a changed `pub`
item directly. (c) misses a test reaching a changed private helper through an unchanged `pub` item, which
is the common patch release (the same argument as in RFC 0069 D2). Recommended (a).

- Decision:
- Row:

### D4: transitive dependents

Proposal: a dependent of a dependent is in scope only when it can see the change. Walk the dependents
graph outward from the changed package; at each dependent, compare the reached hashes of its exported
items with and without the candidate; continue past a dependent only if some exported item's reached hash
changed. A dependent that re-exports a changed item, or whose `pub` function calls one, passes the change
on; a dependent that only uses an unchanged part of the package stops the walk. Tests are then judged per
D3 in every package the walk visited.

Options: (a) the walk above; (b) direct dependents only, `--depth 1` as the default; (c) the full
transitive closure. (c) is where crater's cost comes from and is mostly unaffected packages. (b) is
cheap but silent about a break carried through an intermediary. Recommended (a), with `--depth N` as an
explicit cap that is printed when it cuts a branch.

- Decision:
- Row:

### D5: what the report says, and what it does not decide

Proposal: one record per dependent test whose verdict is not `carried`, plus counts for the rest: tests
considered, carried, rerun (pass, fail), pre-existing failures, unjudged. A failure record names the
dependent package and version, the test id, the failure (as `yan test --format json` prints it) and the
changed item hashes the test reaches, so the author can open the reason with `yan why`. Exit status is 1
when a rerun test fails, 0 otherwise, and a report that could not reach its dependents is not 0: it is a
distinct status (D8).

The report is evidence, not a gate. A new failure is a reason for a major release or for
`publish --behavior-change item` (20.6), and the report's failing items are what that flag records. This
RFC does not make `publish` refuse on a failing dependent, because a dependent that tests an
implementation detail it should not (and which the author is right to break) is the author's call.

Options: (a) report only; (b) a failing dependent raises the enforced minimum bump (20.6); (c) `--impact --strict`
fails the publish. (b) lets other people's tests set the author's version number and is open to a
dependent registering a test to block a release. Recommended (a); (c) can be added by a later RFC if (a)
is used and ignored in practice.

- Decision:
- Row:

### D6: running other people's code

Proposal: unpack each archive from the store, verify its digest against `.info` as every fetch does
(19.3), and run only tests that bind no real capability, in the checked interpreter, with the step budget
of `yan test`. Stage-0 evaluation is hermetic and archives carry no build scripts (19.3), so nothing a
dependent ships runs outside the interpreter. A test that exceeds its budget is `fail` with the budget
named, not a hang. Nothing is executed natively, and `--impact` needs no `toolchain` capability beyond
the fetch that `yan add` already uses.

The one thing this relies on is that the interpreter confines a pure test. It is the same assumption as
for `yan test` on the author's own tests and for the agent gate; an escape is a bug in that confinement,
not something this RFC adds to the risk. A dependent whose manifest grants an ambient capability is no
different: the test either binds it with `with` (unjudged) or cannot reach it.

Options: (a) the above; (b) run in a container or VM as well. (b) adds a second sandbox to maintain for a
property the language already claims; if the confinement is weaker than claimed, that is the defect to
fix. Recommended (a).

- Decision:
- Row:

### D7: cost, ordering and caps

Proposal: the analysis is two phases. Phase one (keys only) computes every test's old and new key and
runs nothing; for most releases this is the whole job and costs a dependency fetch and a hash pass per
dependent. Phase two runs the tests whose key changed. Dependents are visited in order of dependents
count, then name (R645), and `--max N` caps the dependents visited. A cap is never silent: the report
lists how many dependents were not visited, so a green report on a capped run does not read as full
coverage.

Verdicts computed here are written to the same `.yan/cache/verdicts.json` as any `yan test` verdict, under
the dependent's test key, so a second run on the same release costs the key pass only and the MCP
`run_tests` tool sees them (R669). They are not uploaded anywhere.

Options: (a) the above; (b) random sampling of dependents. (b) makes the report differ between runs on
the same input. Recommended (a).

- Decision:
- Row:

### D8: determinism and what the report is a function of

Proposal: the report is a pure function of the candidate's hashes, the dependents list, the dependents'
archive digests and the toolchain hash. The dependents list is read from a log checkpoint (19.3), and the
report prints the checkpoint it used, so two runs against the same checkpoint print the same bytes and a
later run is explainable as "the index moved". There are no timestamps and no host paths in the report,
and records are sorted by package, version, then test id. A run with no reachable index, an unverifiable
checkpoint or an archive whose digest does not match is not "no impact": it prints which input was
missing and exits with the distinct status of D5, since an empty report on missing data is the wrong
answer a reader is most likely to trust.

Options: (a) the above; (b) best effort, skip what cannot be fetched. (b) is the silent fallback that
practice 13 rejects. Recommended (a).

- Decision:
- Row:

### D9: conformance

Proposal: three fixture sets under `conformance/impact/`, proposed after acceptance as `conform/*`
changes (agents do not write `conformance/`):

1. A fixture registry with a package and three dependents; a release that changes only an item no
   dependent reaches. Expected: zero rerun, all `carried`.
2. A release that changes a private helper reached through an unchanged `pub` item. Expected: the one
   dependent test that reaches it reruns and fails; the others `carried`.
3. A dependent whose only affected test binds a real capability. Expected: `unjudged`, never `pass`.

And one law, checked by the differential harness: for every fixture, the set of dependent tests that fail
when all tests are rerun under the candidate is a subset of the set the key-based selection reruns. It
is the soundness claim of D3 and the only way to find a key that misses a dependency (a missing edge in
the reached-hash computation would also be a bug in the verdict cache and in `yan review`, so it is one
issue, not three).

Options: (a) the above; (b) no soundness law. (b) leaves the false `carried`, the dangerous result,
untested. Recommended (a).

- Decision:
- Row:

## Spec changes

- 18.1: the `yan publish` row gains `--impact [--dependents SRC] [--max N] [--format json|md]`, M2b.
- 19.3: the index gains a dependents path with requirement ranges (D2), mirrorable and covered by the
  checkpoint; the search snapshot keeps its count.
- 19.4: an impact paragraph after Publish refusals (D3 to D8).
- 20.6: a sentence in Behavioral check that `--impact` can supply the failing items `--behavior-change`
  records (D5).
- 24 (conformance suite): `impact/` fixtures and the soundness law (D9).

Known gap, to be filed as a numbered item in a gap issue on acceptance: until the registry and its
dependents path exist (M2b), D2 sources 2 and 3 are unimplemented and `--impact` covers workspace
members and a local directory only.

Implementation order: the reached-hash and key computation shared with `yan test` and `yan review`
(phase one of D7, over a workspace), then D3's rerun, then D4's walk, then the index path and the
registry source. New code goes in a new module of `yan-cli` and `yan-pkg`, not in the lowering files of
practice 5.
