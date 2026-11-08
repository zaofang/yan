# 0053: Package manager gaps after the directory-index resolver

- Status: accepted
- Research: none; starts from issue #767, RFC 0044, spec 19 and the resolver in `crates/yan-cli/src/{registry,deps}.rs`
- Rows: R943 to R951 (D1 to D9 in order); E022

## Summary

The resolver, `yan add`, `update`, `remove` and `--offline` work against directory indexes. Issue #767
lists what is still unspecified or unimplemented. Items 7 and the `remove` half of item 4 are done (#799).
This note settles the open design questions, one decision each with options and a recommendation, so the
remaining implementation PRs have a spec to cite. Decisions D1 to D9 were taken as recommended on question cards, Antonio, 2026-11-08. Conformance cases are proposed in
a separate `conform/*` PR.

## Decisions

### D1: `.yan/config.yag` syntax

Today the reader accepts one constant, `config: Config = Config(registries={"@org": "dir"})`, with no
spec text. Options: (a) a fixed `Config` record, unknown fields an error, same data-file profile as
`package.yag`; (b) a free map of keys, so tools add their own; (c) TOML like the lock.

Fields under (a): `registries` (namespace to index, URL or directory), `min_release_age` (days, integer,
default 7; a workspace value may only raise the user's value unless the user file sets `allow_lower`),
`trust` (list of audit sources, 19.4), `offline` (bool, default false), `store` (path override, D2).
Lookup order: workspace `.yan/config.yag`, then user `$XDG_CONFIG_HOME/yan/config.yag`; per key, the
workspace wins, except `min_release_age` as above. `registries` entries merge by namespace.

Recommendation: (a). It reuses the reader and E018-style unknown-field rule already used for manifests, and
a typo in `trust` or `min_release_age` must not be silently ignored, since both weaken a gate. The lowering
rule for `min_release_age` stops a cloned repository from disabling the cool-off.

- Decision: as recommended; Antonio, 2026-11-08
- Row: R943

### D2: archive and store layout

Spec 19.4 says `--offline --locked` looks in `.yan/` or the vendor directory, with no layout. Options:
(a) content-addressed, `<root>/archives/<sha256>.zip` plus an unpacked cache `<root>/src/<sha256>/`, where
`<root>` is `.yan/store` for a workspace and `$XDG_CACHE_HOME/yan/store` for the user, vendor being the
same layout under `vendor/`; (b) name-addressed, `<root>/@org/name/<ver>.zip`, mirroring the index;
(c) a single tar-like pack file per workspace.

Lookup is workspace store, vendor, user store, then network. Every read re-verifies the SHA-256 against
`yan.lock` before unpacking; unpacking is into a temp directory and renamed. An archive with an entry that
is a symlink, an absolute path, a `..` component or a duplicate path is rejected (a new code).

Recommendation: (a). The digest is the identity everywhere else (index, log, lock), so two versions with
equal bytes share one file, a corrupt file cannot be mistaken for a good one, and vendoring is a copy of a
directory with nothing to translate. (b) is easier to browse, which `yan tree` and `yan why` do not need.

- Decision: as recommended; Antonio, 2026-11-08
- Row: R944

### D3: fields added to `yan.lock`

Spec 19.1 already lists the contents; the TOML field order is not fixed and the repo writes only name,
version, source, digest, publisher. Options for the new fields: (a) add them to each `[[package]]` table in
a fixed order; (b) keep `[[package]]` minimal and put approvals and attestations in separate top-level
tables keyed by (name, version).

Fixed order under (a): `name`, `version`, `source`, `digest`, `capability_digest`, `publisher`,
`approvals` (array of bundle digests), `rebuilds` (array of attestation digests). Bundle bodies live in
`.yan/bundles/<digest>.json` and in `vendor/bundles/`, not inline, so the lock stays diffable. Top level:
`format`, `[data]` (tzdb, unicode, cldr, iso4217, `math_edition`), `[headers]`, then optional `pylock`
(digest of `pylock.toml`) as R647 says. A package resolved before it has a computed capability digest
(directory index with no `capability` in `.info`) omits the field, and the first `yan approve` or registry
resolve fills it; omission under `--locked` with a registry source is E020.

Recommendation: (a) with bundles outside the lock. A package's whole security state is then one table, the
reviewer sees a capability gain as a one-line diff next to the version bump, and the bundles, being
large, stay out of merge conflicts. Inlining them (the 19.4 text says "bundles live in `yan.lock`") should
be changed to "referenced from"; this RFC asks for that edit.

- Decision: as recommended; Antonio, 2026-11-08
- Row: R945

### D4: HTTPS indexes

Today a `registries` value containing `://` is E017. Options: (a) support `https://` only; (b) also
`http://` for loopback, for tests; (c) also `file://`.

Requests: `GET <base>/@org/name/@v/list`, `<ver>.info`, `<ver>.zip`. Redirects are followed only within the
same host and only over HTTPS. A 404 on `list` means the package does not exist (E003 with nearest names
from the local search snapshot if present); any other non-2xx or a body larger than 1 MiB for `list` and
`.info`, or 256 MiB for `.zip`, fails with a record naming the URL. The archive digest is always checked
against `.info` and then against the lock; an index is untrusted (19.3). Timeouts: 10 s connect, 60 s
total per request. The HTTP client is the toolchain's own (no `curl` subprocess). The proxy settings
`HTTPS_PROXY` and `NO_PROXY` are honored. Credentials are not supported before M2b.

Recommendation: (a) plus loopback `http://` under (b) for tests only, gated on the host being `127.0.0.1`
or `::1`. A directory is already the `file://` case, so (c) adds a second spelling for nothing.

- Decision: as recommended; Antonio, 2026-11-08
- Row: R946

### D5: gate semantics for E004 and E007

E004 (agent add of an unlocked, non-established package) and E007 (malicious mark) are in the catalogue
with no implementation. The "established" test (30 days, 10 dependents) needs the search snapshot
(R645), which a directory index does not have. Options for E004 before M2b: (a) enforce with the data
present: with no snapshot, nothing is established, so an agent add of an unlocked package is always E004;
(b) skip the gate until the snapshot exists; (c) enforce on publish age alone from `.info`.

Options for E007 without a log: (a) the index's `.info` carries `malicious: true` and the client treats
the mark as authoritative from an unsigned index; (b) honor it only from a signed source, so never before
M2b.

Both gates apply to the caller, not to the content: the caller is an agent when the `toolchain` capability
is the authority for the call. A human running `yan add` interactively is never gated by E004. E007 refuses
the version even if pinned; the refusal prints the advisory text and the nearest clean version.
`yan update` ignores the cool-off for that move (R798). An E004 refusal prints the exact `yan approve`
command that would resolve it and records nothing in the lock.

Recommendation: E004 (a), E007 (a). Fail loud (CLAUDE.md practice 13): a gate that is off until a later
milestone is the silent fallback the practice forbids. An unsigned index can already substitute any
archive, but the lock's digest means the substitution is caught on the next build; a false malicious mark
at worst blocks a version, which `yan approve` with `--override-mark` (D6) clears.

- Decision: as recommended; Antonio, 2026-11-08
- Row: R947

### D6: `yan approve` and `yan vendor`

Neither is implemented. Spec 18.1 marks `approve` as M2, because it needs Sigstore. Options for `approve`
now: (a) implement the data path only: `yan approve pkg@ver` writes an unsigned approval record
(`signer = "local"`) that satisfies gates for the user's own workspace but not CI's `approvers` check
(E009); signing is added at M2; (b) wait for M2; (c) implement Sigstore keyless signing now.

`yan vendor` copies every locked archive into `vendor/archives/<sha256>.zip` and every referenced bundle
into `vendor/bundles/`, verifies each digest while copying, removes entries no longer locked, and writes
no source tree. It is deterministic: running it twice changes nothing. `--offline --locked` then needs no
store.

Recommendation: `vendor` as above. `approve` (a): the E004, E005 and E007 gates are unusable without some
approval to clear them, the record format is the final one except the signature, and CI refuses an
unsigned record, so nothing weaker than the spec is accepted anywhere that matters. The record carries
`signer = "local"` explicitly so M2 can reject it by value.

- Decision: as recommended; Antonio, 2026-11-08
- Row: R948

### D7: cool-off bypass for advisory fixes, and the yank reason

19.4 already bypasses the cool-off for a version that fixes an advisory affecting the current lock. Open:
where the advisory comes from without OSV export, and what `yan update` prints for a yanked version.
Options for advisory source: (a) the `.info` of the fixed version names the advisory ids it fixes
(`fixes: [...]`) and the locked version's `.info` names the ones affecting it; (b) a separate advisory
directory in the index; (c) none until the registry ships.

Recommendation: (a). It needs no new path in the static layout and `yan audit` already reads advisory ids
from `.info`. `yan update` prints, per yanked locked version, one line: the package, version, the reason
verbatim (control characters stripped, 200 characters, then `...`), and the version it moved to. A bypass
is printed as such, naming the advisory id, so a reviewer of the lock diff sees why a young version was
taken.

- Decision: as recommended; Antonio, 2026-11-08
- Row: R949

### D8: `overrides` and the `python` environment

The resolver ignores `overrides` and `python`. Options for `overrides` (root only; local path or git commit
with an archive digest, R636): (a) an override replaces the package and all of its versions in the whole
graph, and its own requirements are resolved as if it were a registry version; (b) an override replaces only
the named (name, major); (c) overrides are rejected with E017 until implemented.

For `python`: (a) resolve it after the Yan graph as R638 says, writing the `pylock.toml` digest to the lock;
(b) leave it to `uv` entirely and record only the digest.

Recommendation: overrides (a) restricted to the named (name, major), which is also what identity means,
so (a) and (b) coincide; the lock records `source = "override"` and the path or commit, and a root with
overrides cannot publish (E016, already specified). Until the work lands, a manifest with `overrides` is
E017, not ignored. `python` (b): the Python environment is `uv`'s job, the project assumes `uv`, and a second
resolver would drift from it. Yan resolves only what must be identical across machines, the digest.

- Decision: as recommended; Antonio, 2026-11-08
- Row: R950

### D9: dependencies, dev dependencies and tools as three subgraphs

R894 says the three are subgraphs of one lock; the resolver builds one graph. Options: (a) three graphs,
each resolved separately from the shared minimums, so a dev dependency's requirement raises a runtime
dependency's version only if both name it; the lock marks each `[[package]]` with `kinds` (`"dep"`,
`"dev"`, `"tool"`); (b) one graph with kind labels, as now; (c) three locks.

Recommendation: (a). It is what R894 says and what the capability ceiling needs: a test-only package must
not widen the capability set a dependent sees (R636). A package in two subgraphs resolves to the maximum
of its minimums across them, so the lock holds one version per (name, major), and `kinds` records which
subgraphs use it. (c) is rejected because it splits the one source of truth for versions.

- Decision: as recommended; Antonio, 2026-11-08
- Row: R951

## Spec changes

19.1: lock fields and order (D3), config syntax (D1), `kinds` (D9). 19.3: HTTPS index behavior (D4), the
store layout and archive rejection (D2), `.info` `fixes` and `malicious` (D5, D7). 19.4: gate behavior
before M2b (D5), `approve` and `vendor` (D6), bypass printing (D7), "bundles referenced from `yan.lock`"
(D3). 18.1: no change. Overrides (D8) edit 19.1's Resolving paragraph. New code (D2): claimed with
`tools/rows.py next-code` in the writing PR, not here. Conformance cases for each
accepted item are proposed in a separate `conform/*` PR.
