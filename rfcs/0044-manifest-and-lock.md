# 0044: Package manifest and yan.lock

- Status: draft
- Research: none; starts from spec 19.1, 19.3 and RFC 0020, and from issue #362
- Rows: filled in when accepted

## Summary

Spec 19 fixes the manifest's fields and the lock's contents but not the pieces a toolchain needs to
implement `yan add`, `remove`, `update`, `tree`, `why`, `vendor`, `x` and `toolchain`: the manifest's
schema rules, the lock's text format and versioning, the resolver's exact procedure, the static index's
file contents and how a build finds an index. This note lists each as a decision with options and a
recommendation. It records no decision. The pkg conformance family cannot grow past its current cases
until these are settled.

## Decisions

### D1: manifest file name

The spec and the CLI already use `package.yag`: one `package: Package = Package(...)` constant, literals
only (a data file). Options: (a) keep `package.yag`; (b) add a Yan-surface twin `package.yn` read the
same way; (c) rename to a neutral `yan.yag`.

Recommendation: (a). It is implemented, the data-file profile already checks it, and a second spelling
doubles every tool's discovery rules. A `.yn` twin can come later without a format change, since the
surface only changes the spelling of the same constant.

- Decision:
- Row:

### D2: manifest schema rules

The field table is settled. Open are the rules around it. Options for unknown fields: (a) an error;
(b) a warning, so older toolchains read newer manifests. Options for required fields: (a) `name`,
`version`, `edition` and `yan` required for a published package, all optional for a script-style `.yag`
and a workspace root with `members` only; (b) all four always.

Recommendation: unknown fields are an error (a), because the manifest is frozen at 1.0 and additive
after, and a silently ignored `capabilities` typo is a security hole; the `yan` field already gates which
toolchain may read it. Required fields (a), with `name` and `version` waived only for a root that sets
`members` and publishes nothing.

- Decision:
- Row:

### D3: lock file encoding

The lock is machine-written. Options: (a) a Yan data file, one `Lock(...)` literal in `yan.lock`, using
the same reader as `package.yag`; (b) TOML, as Cargo and uv do; (c) JSON Lines, one record per line.

Recommendation: (a) with a canonical print: records sorted by (name, major), fields in fixed order,
one field per line, trailing commas. One reader and one canonical printer are already specified for data
files, merge conflicts are line-local, and no second parser enters the trusted base. TOML would reuse
`std.toml` but adds a format `yan` otherwise needs only for interop.

- Decision: TOML, as Cargo and uv do (not the recommendation); Antonio, 2026-11-07. Written with the same canonical
  rules: records sorted by (name, major), fields in fixed order, one field per line.
- Row:

### D4: lock format version

The spec says the lock carries a format version and newer toolchains read every older lock. Open: how it
is written and when it changes. Options: (a) one integer `format` on the first line; a toolchain rewrites
a lock into its own version only when the lock changes anyway; (b) a toolchain always upgrades the lock
it reads; (c) the lock records the writing toolchain's version and no format number.

Recommendation: (a). Reading never writes, so a CI job on an older toolchain does not churn files, and a
newer lock read by an older toolchain fails with a message naming the needed version. Pinned data
versions (tzdb, Unicode, CLDR, ISO 4217, `math_edition`) and header hashes live in named top-level
fields, not in the format number.

- Decision:
- Row:

### D5: resolver procedure

Selection is minimal version selection over minimum requirements, with (name, major) as identity. The
procedure around it is open. Options for what the resolver reads: (a) only each version's `.info` in the
index, which lists its requirements, so no archive is fetched until the graph is chosen; (b) the
manifest inside each candidate archive. Options for choosing a version when adding: (a) `yan add` writes
the newest version that is outside the cool-off window and records it as the requirement's minimum;
(b) it writes the minimum that satisfies the user's text.

Recommendation: (a) for both. Reading `.info` keeps resolution to a few small requests and works the
same against the future registry. A requirement's minimum is the newest eligible release at add time,
so MVS stays deterministic and the cool-off applies to what is written, never to what is rebuilt. The
resolve order is fixed: overrides first, then dependencies, dev dependencies and tools as three
subgraphs of one lock, then the Python environment. A conflict names both requirers.

- Decision:
- Row:

### D6: static index contents and discovery

The layout is settled (`list`, `.info`, `.zip` per version). Open: what `.info` holds, and how a build
maps a namespace to an index. Options for `.info`: (a) JSON with archive digest, publisher identity,
computed capability set, requirements, yank state and publish time; (b) the same plus the manifest text.
Options for discovery: (a) a `registries` table in a user or workspace configuration data file
(`.yan/config.yag`), exact namespace to URL or directory; (b) an environment variable only; (c) a field in
the root manifest.

Recommendation: (a) for `.info`, so the log leaf later hashes the same fields; (a) for discovery, kept
out of the manifest so a dependency cannot redirect a namespace. Publish time is recorded in `.info`
because the cool-off needs it before any log exists. Until M2b the default namespace maps to nothing:
`yan add` on an unmapped namespace fails with a message naming the config key.

- Decision:
- Row:

### D7: when the lock is written

Options: (a) only `add`, `remove`, `update`, `approve`, `vendor` and an explicit `yan lock` write it; `build`,
`check` and `test` read it and fail on drift under `--locked`, and otherwise resolve from the manifest
without writing; (b) every command refreshes the lock as Cargo does.

Recommendation: (a). A plain build never selects new versions, and a build that rewrites a file the patch
protocol protects would be a surprise to agents and to reviewers. A missing lock for a package with
dependencies is a resolve from the manifest's minimums, written only by `yan lock`. `--offline --locked`
fails if any pinned archive is absent from `.yan/` or the vendor directory.

- Decision:
- Row:

## Spec changes

On acceptance: 19.1 gains the schema rules (D1, D2), the lock encoding and version (D3, D4) and the
write rules (D7); 19.3 gains the `.info` fields and discovery (D6); 19.4 gains the resolve procedure
(D5); 18.1 gets `yan lock` if D7 is accepted. New diagnostics, if any, take the next free code in their
section 25 family. The pkg and solver conformance cases for each accepted item are proposed in a
separate PR.
