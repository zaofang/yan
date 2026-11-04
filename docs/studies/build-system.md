# Yan build system: one engine, no build files

Study and proposal, 2026-11-04. Docs only: no spec rows, no conformance edits, no diagnostic codes.
Items are labelled D1, D2 and so on; none carries a row number. Everything here is non-normative until
an RFC and accepted rows say otherwise.

Brief: what a Yan build system should be (manifest, lockfile, content-hash cache from the query
database, hermetic builds, capability-aware dependencies, agent-friendly zero-config), compared with
Cargo, Bazel, Nix, Deno and uv; what to take from the zero-config build sketch
(https://claude.ai/artifact/TM5MAoNw4ESrbCfYGNefFu, Part 2) for Yan packages, and what belongs to a
separate polyglot product. Recommend one design and a minimal first milestone after M1.

## 1. Answer

Yan needs no general build system. The language already removes the reasons build systems exist:
the module graph is the import graph, there are no build scripts, install scripts or proc macros,
and stage 0 is hermetic by construction (10.4: only `alloc`, root-confined `embed` with recorded
hashes, and `diag`). So the build is the query database extended across packages, with a content
cache under it. Actions, sandboxes and depfiles appear only at the one edge where foreign tools run:
C sources compiled by the pinned clang (12.8, 19.5) and the final link.

From the sketch, adopt the cache model (content-addressed keys, action cache, early cutoff, local plus
remote store) and its four oracles. Leave the ecosystem plugins, the cross-language action graph, the
general sandbox, REAPI and remote execution to a separate product if one is ever wanted; for Yan they
cost more than they return.

## 2. What exists today

Specified:

| Piece | Where | State |
| --- | --- | --- |
| `package.yag`, literal-only `Package(...)` constant, single-file scripts | 19.1 | specified; the CLI only finds `package.yag` to list modules (`crates/yan-cli/src/main.rs`) |
| `yan.lock` fields (version, SHA-256 archive digest, capability digest, identity, approvals, pinned data versions, header hashes) | 19.1 | specified; file format open; not implemented |
| Minimal version selection, (name, major) identity, static index before the registry | 19.1, 19.3 | specified; not implemented |
| Capability ceilings enforced transitively | 19.2 | checker enforces E314 within a package |
| Stage-0 hermeticity, `embed` hashes | 10.4 | specified |
| Item hashes: salted (with toolchain salt) and identity (without), positional locals | 3.6 | implemented in `yan-core::hash`, used by `yan-db` |
| Per-item and per-instantiation salsa cache, object fragment per instantiation, remote CAS read by all and written by CI | 18.4 | salsa per file and item in `yan-db`; codegen is Cranelift JIT only, no object files |
| Test verdict cache keyed by item hashes and toolchain hash | 18.4 | implemented, `.yan/cache/verdicts.json` |
| Bit-reproducible release builds, `yan build --verify`, pinned clang, bundled linker | 19.5 | specified; `yan build` is M1b and M2 |
| Global store, reflinks, HTTP CAS remote cache | Toolchain Design doc (https://claude.ai/artifact/1DBzFtmytWANhwwKurTg3i) | design only |

Missing in the spec: where the global store lives and how it is keyed, the cache key of an object
fragment, how the remote cache is addressed and trusted, the lockfile's file format, how C sources
declare headers and flags, and what `yan build` produces when a package has no `main`.

## 3. Prior art

| System | Unit of caching | Hermeticity | Lockfile | What Yan takes | What Yan avoids |
| --- | --- | --- | --- | --- | --- |
| Cargo | crate (fingerprint of sources, flags, deps) | none: `build.rs` and proc macros run arbitrary code | `Cargo.lock`, SAT-style resolver | manifest plus lock, workspaces sharing one lock, `cargo vendor`, offline mode | build scripts, features that fork the graph, crate-level invalidation, mtime fingerprints |
| Bazel and Buck2 | action (argv, env, input digests) | sandbox per action | `MODULE.bazel.lock` | content-addressed action keys, early cutoff, remote cache CI writes, reproducibility culture | hand-written BUILD files, Starlark, REAPI gRPC stack, sandbox cost on every action |
| Nix | derivation (input-addressed, optionally content-addressed) | full: chroot, no network, fixed time | `flake.lock` | pinned toolchains as content-addressed store paths, "fetch once, then offline" | a second language, the store's disk cost, slow evaluation |
| Deno | module (URL plus integrity hash) | runtime permissions, not build sandbox | `deno.lock` with integrity per module | zero config, a global cache keyed by content, permission flags as the security story | URL imports without version selection |
| uv | wheel (content hash) | none needed: wheels are prebuilt | `uv.lock`, universal across platforms | global cache with reflinks or hardlinks, inline script metadata (PEP 723) like Yan scripts, speed as a feature | SAT resolution cost; sdists building at install time (Yan already refuses them, 19.1) |

The pattern: Bazel and Nix earn hermeticity by sandboxing tools that could do anything. Deno and uv
are fast and zero-config because their inputs cannot do anything at build time. Yan is in the second
group by language design, and needs Bazel's cache model without Bazel's sandbox.

## 4. Sketch against Yan

| Sketch component | Yan already has | Decision |
| --- | --- | --- |
| Action (argv, env, inputs, tool) and action key | items and instantiations keyed by salted hash; the toolchain salt covers the tool | adopt for codegen and C only; Yan items are queries, not actions |
| CAS (BLAKE3, chunking, local and remote) | BLAKE3 internal hash (19.6); `.yan/cache/` | adopt; no chunking (fragments are small) |
| Action cache | none for codegen | adopt: key to object digest |
| Early cutoff | salsa backdating; identity hash ignores local names | already free; make it a tested oracle |
| Scheduler on a salsa or Shake DB | `yan-db` is salsa | adopt: one DB across packages |
| Sandbox (namespaces, overlayfs) | stage 0 is hermetic; no build scripts | skip for Yan; for clang, a confined argv and depfile check, namespaces later |
| Pinned toolchains in CAS | pinned clang fetched on first need (19.5) | adopt, store in the global store by SHA-256 |
| REAPI remote cache and execution | HTTP CAS in the Toolchain Design | skip REAPI; plain HTTP GET and PUT by key |
| Discovery (trace reads, `inputs.lock`) | imports are explicit; `embed` records hashes | skip for Yan; clang depfile serves C |
| Ecosystem plugins (Cargo, npm, Python, Go, CMake) | Python via uv into `pylock.toml` (19.1); npm via jco at M4 | separate product |
| Cross-language action graph | not needed | separate product |
| UI, why-rebuilt | `yan why` for item facts; salsa knows what changed | adopt `yan build --why` cheaply (D9), skip the UI |
| Oracles: reproducibility, hermeticity, incrementality, cache soundness | `--verify` covers reproducibility | adopt all four as CI checks on the toolchain (D10) |

## 5. Recommended design

**D1. One engine.** `yan build` is a driver over `yan-db`, like `check`, `run` and `test`. The DB's
inputs become (package root, module text, resolved dependency archive digests, target, profile). No
separate action graph exists for Yan code; the only actions are C compilation and linking, each a
query whose result is a CAS digest.

**D2. Zero configuration.** No build files, ever. A package is its `package.yag` plus the import
graph from it; a single `.yag` file with a `package` constant is a package (19.1). Targets are
inferred: a module with `main` builds a binary named after the package, anything else is a library
checked and cached but not linked. `yan build` with no arguments builds the package in the current
directory; inside a workspace, every member. Overrides stay in `package.yag` (`native`, `platform`,
`profile`); there is no second file.

**D3. Cache keys.**

- Front end: what `yan-db` caches today, per file and per item.
- Object fragment: BLAKE3 of (instantiation's salted hash, the salted hashes of the stage-0 arguments,
  target triple, profile, codegen backend version). The salted hash already contains the toolchain
  salt, so a toolchain change misses cleanly. Early cutoff follows from hashing elaborated Yan Core:
  a comment, a local rename or a surface conversion leaves the key unchanged.
- C object: BLAKE3 of (clang digest, normalized argv, source digest, digest of every header in the
  depfile). Headers outside the package root or the toolchain's sysroot fail the build, the same rule
  as `embed`.
- Link: BLAKE3 of (linker digest, sorted input object digests, link flags).
- Dependency capability digest: computed once per (archive digest, toolchain identity) and stored, so
  checking ceilings across the graph is a lookup.

**D4. Stores.** Two content stores, both BLAKE3 keyed internally:

- `.yan/cache/` per workspace: front-end snapshots, object fragments, verdicts. Disposable.
- A global store (default the platform cache directory, overridable by one environment variable):
  dependency archives by SHA-256 (the external identity, 19.6), unpacked trees, pinned clang and
  linker. Read-only after write; materialized into builds by reflink where the file system has it,
  else hard link, else copy (uv's order). Two workspaces share one copy of every dependency.

**D5. Remote cache.** Plain HTTPS: `GET` and `PUT /v1/<kind>/<key>`, the body the CAS blob, plus
`HEAD` for existence. Everyone reads; only CI writes, with a token outside `package.yag`. A blob read
from the remote is accepted only after its BLAKE3 matches the key, so a mirror or a compromised cache
can cause a miss, never a wrong build. This fits any object store or a static server. No REAPI: it
brings gRPC, a Bazel-shaped action schema and remote execution, none of which a single-language
compiler with millisecond actions needs. A REAPI adapter can be written later outside the toolchain if
someone wants Bazel's caches.

**D6. Hermeticity where it is not free.** Yan code needs no sandbox. C compilation runs the pinned
clang with an argv built only from the `native` table, `-ffile-prefix-map` to the package root, fixed
`SOURCE_DATE_EPOCH`, a scrubbed environment and `-MD` for the depfile; the depfile check of D3 is the
hermeticity proof. Linux namespaces (no network, read-only bind of the package and store) are
hardening for M2, not a prerequisite.

**D7. Lockfile format.** `yan.lock` is machine-written and read by the same literal-only reader as
`package.yag`: one `Lock(...)` constant, one `Locked(...)` record per package sorted by name and
major, canonical printing so diffs are one line per change. One parser, no TOML or JSON dependency,
and the format version is a field. This settles the open format question in 19.1.

**D8. Capability-aware dependencies.** The build graph carries each dependency's computed
capability digest from the lock (19.1). `yan build` checks the root ceiling against the union over
the reachable graph from cached digests, never re-elaborating dependencies to do it, and prints the
dependency chain on E314 using `yan why`. A dependency whose archive digest matches the lock is
trusted from the cache; one that does not is the existing lock errors, not a rebuild.

**D9. Explain.** `yan build --why <artifact>` prints the first key component that differs from the
last build's key (an item hash, a dependency digest, a flag, the toolchain), from the stored keys.
Cheap because keys are already structured; useful to agents because it is one line.

**D10. Oracles in CI.** Four checks run on the toolchain repo against a small corpus (the golden
programs and the conformance packages that build):

1. Reproducibility: `yan build --verify` (two roots, compare digests).
2. Incrementality: apply random item-level patches, compare an incremental build with a clean one.
3. Cache soundness: drop one component from each key kind and expect a miss or a failure, never a
   silent hit.
4. Hermeticity: build C sources with the toolchain sysroot only and the network off.

## 6. Agent fit

Agents get the most from what the design leaves out: there is no build file to write, no target to
name, no flag to remember. `yan build` takes no arguments in the common case, failures are the same
one-line diagnostics as `check`, `--why` answers "why did this rebuild" in one line, and the
`toolchain` capability can run builds but not write the remote cache or the lock's approvals. The
agent gate on `yan add` (19.4) is unchanged.

## 7. Separate product

A zero-config polyglot builder (plugins for Cargo, npm, Python, Go and CMake, a shared action graph,
sandboxed tracing, REAPI) is a different product with a different user. It could reuse D3 to D5 and
the oracles, but it does not help Yan reach M1 or M1b. Recommend not starting it; if it is wanted
later, start it as its own repository that depends on nothing in `yan-*` except possibly a small CAS
crate.

## 8. Milestones and cost

Nothing here belongs before M1, with one exception already in M1's scope: `yan add`, `update`,
`tree`, `why` and `vendor` (18.1) need the manifest reader, MVS over the static index, the global
store and `yan.lock`. That is queue item 10 and the critical path for any build; D4 and D7 should be
decided when it starts so it is built once.

| Step | Contents | Depends on | Rough size |
| --- | --- | --- | --- |
| M1 (existing scope) | manifest reader, static index, MVS, global store (D4, archives only), `yan.lock` in the D7 format | queue item 10 | about one week of agent work |
| B1, first build milestone, inside M1b | `yan build` for one package and workspace members: Cranelift object emission per instantiation, object cache (D3), system linker, `--why` (D9), oracles 1 to 3 | Cranelift object backend (today JIT only) | one to two weeks; object emission is most of it |
| B2 | dependencies from the global store in the build, cached capability digests (D8) | M1 lock, B1 | days |
| B3 | C sources through pinned clang with depfile checks (D3, D6), oracle 4 | pinned clang fetch (19.5) | one week |
| B4 | HTTP remote cache, CI writes (D5) | B1 | days |
| M2 | namespaces hardening, LLVM release builds through the same keys | spec M2 | as spec |

Sizes are guesses from comparable work in this repo, not measurements.

**B1 gate:** a clean build and an incremental build after 100 random item patches produce identical
binaries; `--verify` passes on G1 to G4; a no-op rebuild of G1 to G4 touches no codegen.

## 9. Open questions

- Q1. Should the remote cache also hold front-end snapshots (elaborated modules), or only objects?
  Objects first; front-end snapshots only if cold `check` times on CI measure slow.
- Q2. Library packages: is there ever a reason to emit a linkable `.a` for a Yan library, or only for
  `yan bind` targets (12.6, M2)? This study assumes only for `yan bind`.
- Q3. The global store's eviction: none until measured, or a size cap with least-recently-used
  eviction like uv's `cache prune`.

## 10. Decisions (Antonio, cards, 2026-11-04)

D1, D2, D3, D4, D5, D6, D8, D9 and D10 accepted as recommended. Two answers differ from the
recommendation:

- D7: `yan.lock` is TOML, not a Yan literal. The toolchain carries a TOML reader and writer for the
  lock; the lock stays machine-written and canonical (sorted tables, one package per table).
- Q1: the remote cache holds front-end snapshots (elaborated modules) as well as objects from the
  start, so the snapshot format needs a version field and a key like D3's.

Q2: linkable archives only through `yan bind`. Q3: no store eviction until measured.

These are design decisions for a later RFC; the spec is unchanged until rows are accepted.

## Sources

- Spec 1, 3.6, 10.4, 12.8, 18.1, 18.4, 19.1 to 19.6, 22.2, 23 (M1, M1b).
- `crates/yan-db/src/lib.rs`, `crates/yan-cli/src/test.rs`, `crates/yan-cg-cranelift/src/lib.rs`.
- Zero-config build sketch: https://claude.ai/artifact/TM5MAoNw4ESrbCfYGNefFu (Part 2).
- Yan Toolchain Design: https://claude.ai/artifact/1DBzFtmytWANhwwKurTg3i.
- Cargo book (build scripts, fingerprints), Bazel remote caching and REAPI v2, Nix derivations and
  content-addressed paths, Deno lockfile and permissions, uv cache and lockfile docs.
