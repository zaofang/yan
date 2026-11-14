# Contributing to Yan Core

Yan has one lead, Antonio ([@amteb](https://github.com/amteb)), who owns the spec, the tests and the gates,
accepts RFCs, approves tests, merges and cuts releases.

## Filing issues

Label every new issue `group:P1` to `group:P10` (see `docs/research/2026-11-14-work-hierarchy.md`) and add it to that group's `TRACK-P<n>` tracker.

## Sign-off (DCO)

Every commit must carry a `Signed-off-by:` line (`git commit -s`), certifying the
[Developer Certificate of Origin 1.1](https://developercertificate.org/). Contributions come in under the
license they go out under (see `LICENSING.md`). There is no CLA. The `dco` workflow fails a PR if any
commit lacks a matching `Signed-off-by:`; fix with `git rebase --signoff main` and force-push.

## AI-assisted contributions

AI-assisted work is welcome when a human has read it, run it and takes responsibility for it.

- Mark each such commit with an `Assisted-by: <tool or agent>` trailer.
- Bulk or unreviewed agent PRs from outside contributors are closed without review.
- Project agents may draft, research, implement and review. They never accept RFCs, approve tests, merge,
  publish, or edit `tests/` or `conformance/`. CODEOWNERS and the CI path guard enforce this. Agents may open PRs
  that edit `spec/` or `rfcs/` for rows the lead accepted; the lead merges them.
- These rules are mechanical (trailers, path guards, CODEOWNERS) on purpose. Rust's AI-contribution
  policy, RFC 3950, was closed because it could not be enforced fairly; a rule a tool checks does not
  depend on judging intent.

## What needs an RFC

An RFC is required for anything in the spec's freeze table (R171), editions, the std public API, the grammar
of any surface, registry policy, and the project's licensing and governance rules. Bug fixes, diagnostic
wording, performance work and internal refactors do not need one.

The pipeline: draft, adversarial review by an independent reviewer, decision by the lead, spec rows, preview
behind `@preview`, stable. Accepted and rejected RFCs are kept in `rfcs/NNNN-slug.md`; [rfcs/README.md](rfcs/README.md)
says how to write one. Row numbers are taken only in the PR that writes the rows, from `tools/rows.py next`, and
CI rejects a row registered twice. Studies are listed in [docs/research](docs/research/README.md).

## Tests

Tests under `tests/` and `conformance/` are approved by the lead and read-only. Propose new or changed tests in
a separate PR.

## Conduct and security

See `CODE_OF_CONDUCT.md` and `SECURITY.md`.

## Merging

`main` is protected by a ruleset (`.github/rulesets/main.json`): `guard`, `canary`, `check`, `formal`, `rust` and
`signoff` must pass, with no bypass. A red check is fixed, never merged over. Apply it with
`gh api -X POST repos/zaofang/yan/rulesets --input .github/rulesets/main.json`.
