# 0085: The first milestone is 0.1.0

- Status: accepted
- Research: none
- Rows: none (wording of existing rows changes; no new row)

## Summary

The first release milestone, called 1.0 until now, is named 0.1.0 throughout the spec, RFCs, docs and
tools. The freeze list, the toolchain requirement and the "after 1.0" buckets move with it.

## Decisions

### D1: Rename the milestone and its freezes

Options weighed: (a) rename the milestone only and keep the stability freeze at a later 1.0; (b) rename
everything to 0.1.0; (c) docs only. The lead chose (b).

- Decision: (b), rename everything to 0.1.0 (lead, 2026-11-17)

### D2: Rules keyed to a package version of 1.0

E002, E016 and R643 refuse previews and pre-release dependencies for "a package at 1.0 or later"; the
code in `yan-cli` tests the major version, and conformance cases pin versions around that line. The
spec text for those rules is unchanged here. They move to 0.1.0 in a follow-up that changes the
threshold, `conformance/` and `yan = "1.x"` in the manifests together.

- Decision: pending the follow-up (lead card)

## Spec changes

Wording only: 1.0 becomes 0.1.0 and `1.x` becomes `0.1.x` in sections 2 to 26, `spec/rows.toml` and the
rendered appendix, except float literals and the D2 rules.
