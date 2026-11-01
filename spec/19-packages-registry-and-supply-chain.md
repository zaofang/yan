## 19. Packages, registry and supply chain

The design behind this section is the Yong supply-chain security design (https://claude.ai/artifact/XcMuaSWJuuHdxNPs3R6668).

### 19.1 Manifest and lockfile

`package.xin` holds one constant, `package: Package = Package(...)`, built from literals only, so tools read it without running code. A single `.xin` file may define the same constant and is then a runnable script whose dependencies resolve into the global store and which runs under its declared ceiling.

| Field | Meaning |
| --- | --- |
| `name`, `version` | `@org/name` and a semver version; in 0.y.z, y is the major |
| `edition` | the language edition for every `.xin` and `.wen` file of the package (20.4) |
| `yong` | the required toolchain and std API version, `"1.x"` (20.3) |
| `dependencies` | name to minimum version; each major is a distinct package path |
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

Optional features and conditional dependencies are a non-goal for 1.0; platform variation goes through platform bundles and capability handlers.

**Version selection** is minimal version selection over minimum requirements (SQ30). A plain build never selects new versions. Package identity is (name, major), so majors coexist. A dependency is public when its types appear in a `pub` signature (inferred).

**`yong.lock`** records, per package: version, archive digest (SHA-256), computed capability digest, publisher identity, approvals, and rebuild attestations; plus the pinned tzdb, Unicode, CLDR, ISO 4217 and `math_edition` versions and imported C header hashes. It carries a format version, and newer toolchains read every older lock.

### 19.2 Capability ceilings

`capabilities` is a ceiling enforced transitively: using a capability the package did not declare is E314, including through dependencies. At publish the registry computes each version's effective capability set per `pub` item, with attenuations, plus extern libraries, `uses none` claims, `ffi` modules and unsafe items. It goes into the version's `.info`, the log leaf and the provenance, together with each `pub` item's contracts proven by certificates the registry re-checked (R608). The declared ceiling MUST contain it, or publish fails (E006). Approvals and ceilings compare against this computed set.

### 19.3 Registry, index and log

- **Trust roots.** A `yong` release embeds Sigstore's public-good TUF root, the Yong log's public key and the witness set with its quorum. These are the only roots for package verification; the registry runs no TUF repository of its own. Private registries add their log key as a further root.
- **Index.** Static paths shaped like Go's module proxy: `/@org/name/@v/list`, `/@v/<ver>.info` (archive digest, publisher identity, computed capability set), `/@v/<ver>.zip`. Mirrors serve the index untrusted, since clients verify against the log.
- **Witnessed transparency log.** C2SP tlog-tiles with signed-note checkpoints. A client accepts a checkpoint only with the log signature and cosignatures from the witness quorum (2 of 3 at launch). On first fetch of a version, `yong` checks inclusion of (name, version, archive digest, capability digest) and consistency with the last checkpoint it stored in `.yong/` (E011 on failure).
- **Version archive.** A published version is a deterministic zip built by the registry from the tagged commit: exactly the files the module graph reads, plus LICENSE, README and declared fixtures (invisible to builds). Entries are sorted, timestamps and modes fixed, and anyone can rebuild it byte for byte. Its SHA-256 is the version's identity in the index, log and `yong.lock`. Packages have no install scripts or build scripts; stage-0 evaluation is hermetic.
- **Build provenance.** The registry builder emits SLSA v1 provenance (in-toto Statement, DSSE, Sigstore bundle logged in Rekor) at Build L3: source repo, commit, tag, publisher OIDC identity (repo, workflow, ref), toolchain digest, archive digest and computed capability digest. Builds run fresh and without network. SLSA v1.2 Source-track facts are recorded, not enforced.
- **Publishing credentials.** Publishing accepts only a short-lived OIDC token from a trusted-publisher binding scoped to one package; no API token type exists. Namespace administration needs a passkey (WebAuthn). A namespace MAY require two-person publish: the version stays staged until a second maintainer confirms with a passkey.
- **Namespaces and names.** `@org` is bound at creation to a GitHub user or organization (via OIDC) or a DNS domain (`_yong` TXT record), rechecked on binding changes. Names are ASCII and lowercase; `-` and `_` fold. A new name whose UTS #39 confusable skeleton or edit distance 2 matches a top-1000 name goes to review.
- **Yank, malicious marks and deletion.** A yanked version still resolves from existing lockfiles but is never newly selected. The registry can mark a version malicious by a log entry; clients refuse it even when `yong.lock` pins it (E007), print the advisory, and `yong update` moves to the nearest clean version. The archive is quarantined, not deleted. Legal deletion is separate.

### 19.4 Adding and updating dependencies

- **Nonexistent names.** `yong add` on a name that does not exist fails with the nearest real names (E003). The registry reserves names that its quarterly probes of the M1 evaluation models hallucinate often.
- **Agent gate.** Through the `toolchain` capability an agent may add only a package already in `yong.lock` or established (published 30 days or more and depended on by at least 10 packages); otherwise the add waits for an approval (E004). The MCP `query` tool exposes index search.
- **Cool-off.** `yong add` and `yong update` skip versions younger than `min_release_age` (default 3 days, all packages), unless the version fixes an advisory affecting the current lock or a human pins it exactly (`@=x.y.z`).
- **Identity pinning.** An update whose publisher identity differs from the one in `yong.lock` blocks like a capability gain (E005). `yong audit --diff` shows both identities.
- **Signed approvals.** A capability gain, an identity change or a gated agent add needs `yong approve`: an in-toto statement accepting (package, version, archive digest, capability digest, identity), signed with Sigstore keyless signing after the human's interactive OIDC login. Bundles live in `yong.lock`. CI verifies each signer against `approvers` in the root `package.xin` (E009); the `toolchain` capability cannot produce one.
- **Shared audits.** An approval is also an audit with criteria `safe-to-run` or `safe-to-deploy`. `trust = [...]` imports another party's approvals as satisfying gates. std and `@yong/*` ship with team audits.
- **Advisories and reachability.** Advisories are OSV records (ecosystem `Yong`) in a git repository, exported to osv.dev, naming affected version ranges and item paths. `yong audit` marks an advisory as affecting the build only when an affected item is reachable in the query database's call graph; others are listed as present, not reachable.
- **Registries, mirrors and offline builds.** User or CI configuration maps namespaces to registries exactly; a mapped namespace never falls back to the public registry. `yong vendor` copies archives and bundles into the repository; `--offline --locked` builds verify from `yong.lock` and `.yong/` with no network.

### 19.5 Reproducible builds and rebuilders

- **Hermetic, reproducible builds.** Release and registry builds are bit-reproducible across machines: remapped paths, commit or zero timestamps, sorted inputs, codegen independent of thread count, the bundled linker and libc stubs. All C sources in packages compile with a clang pinned by the toolchain, fetched on first need; the user's `cc` is never used. `yong build --verify` builds twice in different roots and fails on any difference (E010). Provenance records toolchain, clang and linker digests.
- **Independent rebuilders.** `yong rebuild pkg@v` reproduces the archive and build from the tagged commit and, on a byte match, signs and logs a rebuild attestation. `rebuilders = k` requires k independent rebuilders besides the registry (default 0). The project runs one rebuilder outside registry infrastructure for std and `@yong/*`.
- **SBOM.** `yong sbom` writes CycloneDX 1.7 only, from `yong.lock` and the query database: components with archive digests, provenance and rebuild references, computed capability sets as properties, and VEX (`code_not_reachable`) from the reachability analysis.

### 19.6 Toolchain supply chain and hashes

The toolchain's Rust dependencies are pinned, vendored for releases and gated by cargo-vet with imported audits. Release binaries are reproducible, and `yong self update` requires matching signatures from the release build and one independent rebuilder. Each release checks that item hashes for std and the conformance corpus are equal on Linux, macOS and Windows.

External artifact identities (archives, in-toto subjects, log leaves, SBOM) use SHA-256; BLAKE3 stays the internal item and cache hash. Signatures are those Sigstore issues. Checkpoints are signed notes that can carry an ML-DSA cosignature beside Ed25519 without a format change, adopted when Sigstore's post-quantum path ships.
