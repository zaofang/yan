## 19. Packages, registry and supply chain

The design behind this section is the Yan supply-chain security design (https://claude.ai/artifact/XcMuaSWJuuHdxNPs3R6668).

### 19.1 Manifest and lockfile

`package.yag` holds one constant, `package: Package = Package(...)`, and is a data file (17.3, R829): built from literals only, so tools read it without running code, and a violation is E144. A single `.yag` file may define the same constant and is then a runnable script whose dependencies resolve into the global store and which runs under its declared ceiling. The optional `lints` field is a `Lints(...)` literal (18.2, R819).

| Field | Meaning |
| --- | --- |
| `name`, `version` | `@org/name` and a semver version; in 0.y.z, y is the major |
| `edition` | the language edition for every `.yag` and `.yn` file of the package (20.4) |
| `yan` | the required toolchain and std API version, `"1.x"` (20.3) |
| `dependencies` | local root name to requirement `"@org/name x.y[.z]"` (R635, R642); each major is a distinct package |
| `dev_dependencies` | as `dependencies`, for tests, fixtures and benches only (R636) |
| `tools` | packages `yan x` runs, each under its own ceiling (R636) |
| `overrides` | root only: a package replaced by a local path or a git commit with its archive digest (R636) |
| `python` | `Python(requires=..., dependencies=[...])`, the Python packages `py` calls (R638) |
| `exclude` | versions version selection must skip |
| `capabilities` | the capability ceiling, with `net.only` patterns and foreign-library capabilities (7.4) |
| `platform` | a capability bundle with default handlers (`cli`, `service`, `job`, `wasm`, `freestanding`); sets the ceiling, never replaces `uses` |
| `allow_ffi` | packages allowed to contain extern blocks |
| `native` | static or dynamic linking per target; C sources (12.8) |
| `preview` | opted-in preview names (20.6); forbidden in published packages at 1.0 or later |
| `members` | workspace members, sharing one lock and cache (root only) |
| `approvers`, `trust`, `rebuilders`, `min_release_age` | supply-chain policy (19.4, 19.5; root only) |
| `license`, `license_policy` | the SPDX license expression; an allow or deny list enforced over the dependency graph (26.5, R356) |
| `profile`, `embedded` | a named profile such as `"embedded"`; chip, memory map and stack reserve (17.1, R320, R326) |
| `i18n` | shipped locales and data keys, `I18n(locales=[...], data=[...])` (17.5, R333) |

**Configuration (R943).** `.yan/config.yag` is one `config: Config = Config(...)` constant (a data file) with fields `registries` (namespace to index), `min_release_age` (integer days, default 7), `trust` (audit sources, 19.4), `offline` (bool, default false) and `store` (store path, 19.3); a field outside this list is E018. The workspace file is read first, then the user's (`$XDG_CONFIG_HOME/yan/config.yag`); per key the workspace wins and `registries` merge by namespace, except that a workspace `min_release_age` below the user's is ignored.

**Dependency names (R635).** Each `dependencies` key is the local root name by which the package's code refers to that dependency: `"http": "@acme/http 1.4"` makes `http.get(...)` call `@acme/http`. `dependencies` may also be a list of requirements; each local name is then the package name's last segment. Two majors of one package, or two packages with the same last segment, need explicit names. A local name may not be `std`, `core`, `alloc` or a prelude name. Item hashes use the full package identity, never the local name.

**Requirements (R642).** A requirement is `"@org/name x.y[.z]"`. The major (the minor in 0.y.z) is part of the identity; the rest is the minimum version selection starts from.

**Dependency kinds (R636).** `dev_dependencies` are visible only to tests, fixtures, benches and doc fences of the package itself; they are outside its capability ceiling and invisible to its dependents. `tools` are packages run by `yan x`, each under its own ceiling. `overrides` (root only) replace a package anywhere in the graph with a local path or a git commit pinned by its archive digest, for fixing a dependency before a release. There are no other git or path dependencies; multi-package work uses `members`.

**Pre-releases (R643).** A pre-release version is selected only when a requirement names a pre-release of the same version. A published package at 1.0 or later MUST NOT depend on a pre-release.

**Python dependencies (R638).** Libraries declare the Python packages their `py` calls need in `python`; a package with a `python` field must list `py` in its ceiling. The root resolves one Python environment for the whole graph with uv into `pylock.toml` (PEP 751), and `yan.lock` pins that file's digest. Conflicting requirements between packages are a resolve error naming both. Only wheels are installed; no source distribution is built.

Optional features and conditional dependencies are a non-goal for 1.0; platform variation goes through platform bundles and capability handlers.

**Version selection** is minimal version selection over minimum requirements (SQ30). A plain build never selects new versions. Package identity is (name, major), so majors coexist. A dependency is public when its types appear in a `pub` signature (inferred).

**Manifest rules (R890, R891).** The manifest is `package.yag`; no other name is read. A field that is not in the table above is E018, and so is a missing `name`, `version`, `edition` or `yan` on a package that is published or depended on. A root that sets `members` and publishes nothing, and a single-file script, may omit them.

**`yan.lock`** (fields and order in R945) records, per package: version, archive digest (SHA-256), computed capability digest, publisher identity, approvals, and rebuild attestations; plus the pinned tzdb, Unicode, CLDR, ISO 4217 and `math_edition` versions and imported C header hashes; and, as optional fields (R647), the `pylock.toml` digest and the index URL a package resolved from.

**Package table (R945).** A `[[package]]` has fields in the order `name`, `version`, `source`, `digest`, `capability_digest`, `publisher`, `approvals`, `rebuilds`, then `kinds` last. `approvals` and `rebuilds` hold bundle digests; the bundles live in `.yan/bundles/` and `vendor/bundles/`, not in the lock. `source` is `"override"` for an override, and the record then carries `path` after `publisher`: the directory, or `git+<url>@<commit>` with the archive digest in `digest` (R950). `kinds` lists the subgraphs that use the package (R951).

**Lock file (R892, R893).** `yan.lock` is TOML, written canonically: a top-level `format` integer first, then the pinned-data tables, then one `[[package]]` table per (name, major) sorted by name and then major, with fields in a fixed order and one field per line, so diffs and merge conflicts are line-local. Newer toolchains read every older `format`. A toolchain writes its own `format` only when it changes the lock for another reason; reading never writes. A lock whose `format` is newer than the toolchain knows is E019, naming the toolchain version that reads it.

**Resolving (R894).** The resolver reads each candidate version's `.info` (19.3), which lists its requirements, and fetches no archive until the graph is chosen. It applies `overrides` first, then selects `dependencies`, `dev_dependencies` and `tools` as three subgraphs of one lock, then resolves the Python environment (R638). An override replaces only its (name, major) (R950): its key is `"@org/name major"` and its value a directory relative to the root or `"git+https://host/repo@<40-hex commit> sha256:<hex>"`, whose `git archive --format=zip` output must hash to the digest (E020) and is kept in the store. A package in several subgraphs resolves to the maximum of its minimums, one version per (name, major), and its `kinds` entry records the subgraphs `dep`, `dev` and `tool` (R951). The Python environment is `uv`'s; the lock records only the `pylock.toml` digest. `yan add` writes, as the requirement's minimum, the newest version outside the cool-off window (19.4) that satisfies the user's text; selection then stays minimal version selection over those minimums, so a rebuild never moves and the cool-off applies only to what `add` and `update` write. A conflict names both requirers.

**When the lock is written (R896).** Only `yan add`, `remove`, `update`, `approve`, `vendor` and `yan lock` write `yan.lock`. `build`, `check` and `test` read it and never write; under `--locked` a lock that does not match the manifest, or is missing for a package with dependencies, is E020. Without `--locked` they resolve from the lock and, for a name it lacks, from the manifest's minimums, without writing. `--offline --locked` additionally fails when a pinned archive is in neither `.yan/` nor the vendor directory.

### 19.2 Capability ceilings

`capabilities` is a ceiling enforced transitively: using a capability the package did not declare is E314, including through dependencies. At publish the registry computes each version's effective capability set per `pub` item, with attenuations, plus extern libraries, `uses none` claims, `ffi` modules and unsafe items. It goes into the version's `.info`, the log leaf and the provenance, together with each `pub` item's contracts proven by certificates the registry re-checked (R608). The declared ceiling MUST contain it, or publish fails (E006). A ceiling that lists an unconfined capability (7.1) is shown as granting every capability (R673). Approvals and ceilings compare against this computed set.

### 19.3 Registry, index and log

- **Trust roots.** A `yan` release embeds Sigstore's public-good TUF root, the Yan log's public key and the witness set with its quorum. These are the only roots for package verification; the registry runs no TUF repository of its own. Private registries add their log key as a further root.
- **Index.** Static paths shaped like Go's module proxy: `/@org/name/@v/list`, `/@v/<ver>.info`, `/@v/<ver>.zip`. A `.info` is JSON with the archive digest, publisher identity, computed capability set, requirements, yank state and publish time (R895); the log leaf later hashes the same fields, and the cool-off reads the publish time before any log exists. A `registries` value is a directory, an `https://` URL or an `http://` URL whose host is loopback (R946); redirects stay on the same host over HTTPS, `list` and `.info` bodies are limited to 1 MiB and archives to 256 MiB, and the archive digest is checked against `.info` and then `yan.lock`. A `.info` also lists the advisory ids a version fixes and affects, and a `malicious` flag (R949, R947). Mirrors serve the index untrusted, since clients verify against the log. **Discovery (R895).** A namespace maps to an index, a URL or a directory, in a `registries` table of the configuration data file `.yan/config.yag` at user or workspace level; never in a manifest, so a dependency cannot redirect a namespace. A namespace with no mapping fails with E021 naming the key.
- **Witnessed transparency log.** C2SP tlog-tiles with signed-note checkpoints. A client accepts a checkpoint only with the log signature and cosignatures from the witness quorum (2 of 3 at launch). On first fetch of a version, `yan` checks inclusion of (name, version, archive digest, capability digest) and consistency with the last checkpoint it stored in `.yan/` (E011 on failure).
- **Version archive.** A published version is a deterministic zip built by the registry from the tagged commit: exactly the files the module graph reads, plus LICENSE, README and declared fixtures (invisible to builds). Entries are sorted, timestamps and modes fixed, and anyone can rebuild it byte for byte. Its SHA-256 is the version's identity in the index, log and `yan.lock`. Packages have no install scripts or build scripts; stage-0 evaluation is hermetic. An archive contains no symbolic links (R677).
- **Build provenance.** The registry builder emits SLSA v1 provenance (in-toto Statement, DSSE, Sigstore bundle logged in Rekor) at Build L3: source repo, commit, tag, publisher OIDC identity (repo, workflow, ref), toolchain digest, archive digest and computed capability digest. Builds run fresh and without network. SLSA v1.2 Source-track facts are recorded, not enforced.
- **Publishing credentials.** Publishing accepts only a short-lived OIDC token from a trusted-publisher binding scoped to one package; no API token type exists. Namespace administration needs a passkey (WebAuthn). A namespace MAY require two-person publish: the version stays staged until a second maintainer confirms with a passkey.
- **Namespaces and names.** `@org` is bound at creation to a GitHub user or organization (via OIDC) or a DNS domain (`_yong` TXT record), rechecked on binding changes. Names are ASCII and lowercase; `-` and `_` fold. A new name whose UTS #39 confusable skeleton or edit distance 2 matches a top-1000 name goes to review.
- **Yank, malicious marks and deletion.** The publisher yanks a version with a reason, which is logged and printed by `yan update` (R644). A yanked version still resolves from existing lockfiles but is never newly selected. The registry can mark a version malicious by a log entry; clients refuse it even when `yan.lock` pins it (E007), including from an unsigned index before the registry exists (R947), print the advisory, and `yan update` moves to the nearest clean version. The archive is quarantined, not deleted. Legal deletion is separate.

- **Store layout (R944).** Archives are content-addressed: `archives/<sha256>.zip` and an unpacked `src/<sha256>/` under the workspace `.yan/store`, `vendor/` or the user store (`store` in the configuration), searched in that order and then the network. Each read re-verifies the SHA-256 against `yan.lock`; unpacking is into a temporary directory and a rename. An archive with a symbolic link, an absolute path, a `..` component or a duplicate path is E022.
- **Search snapshot (R645).** The index also serves `/-/index.jsonl`, rebuilt daily and signed like a checkpoint: each package's name, summary, latest version per major, computed capability set, publish dates and dependents count. It is mirrorable. E003's nearest names, the agent gate and the MCP `query` tool search a local copy, offline.
- **Operations (R637).** The log runs on Tessera over object storage. Cosignatures come from the public transparency.dev witness network under the quorum policy above, not from witnesses the project runs. The registry builder is one sandboxed job per publish that builds the archive, runs `yan check` to compute the capability set and signs the provenance. The rebuilder for std and `@yan/*` (19.5) runs on GitHub Actions. Name review is automatic rejection plus a queue the lead reviews weekly.
- **Before the registry (R636).** Until the registry ships (M2b), `yan add` and `update` resolve against a static index in the layout above, served from a directory or HTTPS and unsigned. `yan.lock` records archive digests from the first resolve, so log verification later needs no lock format change.

### 19.4 Adding and updating dependencies

- **Nonexistent names.** `yan add` on a name that does not exist fails with the nearest real names (E003). The registry reserves names that its quarterly probes of the M1 evaluation models hallucinate often.
- **Agent gate.** Through the `toolchain` capability an agent may add only a package already in `yan.lock` or established (published 30 days or more and depended on by at least 10 packages); otherwise the add waits for an approval (E004). Without a search snapshot nothing is established, so an agent add of an unlocked package is E004; an interactive human add is never gated by E004 (R947). The MCP `query` tool exposes index search.
- **Cool-off.** `yan add` and `yan update` skip versions younger than `min_release_age` (default 7 days, all packages), unless the version fixes an advisory affecting the current lock (named by the `.info` advisory ids, R949; `yan update` prints the bypass and, for each yanked locked version, its reason with control characters stripped, cut at 200 characters), a human pins it exactly (`@=x.y.z`), or the locked version is yanked or marked malicious, in which case the move to the nearest clean version ignores the cool-off (R798).
- **Identity pinning.** An update whose publisher identity differs from the one in `yan.lock` blocks like a capability gain (E005), and so does an update that gains an unconfined capability (R673). `yan audit --diff` shows both identities.
- **Signed approvals.** A capability gain, an identity change or a gated agent add needs `yan approve`: an in-toto statement accepting (package, version, archive digest, capability digest, identity), signed with Sigstore keyless signing after the human's interactive OIDC login. Bundles live beside the lock (R945) and `yan.lock` references them by digest. Until the signing path lands (M2), `yan approve` writes a record with `signer = "local"` that clears local gates and fails `approvers` (E009) (R948). CI verifies each signer against `approvers` in the root `package.yag` (E009); the `toolchain` capability cannot produce one.
- **Shared audits.** An approval is also an audit with criteria `safe-to-run` or `safe-to-deploy`. `trust = [...]` imports another party's approvals as satisfying gates. std and `@yan/*` ship with team audits.
- **Advisories and reachability.** Advisories are OSV records (ecosystem `Yan`) in a git repository, exported to osv.dev, naming affected version ranges and item paths. `yan audit` marks an advisory as affecting the build only when an affected item is reachable in the query database's call graph; others are listed as present, not reachable.
- **Publish refusals (R646).** `yan publish` refuses a manifest with `overrides`, a dev dependency reached from non-test code, or a pre-release dependency at 1.0 or later (E016).
- **Registries, mirrors and offline builds.** User or CI configuration maps namespaces to registries exactly; a mapped namespace never falls back to the public registry. `yan vendor` copies archives and bundles into `vendor/`, verifying each digest and pruning unlocked entries (R948); `--offline --locked` builds verify from `yan.lock` and `.yan/` with no network.

### 19.5 Reproducible builds and rebuilders

- **Hermetic, reproducible builds.** Release and registry builds are bit-reproducible across machines: remapped paths, commit or zero timestamps, sorted inputs, codegen independent of thread count, the bundled linker and libc stubs. All C sources in packages compile with a clang pinned by the toolchain, fetched on first need; the user's `cc` is never used. `yan build --verify` builds twice in different roots and fails on any difference (E010). Provenance records toolchain, clang and linker digests.
- **Independent rebuilders.** `yan rebuild pkg@v` reproduces the archive and build from the tagged commit and, on a byte match, signs and logs a rebuild attestation. `rebuilders = k` requires k independent rebuilders besides the registry (default 0). The project runs one rebuilder outside registry infrastructure for std and `@yan/*`.
- **SBOM.** `yan sbom` writes CycloneDX 1.7 only, from `yan.lock` and the query database: components with archive digests, provenance and rebuild references, computed capability sets as properties, and VEX (`code_not_reachable`) from the reachability analysis.

### 19.6 Toolchain supply chain and hashes

The toolchain's Rust dependencies are pinned, vendored for releases and gated by cargo-vet with imported audits. Release binaries are reproducible, and `yan self update` requires matching signatures from the release build and one independent rebuilder. Each release checks that item hashes for std and the conformance corpus are equal on Linux, macOS and Windows.

External artifact identities (archives, in-toto subjects, log leaves, SBOM) use SHA-256; BLAKE3 stays the internal item and cache hash. Signatures are those Sigstore issues. Checkpoints are signed notes that can carry an ML-DSA cosignature beside Ed25519 without a format change, adopted when Sigstore's post-quantum path ships.
