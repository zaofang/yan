# 0072: Proof-carrying patches: signed test evidence that `yan review` and `yan test` accept by key lookup

- Status: draft
- Research: [Yan tooling study: borrow and champion](https://claude.ai/artifact/SAaQdxrqitundhjM7DoBxz),
  champion C7; tracker #1004, under #449.
- Rows: filled in when accepted

## Summary

A test that binds no real capability is a pure function of its verdict key (18.4, R669): the test
hash, the identity hashes of every item it reaches, and the toolchain hash. Two machines that
compute the same key MUST get the same verdict. Today that fact is used only inside one machine,
through `.yan/cache/verdicts.json`. A patch from an agent, or a PR from a CI job, arrives with no
evidence, so the reviewer's `yan review` and the project's CI run every reached test again.

This RFC lets a patch carry *evidence*: a signed statement that named verdict keys had named
verdicts. The reviewer recomputes the keys locally, which needs elaboration and no execution, and
accepts a verdict only when the recomputed key equals the attested key and the signer is trusted.
Everything else runs as today. Evidence changes cost, never the answer.

Nearest prior art: a Bazel remote cache shares build outputs, and trusts the cache writer; it does
not carry review evidence with a change, and Bazel actions are not required to be deterministic.
Yan can do this because a verdict is a function of its key.

The champion is not about proofs of correctness. "Proof" here means a replayable fact: anyone can
rerun the test and obtain the same verdict, so a lying signer is detectable (D4).

Dependencies: the verdict cache and `yan review` are built (checked against
`crates/yan-cli/src/review.rs` and the 18.4 text at 4fbc37b, 2026-11-11). Signed approvals reach M2
(R948); until then only `signer = "local"` exists, which is the case D3 handles. This RFC reads the
verdict key as 18.4 defines it and does not redefine it (practice 12). It composes with RFC 0067
(no-op review, draft): a behavior-neutral change reaches no changed item, so BASE verdicts already
cover it and evidence matters only for changes that do reach tests.

## Decisions

### D1: what evidence is

Evidence is a claim about keys, not about trees. One evidence statement holds:

- the key scheme version (so a change to the 18.4 key invalidates old evidence instead of
  matching it by accident);
- the toolchain hash the verdicts were produced under;
- a list of entries: test item path, test hash, verdict key, verdict (`pass`, `fail`), and the
  instruction count of the run;
- `after`: the identity hash of every item the patch added, replaced or edited, as the signer saw
  them after applying it.

It names no source text and no values, so it leaks nothing a patch does not already carry. It
binds to the exact post-patch state through hashes, with no commit or tree id, so it survives a
rebase that leaves those hashes alone.

Only tests whose keys 18.4 allows in the cache can have entries. A test that reaches a `with` real
capability, and any test the toolchain will not cache, has none and always runs. Fuzz, mutation,
`explore` and bench results are out of scope; their keys need seeds and budgets that 18.4 does not
yet key on.

Options: (a) the above; (b) a whole-tree attestation ("tests passed at commit X"), which breaks on
every rebase and says nothing about which tests it covers; (c) a copy of `verdicts.json`, which is
unsigned local state with no `after`. Recommended (a).

- Decision:
- Row:

### D2: how a reviewer checks it

For each entry, in order:

1. Apply the patch (or read HEAD) and elaborate as `yan review` already does (R665).
2. Check `after` against the item hashes now in the tree. Any difference discards the whole
   statement with a note naming the first differing item (D6).
3. Recompute the verdict key of the entry's test at this tree. Equal to the attested key: the
   entry applies. Different: that test runs through the normal cache path and the entry is
   dropped.
4. For an applying entry, check the signer (D3). Trusted: report the verdict as `attested`. Not
   trusted: ignore the entry as if absent.

No step runs a test. A statement never causes a test to be skipped on its own claim: a test is
skipped only when step 3 recomputed its key on the reviewer's tree.

Verdicts have a third provenance beside 18.4's `cached` and `run`: `attested`, shown with the
signer, so a reader can tell "I checked" from "someone I trust checked".

Options: (a) the above; (b) trust the attested key without recomputing it. (b) lets a statement for
another tree vouch for this one. Recommended (a).

- Decision:
- Row:

### D3: who may vouch

The reviewer's policy names the identities whose evidence it accepts. The policy is a new
`verifiers` list in the root `package.yag`, beside `approvers`, entries being Sigstore identities
(repo, workflow, ref) of the same kind as `approvers` (19.4). It is separate from `approvers`
because approving a capability gain and attesting a test run are different acts held by different
parties: a human approves, a CI workflow attests.

Statements are in-toto statements (DSSE, Sigstore bundle) like signed approvals, stored beside
them in `.yan/bundles/`; the patch carries only the bundle digest, in an optional `evidence` field
of the 18.3 patch (a bundle is large and a patch is reviewed as text). The `toolchain` capability
cannot produce a signed statement, the same bar as `yan approve`, so an agent can never vouch for
its own patch.

Until the signing path lands (M2), `yan test --attest` writes `signer = "local"`, as R948 does for
approvals. A local statement is accepted only by the machine that wrote it and fails `verifiers`
elsewhere (E009 family, as R948). On the writing machine it adds nothing over the verdict cache,
which is why signed evidence is what this champion delivers; the local form exists so the format
can be built and tested before M2.

Options: (a) the above; (b) reuse `approvers`, which lets any approver's key vouch for test runs and
mixes two policies; (c) accept unsigned evidence as a hint. (c) is no verification and invites
mistaking a hint for evidence. Recommended (a).

- Decision:
- Row:

### D4: a lying verifier is detectable

Because a verdict is a function of its key, a verifier that signs a wrong verdict can be caught by
rerunning. `yan test --audit-attested N` reruns a deterministic sample of N attested entries
(chosen by sorting verdict keys by their BLAKE3 hash, so the same tree samples the same entries)
and compares. A mismatch is a finding that names the signer and the key, exits 1, and is never
cached. The default is no sampling, since sampling costs what the champion saves; a project that
wants a standing check sets it in CI.

Options: (a) the above; (b) no audit, trust only; (c) always rerun a fraction at random. (b) leaves a
compromised workflow identity undetectable. (c) is not reproducible. Recommended (a).

- Decision:
- Row:

### D5: what evidence does not touch

Evidence supplies verdicts for tests and nothing else. It does not change approval or any "needs a
human" finding of 21.5:

- a test edited by the patch is a protected target (18.3, P006) and its approval is judged by
  BASE's `tests.lock` (R668); an attested verdict for an edited, unapproved test stays `proposed`;
- capability, `fail`, `unsafe` and `pub` gains, extern changes and frozen edits are computed from
  Core and are not attested;
- exit status: an attested pass counts as a passing reached test and an attested fail as a failing
  one, exactly as a run would.

Options: (a) the above; (b) let attested passes clear findings. (b) turns a CI identity into an
approver. Recommended (a).

- Decision:
- Row:

### D6: when evidence is stale

Evidence is discarded, never partly applied, when: the key scheme version differs; the toolchain
hash differs from the reviewer's (the toolchain hash is the BLAKE3 of the `yan` binary, so builds
that are not bit-identical never match, and reproducible builds are what make evidence portable);
`after` does not match the tree; or the bundle does not verify. In each case the tests run through
the cache as if no evidence existed, and the response says why in one line, so a silent loss of the
CI saving is visible (practice 13).

