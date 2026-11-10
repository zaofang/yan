# Infrastructure gaps: what Yan still needs

Study only. Measured on main at `e8f1f0e`, 2026-11-10. No rows.

The question: what engineering and release infrastructure does the project lack, given one part-time
lead, agent-written code, and a repository that stays private for now.

## What exists

| Area | What is there | Where |
| --- | --- | --- |
| CI | `ci.yml`: changes, guard, canary, check (ruff, mypy, selftests, rows), formal (Lean + differential random testing, 300 generated programs), rust (fmt, clippy, nextest, doctests, M1 conformance gate, laws report, difftest report-only), bench and status on main and nightly. Per-PR cancel, docs-only PRs skip cargo jobs. | `.github/workflows/ci.yml` |
| Runners | Self-hosted only. quant-dev (4 cores, 7.5 GiB, shared with other projects, saturated) plus Mac runners. Non-ephemeral, warm `target/` and cargo home, shared `/tmp`. | #850, #376, #462 |
| Agent governance | `tools/guard.py` path guard with a canary job, CODEOWNERS for lead paths, development laws DL1 to DL16, queue brief format, infra-rerun workflow, merge log on #449. | `docs/development-laws.md`, `docs/automation.md` |
| Release | `release.yml` on `v*` tags: four targets on hosted runners, SBOM (syft), cosign keyless signature, build-provenance and SBOM attestations, installer, Homebrew formula, clean-runner install test. Never run. | #863, gaps in #877 |
| Docs and site | Site generator, tour, blog, playground (wasm), status page data. Pages deploy fails at `configure-pages`. | `site/`, `playground/`, `.github/workflows/site.yml` |
| Benchmarks | Six programs in Yan Core, Python and Rust, wall time, one run, nightly. Callgrind budgets planned. | `bench/`, #350, #759 |
| Planned, not built | Registry and supply chain commands (#378), native fuzz/coverage/profiling tools for users (#379), Windows CI (#367), ephemeral runners (#461, #462). | issues |

## Findings that change the ranking

1. **Hosted runners do not start on this account.** Every `merged.yml` run fails with zero steps and no
   runner (latest four on 2026-11-10, all `failure`). So the merge log on #449 has not been written
   since hosted runs stopped, `rerun-infra.yml` cannot rerun anything, and `release.yml`, which runs
   entirely on hosted runners (`macos-15`, `macos-15-intel`, `ubuntu-24.04`, `ubuntu-24.04-arm`),
   cannot produce a release. The ci.yml comment says the same. The usual cause is the account's
   Actions minutes or spending limit for private repositories; only the account owner can see it
   (Settings, Billing and plans).
2. **No dependency hygiene.** `Cargo.lock` has 296 packages; there is no `cargo-deny`, `cargo-audit`,
   Dependabot or Renovate. Actions are pinned by tag, not commit (`taiki-e/install-action@v2`,
   `anchore/sbom-action@v0`), and the formal job pipes `elan-init.sh` from the `master` branch into
   `sh`. A project whose spec makes supply-chain claims (spec 19) has none of them on itself.
3. **No branch rules visible.** `GET /rulesets` returns `[]`; classic protection is not readable from
   here. CODEOWNERS says "enable Require review from Code Owners", which a personal Free account
   cannot enforce on a private repository. Nothing but convention stops a direct push to `main` or a
   merge with a red `guard`.
4. **No fuzzing or sanitizers on the compiler and runtime.** The formal job's random programs test the
   core semantics, not the parsers, decoders or the runtime. Native drop faults (#963, #964) are the
   class ASan and Miri find cheaply.
5. **Single copy of the project's memory.** Issues are the source of truth, and most studies live only
   as claude.ai artifacts linked from `docs/research/README.md`. Neither is backed up anywhere else.
6. **CI latency.** Over the last nine completed PR runs: median 14 min, max 43 min, wall clock
   including queueing (2026-11-10). Agents wait on this for every PR.

## Ranked list

Cost is monthly unless noted. Effort: S under a day of thread time, M a few threads, L many.

### Needed before 1.0

| # | Item | Why | Cost | Effort | Recommendation |
| --- | --- | --- | --- | --- | --- |
| 1 | Restore hosted runners (plan or billing) | Unblocks release, merge log, infra rerun, Pages, branch rules. Everything below that touches GitHub settings depends on it. | GitHub Pro about $4, plus minutes beyond the plan's quota (macOS minutes count 10x, Linux 1x) | S (Antonio) | Needs Antonio: D1. Until then, move `merged.yml` and `rerun-infra.yml` to `self-hosted` so the log and reruns work today. |
| 2 | Branch ruleset on `main` | Required checks (guard, canary, check, rust, formal), no force push, no deletion, merge commits only, code-owner review on lead paths. Turns the laws from convention into enforcement. | Included in 1 | S (Antonio, admin) | After 1. Required checks must tolerate docs-only PRs (cargo jobs skipped count as success). |
| 3 | Dependency and workflow supply chain | `cargo-deny` (advisories, licences, bans, sources) and `cargo-audit` in `check`; Renovate or Dependabot for Cargo, uv, Actions, `rust-toolchain.toml`, `lean-toolchain`; pin Actions to commit SHAs; pin and checksum `elan-init.sh`; `zizmor` lint of workflows. | 0 | S to M | Do now. Two PRs: tools and config (agent), `.github/` changes (lead merges). |
| 4 | Backups of repo, issues and studies | Nightly `git clone --mirror` plus an export of issues, PRs and comments (JSON) to a second location; export every claude.ai study to Markdown under `docs/studies/` once. | Under $1 (object storage) | S | Do now. Runs on a self-hosted runner on schedule; destination needs D3. |
| 5 | Fuzzing and sanitizers | `cargo-fuzz` targets for each surface parser, the `.yir` reader, `.ybf` decoding and wire decoding; nightly time-boxed run with a stored corpus; ASan on the native runtime tests and Miri on `yan-rt` unsafe code, nightly. Crashes become `conform/*` proposals. ClusterFuzzLite works on private repos; OSS-Fuzz needs public. | 0 on existing runners, needs capacity (item 6) | M | Do before RC. Start with ASan and Miri on `yan-rt` (native drop bugs are open now). |
| 6 | CI capacity | quant-dev is saturated (#850); PR CI median 14 min. One dedicated Linux box (8+ cores, 64 GB) running ephemeral container runners, plus a shared `sccache` so cold agent sessions and CI reuse objects. Doubles as the quiet bench and fuzz machine. | About €40 to €60 for a rented dedicated server | M | Needs Antonio: D2. |
| 7 | Callgrind perf budgets | Law DL6 and the M2a gate need instruction counts, not wall time. Already filed. | 0 | M | Proceed with #350 then #759; no new issue. |
| 8 | Flake tracking | nextest JUnit output uploaded per run; a script that aggregates failures that pass on retry into a ledger on the status page; a quarantine list the lead owns. Today flakes are only noticed by humans. | 0 | S | Do after item 3. |
| 9 | Secret scanning | Secret scanning push protection is a paid add-on on private repos. Free substitute: `gitleaks` in `check` and as an optional pre-commit hook. Matters because agents write files and logs. | 0 | S | Do now with item 3. |
| 10 | Issue templates and a crash path, no telemetry | Telemetry contradicts the deterministic, capability-safe positioning; do not add it. Instead: an internal-compiler-error message that prints version, commit and a `yan bug-report` command that bundles them with the input hash on request; `.github/ISSUE_TEMPLATE/` for bug, ICE, spec question. | 0 | S | Do before RC. |
| 11 | Reproducible dev environment | One definition of the toolchain (Rust 1.97, uv, elan and the pinned Lean, nextest, clang, valgrind) used by CI, cloud agent sessions and contributors: a devcontainer plus a cloud-environment setup script that warms `sccache`. Agent sessions start cold today. | 0 | M | Do before going public. |
| 12 | Hosting while private | Site, playground and status page. GitHub Pages on a private repo needs a paid plan (covered by D1); otherwise Cloudflare Pages by direct upload from CI. | 0 with D1 | S | Follows D1; no separate work if Pro. |
| 13 | Release signing beyond Sigstore | Apple Developer ID for notarization (#877 item 3); Homebrew tap repo and token (#877 item 1); Windows signing when Windows ships. | $99 per year (Apple) | S (Antonio) | Already on Antonio's list; needed for RC. |
| 14 | Independent rebuild | A scheduled job on a second machine rebuilds the latest release from the tag and compares hashes (#877 item 5). Spec 19.5 asks for it. | 0 with item 6 | M | Before RC. |
| 15 | Windows runner | Windows is in M2a (#367). Hosted Windows minutes count 2x; a self-hosted Windows VM is the alternative. | Minutes or a VM, about $20 to $40 | M | Decide at M2a start, after D1 and D2. |
| 16 | Registry hosting (minimal) | Spec 19 registry is M2b (#378): static index plus archives need a domain and object storage with a CDN. Transparency log can start as a static signed log. | Domain about $12 per year; storage under $5 | M | Needs the domain (already on Antonio's list) and D3. |
| 17 | Rust code coverage | `cargo-llvm-cov` nightly, figure on the status page, generated (law 14). Separate from the user-facing coverage tool in #379. | 0 | S | After item 6. |
| 18 | Infra health alerts | Runner offline, disk over 85 %, nightly failed: a scheduled check with a heartbeat service, so a dead runner is seen before a thread stalls on it. | 0 (free tier) | S | After item 6. |

### After 1.0

| Item | Note |
| --- | --- |
| Remote build cache (build-system study milestone B4) | Plain HTTP GET/PUT by key on the same object store as item 16. |
| Transparency log with public witnesses and a rebuilder fleet | Packaging study D3; grows from item 14 and item 16. |
| OSS-Fuzz | Only once the repository is public; replaces ClusterFuzzLite from item 5. |
| Public-repo CI model | Ephemeral runners and approval-gated fork PRs (#423, #461, #462) are a precondition for going public, not for 1.0 if 1.0 ships private. |
| Versioned docs, download CDN and stats, community forum | Launch-time, once there are users. |
| Toolchain distribution on PyPI and crates.io | Packaging study D6. |

### Needs Antonio (money, accounts, admin)

| Item | Action | Cost |
| --- | --- | --- |
| D1 GitHub plan or billing | Upgrade or fix the spending limit; then items 2, 12 | about $4 plus minutes |
| D2 CI box | Rent and register one Linux server | about €40 to €60 |
| D3 Storage and DNS provider | One account for backups, registry, cache, DNS | under $5 |
| Branch ruleset | Configure as in item 2 (admin only) | 0 |
| Apple Developer ID, tap token | Already listed in #877 | $99 per year |
| Domain | Already on the to-do list | about $12 per year |

## Decisions for the lead

- **D1.** Restore hosted runners by moving the account to GitHub Pro (recommended: smallest change,
  brings Pages and branch rules on private repos), by creating the organisation now on Team, or by
  staying Free and running everything, including release, on self-hosted machines.
- **D2.** CI capacity: a rented dedicated Linux server (recommended), paid hosted larger runners, or
  only the one extra runner on quant-dev that #850 allows.
- **D3.** One provider for object storage and DNS: Cloudflare (R2 with no egress fees, DNS, Pages;
  recommended), Backblaze B2, or AWS S3.

Plan and price facts are from GitHub's plan and Actions billing pages as of 2026-11; check them on the
account before buying.
