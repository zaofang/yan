# Package manager crate (`yan-pkg`) design

Issue #481. Blocks #477 (R798). Milestone: M1 (spec 18 command table lists `yan add`, `remove`, `update`, `tree`, `why`, `vendor` as M1; the registry is M2b and R636 gives M1 a static index). Not post-M1.

## 1. Scope

In: resolver (MVS), `yan.lock` read/write, static-index client, global archive store, `yan add` and `yan update` (and `remove`, which is the same edit in reverse).
Out of this issue: registry, log and witness verification (M2b), `yan approve`, identity and capability gates (E004, E005, E009), `yan audit`, `yan vendor`, Python resolution (`pylock.toml`, R638), `yan publish`. Each stays a later issue; the lock format reserves their fields (19.1, R647).

## 2. Crate layout

New `crates/yan-pkg`, no dependency on elab, check or the interpreter; `yan-cli` depends on it.

| Module | Job |
| --- | --- |
| `req` | requirement `"@org/name x.y[.z]"`, versions, (name, major) identity, pre-release rule (R642, R643) |
| `manifest` | the dependency fields of `package.yag` (`dependencies`, `dev_dependencies`, `tools`, `overrides`, `exclude`, `min_release_age`), R635 local names. Reads through the existing literal-only manifest reader in `yan-db`; edits for `add`/`remove` are textual and keep the file otherwise unchanged |
| `lock` | `yan.lock` as TOML (build-system study D7): format version, sorted tables, one per (name, major), canonical output, newer toolchains read older locks |
| `index` | trait `Index { versions(pkg), info(pkg, ver), archive(pkg, ver) }`; `DirIndex` and `HttpIndex` over the Go-proxy layout of 19.3 |
| `store` | global store of SHA-256-addressed archives, digest verified on write and read |
| `resolve` | minimum version selection (SQ30) over the index; plain build never selects new versions; `exclude`, yank and malicious filtering |
| `policy` | cool-off (R798): given a candidate, the lock and `now`, accepts or skips it |
| `ops` | `add`, `remove`, `update` as pure functions from (manifest, lock, index, clock) to (manifest edit, new lock); the CLI only does I/O |

Time enters only through a `Clock` argument so the policy is testable without sleeping.

`yan-cli` gets `add`, `remove`, `update` subcommands in a new `pkg.rs`, with E003 (nearest names), E007 (malicious pinned) and the R644 yank message wired to `yan-diag`. No new diagnostic codes are needed for this slice.

## 3. What R798 (#477) needs from it

1. `policy::accept(candidate, ctx) -> Accept | Skip(reason)` with `ctx` = {`min_release_age` (root manifest, default 7 days), `now`, locked entry, advisory set, exact-pin flag}. #477 implements the body; this issue lands the call site in `resolve` for `add` and `update` and a stub that accepts everything.
2. Per version, from the index: publish time, yanked (with reason), malicious. Spec 19.3 `.info` lists digest, publisher identity and capability set only. Publish time and the yank/malicious marks have no specified home in the static index. This is a spec gap (decision C below).
3. "Fixes an advisory affecting the current lock" needs an advisory source. Advisories are OSV in a git repository (19.4). For M1 the policy takes an `AdvisorySet` that is empty until `yan audit` exists; the exemption is implemented against the trait, not data.
4. "Nearest clean version" is the same resolver call with the current version excluded; it already exists for `update` after a yank.

## 4. Order of work

1. `req`, `lock`, `manifest` with unit tests (no I/O).
2. `index` (`DirIndex` first) and `store`.
3. `resolve` + `ops`, tested against a fixture directory index built in the test.
4. `yan-cli` subcommands; `HttpIndex`.
5. Acceptance: fmt, clippy, cargo test, M0 43/43, M1 439/439, `rows.py check`. No conformance edits; conformance cases for `yan add` are a separate path-guarded PR.

## 5. Open choices (question cards)

A. HTTP client for `HttpIndex`: `ureq` (small, blocking, rustls) versus `reqwest` (async, large). Recommend `ureq`: the index is static GETs and the CLI is not async.
B. Archive handling: add `sha2` and `zip` crates versus a hand-written reader. Recommend the crates; the archive is a deterministic zip and verifying SHA-256 is security-critical code we should not write.
C. Index metadata for R798: add `published` (RFC 3339), `yanked` (reason or null) and `malicious` to `<ver>.info`, which the registry will also serve. Needs a spec edit to 19.3 and a row; Antonio's go-ahead and a separate spec PR. Recommend yes, in the spec PR that precedes #477. Alternative: derive nothing and make R798 wait for M2b.
D. Slice: also implement the identity-pinning and capability-gain blocks (E005, R673) in this issue, or only resolve, lock and cool-off. Recommend only resolve, lock and cool-off; gates need computed capability digests from the registry, which do not exist before M2b.

Decisions A and B are implementation choices Antonio may override; C and D change scope.
