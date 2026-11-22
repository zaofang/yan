# Registry design review record

Audit row: M1 audit 2026-11-21, row 9, "Registry design (section 19) written and reviewed" (docs/reports/2026-11-21-m1-audit.md). Issue: #411.
Commit reviewed: main 140beda, 2026-11-21. Review date: 2026-11-22.
Reviewer: Claude, an agent. The agent also wrote parts of the reviewed text, so this is a design read, not an independent review. The row becomes Present only when the lead countersigns below.

## Scope

Read in full: spec/19-packages-registry-and-supply-chain.md, rfcs/0020-packaging-and-ecosystem.md.
Read in part: rfcs/0084-module-and-package-structure.md (summary, principles, decision headings, conformance and open-point sections; the decision bodies D1 to D13 were not re-derived, only checked against their spec 19 rows R1064 to R1081 and R1135 for naming).
Not reviewed: registry code (none exists; the registry ships at M2b), RFC 0020 D6 and D7 internals (M2 and M4).

## Verdict

The design is coherent and complete enough for M1. Every RFC 0020 decision (D1 to D7, S1 to S6) has a matching row and a matching passage in 19.1, 19.3 or 19.4, and RFC 0084 and spec 19 agree on names and error codes. One required fix (F1). Four findings to track (F2 to F5).

## Findings

### F1 (required): retired name in spec 19.3

19.3 "Namespaces and names" binds a DNS namespace through a `_yong` TXT record. `yong` is a retired name (CLAUDE.md practice 15: retired names do not appear outside the archive), and the record name is a protocol constant, so it must be settled before the registry exists. Recommended: `_yan`. This is a spec edit and needs the lead's decision and a lead-owned PR; not made here.

### F2: no conformance case cites R635 to R647

At 140beda no case under conformance/ cites any of R635 to R647 (RFC 0020's rows). Later rows (R1075, R1079 and others) cover name resolution, but the dependency kinds, overrides, pre-release rule, Python lock, yank, search snapshot, E016 refusals and the optional lock fields have no case. This is spec debt under practice 16 and counts toward the M1 conformance items (#413, #415, #416, #418). Proposed `conform/*` cases are a lead-gated follow-up.

### F3: pre-registry trust gap, accepted

Until M2b the index is static and unsigned (R636). Two 19.4 rules read data from it before any log verification:
- the cool-off bypass for "fixes an advisory" trusts `fixes` in `.info`;
- a malicious mark is honored from the unsigned index (R947).

A hostile index can therefore shorten the cool-off for a chosen version, or deny service by marking versions malicious. Integrity is still held by the archive digest, publisher-identity pinning (E005) and ceiling checks, so this is accepted for M1. Recommend one sentence in 19.3 "Before the registry" stating that the bypass is unverified until log verification ships.

### F4: witness availability is unspecified

19.3 requires 2 of 3 cosignatures from the public transparency.dev witness network (R637). It does not say what a client does when fewer than the quorum are reachable on first fetch of a version (fail with E011, or use the last stored checkpoint and say so), nor who tracks changes to the public witness set. Recommend a row before M2b: fail closed with E011 for a new version, accept a stored checkpoint for versions already in `yan.lock`.

### F5: operational concentration on one person

Name review is a weekly queue the lead reviews (R637), the std and `@yan/*` rebuilder is the only independent rebuilder, and `rebuilders` defaults to 0. Appropriate for one part-time developer and 0.1.0, and nothing here weakens a 19.3 or 19.5 property. Record as a known limit; revisit if the registry has outside publishers.

### Minor

- rfcs/0084 header still says `Rows: pending` although its rows exist (R1064 to R1081, R1135). Lead-owned path (rfcs/).

## Checks that passed

- Each RFC 0020 row R635 to R647 exists in spec/rows.toml and is cited in 19.1, 19.3 or 19.4.
- 19.1 and RFC 0084 agree on source roots, dependency roots, `pub use` and workspace binding (R1064, R1074, R1075, R1079, R1135).
- Capability ceiling, FFI consent and lock recomputation (19.2, R1018 to R1034) close the gap RFC 0020 D2 leaves by rejecting git dependencies.
- Publishing credentials are OIDC-only with no token type; archives are deterministic and re-verified on read; install and build scripts do not exist.
- No RFC status changes are made or implied by this record.

## Lead countersign

Required before audit row 9 moves from Partial to Present.

- [ ] F1 decided: `_yan` / other: ______
- [ ] F2 to F5 accepted as tracked, or sent back
- Countersigned by: ______  Date: ______
