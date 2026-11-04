# 0020: Packaging and ecosystem

- Status: accepted
- Research: [Yan packaging and ecosystem](https://claude.ai/artifact/NHdZaQ9Q6X5tFxz1xh9BQC)
- Rows: R635 to R647

## Summary

Section 19 specifies the supply chain in depth but leaves the everyday parts of packaging open: how
source names a dependency, dependencies for tests and tools, local overrides, what M1 resolves
against, Python dependencies for `py`, and how first-party packages get native Rust code. Interop
shipped Yan into C, Rust and Python but not into npm, and had no wheel or `build.rs` path. This RFC
fills those gaps and lowers what the registry needs one person to operate, without weakening any
property of 19.3 or 19.5.

## Decisions

### D1: dependency names in source

Each `dependencies` entry binds a local root name; code calls `http.get(...)`. The default is the
last segment of the package name; two majors of one package, or two packages with the same last
segment, need explicit names. Local names may not shadow `std`, `core`, `alloc` or a prelude name.
Item hashes use the full package identity. Rejected: Go-style automatic `http_v2` (no answer for
same-named packages from two orgs) and fully qualified names in source (tokens on every call, law 11).

- Decision: local names in the manifest (owner, 2026-11-01)
- Row: R635

### D2: dependency kinds and the M1 index

`dev_dependencies` (tests, fixtures, benches; outside the ceiling and invisible to dependents),
`tools` (what `yan x` runs, each under its own ceiling) and a root-only `overrides` table (a
package to a local path, or to a git commit with its archive digest). No general git or path
dependencies. M1 resolves against a static index in the 19.3 layout served from a directory or
HTTPS, unsigned; `yan.lock` records archive digests from M1. Rejected: git dependencies anywhere
(they bypass computed capabilities and the log); no change.

- Decision: dev, tools, root overrides, static index in M1 (owner, 2026-11-01)
- Row: R636

### D3: registry operations

The Yan log runs on Tessera over object storage. Cosignatures come from the public transparency.dev
witness network under the 2-of-3 quorum policy. The registry builder is one sandboxed job per
publish. The independent rebuilder for std and `@yan/*` runs on GitHub Actions. Name review is
automatic rejection plus a queue the lead reviews weekly. Rejected: running three witnesses and a
builder fleet; putting leaves in Rekor v2 (its entries lack the capability digest).

- Decision: own log, public witnesses (owner, 2026-11-01)
- Row: R637

### D4: Python dependencies

A `python` field, `Python(requires=">=3.12", dependencies=[...])`. Libraries declare
requirements; the root resolves one environment for the whole graph with uv into `pylock.toml`
(PEP 751), whose digest `yan.lock` pins. A package with a `python` field must list `py` in its
ceiling. Conflicting requirements are a resolve error naming both packages. Wheels only. Rejected:
root packages only; none at 1.0.

- Decision: libraries declare, root locks (owner, 2026-11-01)
- Row: R638

### D5: native code for first-party packages

The toolchain bundles the native libraries first-party packages bind (ICU4X for `@yan/i18n`),
built reproducibly with the toolchain under its cargo-vet audits (19.6). Packages stay C-only at
1.0; Rust sources in packages is a post-1.0 RFC. Rejected: Rust sources at 1.0; prebuilt binaries.

- Decision: bundle in the toolchain (owner, 2026-11-01)
- Row: R639

### D6: shipping into PyPI and crates.io

`yan build --wheel` writes abi3 and abi3t wheels from the export set with PEP 740 attestations
derived from Yan provenance. The `yan` PyPI wheel is also a PEP 517 build backend. A published
`yan-build` crate lets `build.rs` run `yan build` and link the generated `-sys` crate. M2.

- Decision: wheels, PEP 517 backend and `yan-build` (owner, 2026-11-01)
- Row: R640

### D7: npm

No npm path at 1.0. With M4 components, `yan bind --lang js` runs jco transpile and writes an npm
package whose capabilities are explicit imports, published with npm trusted publishing.

- Decision: M4 through components and jco (owner, 2026-11-01)
- Row: R641

### S1 to S6: small fixes

Accepted together (owner, 2026-11-01).

- S1 requirement syntax `"@org/name x.y[.z]"` (R642)
- S2 pre-release selection and the 1.0 rule (R643)
- S3 yank by the publisher, with a logged reason (R644)
- S4 signed daily search snapshot `/-/index.jsonl` (R645)
- S5 E016 for manifests `yan publish` refuses (R646)
- S6 optional `yan.lock` fields for the Python lock digest and the M1 index URL (R647)

## Spec changes

12.7 (wheels, `yan-build`), 12.8 (bundled native libraries), 17.8 (Python dependencies), 18.1
(`yan build --wheel`, `yan bind --lang js`), 19.1, 19.3, 19.4, 23 (M4), 25 (E016).