Evidence from a different toolchain is not accepted with a warning. A verdict from a different
binary is not the same fact.

Options: (a) the above; (b) accept older minors when the keys match. (b) is wrong whenever a
toolchain change alters a verdict without altering a key, which the key's toolchain component
exists to prevent. Recommended (a).

- Decision:
- Row:

### D7: commands

- `yan test --attest [-o FILE]` runs the cacheable tests reached by a change and writes the
  statement (unsigned or `local` until M2; signed by the CI workflow identity from M2).
- `yan test --evidence FILE` and `yan review --evidence FILE` read one. Without the flag both
  look in `.yan/bundles/` for a bundle whose digest an applied patch names.
- `yan patch` records the bundle digest in the oplog entry (18.3 item 7), so `yan undo` leaves the
  bundle alone.
- MCP `run_tests` and `check_patch` accept the same `evidence` argument; the result marks each test
  `attested`, `cached` or `run`.

No new command name is proposed. Frozen names are untouched.

Options: (a) flags on `test` and `review`; (b) a new `yan verify` command. (b) adds a name and a
second place that decides what runs; (a) keeps one decision in `yan test` (practice 12). Recommended
(a).

- Decision:
- Row:

### D8: conformance and laws

Cases proposed through `conform/*` in a separate PR (agents never edit `conformance/`):

- Same answer: for every conformance program with tests, the verdict set from a full run equals the
  set from a run that accepts honest evidence. Evidence only moves tests from `run` to `attested`.
- Forged key: a statement whose key differs from the recomputed key never skips a test.
- Stale: a different toolchain hash, key scheme version or `after` discards the statement and says
  so.
- Signer: an unsigned or unlisted signer is ignored; a `local` statement is rejected on another
  machine.
- Lying verifier: a statement with a flipped verdict is caught by `--audit-attested` and by a
  plain run, and the finding names the signer.
- Protected: attested evidence never changes approved or proposed status, and never clears a
  21.5 attention finding.

Differential: `uv run python -m tools.difftest` compares full-run and evidence-run verdicts on the
interpreter and native once native runs tests; until then the laws run on the interpreter only.

Options: (a) the above; (b) unit tests of the checker. (b) misses the first law, which is the
champion's safety claim. Recommended (a).

- Decision:
- Row:

## Spec changes

- 18.3: optional `evidence` field (bundle digest) in the patch JSON; oplog records it.
- 18.4: `attested` verdict provenance beside `cached` and `run` (R669); `--attest`, `--evidence`,
  `--audit-attested` and the key scheme version; D2 order, D6 discard rules.
- 19.4: `verifiers` in the root `package.yag`; statement format and `local` signer, beside signed
  approvals (R948).
- 21.5: `yan review` shows `attested` with the signer, D5 limits; `--evidence`.
- 24 (conformance suite): D8.

Diagnostics: none proposed beyond the existing verification-signer code family of R948. A discarded
statement and an audit mismatch are tool results with exit statuses (D4, D6), not program
diagnostics, as in RFC 0066 D7.

Implementation order: statement format and `--attest` with `local` signer, then recomputed-key
check in `yan review` and `yan test`, then the `evidence` patch field, then `--audit-attested`,
then the signed path with R948 at M2. New code goes in new modules of the CLI and the verdict
cache, not in the lowering files of practice 5.

Python-fidelity policy: not touched; no surface or builtin changes. Frozen names: none changed;
`verifiers`, `attested` and `--attest` are new names, to be fixed by the lead on acceptance
(practice 15).
