# Public-release audit of atbraz/yan-lang

Audit only; visibility unchanged. Commit `1cc4e95` (main, 2026-11-10), run 2026-11-11. Tracking issue
under #449; going-public steps in #982. Not legal advice.

Scope scanned: every object reachable from all 28 branches and 620 `refs/pull/*/head` refs
(1,705 commits, 14,416 objects; no tags exist), all commit messages, and the text of 997 issues
and PRs plus 369 issue comments. PR review comments: none. Secret values are never printed here.

## Verdict

No secret, credential or private key is in any reachable object, commit message, issue or PR.
No history rewrite is needed. The repository cannot go public yet because of CI: every workflow that
a pull request triggers runs on non-ephemeral self-hosted runners, so the first fork PR would run
its code on quant-dev and the Mac. Three other blockers are the lead's own preconditions.

## Blockers (before the flip)

| # | Finding | Evidence | Fix |
| --- | --- | --- | --- |
| B1 | Fork PRs would run on self-hosted runners | `ci.yml` (8 jobs), `site.yml` build (triggered by `pull_request`), `merged.yml`, `rerun-infra.yml` all `runs-on: self-hosted`; runners are non-ephemeral with warm `target/` and cargo home, and quant-dev is shared with other projects | Move every job to GitHub-hosted runners and remove the self-hosted runners from the repo before the flip (#423, #461, #462). Then set Settings, Actions, "Require approval for all external contributors" |
| B2 | Hosted runners do not start on this account | #973; `release.yml` already targets hosted images and has never run | Confirm a hosted job starts (the flip itself may fix the billing block; test on a throwaway public repo first). Without it B1 leaves the repo with no CI |
| B3 | Employment IP and trademark clearance | Lead card 2026-10-31: go public after both; #982 step 3 | Lawyer |
| B4 | README promises a trademark policy before going public | `README.md` "Licensing": "the trademark policy will be published before the repository goes public" | Publish `TRADEMARKS.md` (R364 to R366), or drop the sentence |

## Should fix (before or right after the flip)

| # | Finding | Evidence | Fix |
| --- | --- | --- | --- |
| S1 | LGPL-3.0-only code is statically linked into the `yan` binary | `malachite`, `malachite-base`, `-nz`, `-q`, `malachite-bigint` reach `yan-cli` through `rustpython-parser 0.4` and `rustpython-ast`. A static Rust binary under LGPL-3.0 owes users the means to relink, which release archives do not give | Build `rustpython-parser` with `default-features = false` and its `num-bigint` feature (check the feature names for 0.4), or drop the dependency; then add `cargo-deny` with a license allowlist (#975) |
| S2 | Release archives carry no third-party notices | Archive has `yan`, `libyan_rt.a`, `LICENSE` only; 302 crates, mostly MIT/Apache, which require their notices on binary redistribution; `webpki-roots` is CDLA-Permissive-2.0 | Generate `THIRD_PARTY.md` (e.g. `cargo-about`) in `tools/release.py package` |
| S3 | Licences named in README are not in the repo | README names CC-BY-4.0 (spec, docs) and 0BSD (examples, templates) but only `LICENSE` (Apache-2.0 WITH LLVM-exception) exists; brand assets are all rights reserved by LG8 (R357) but `site/brand/` says nothing | Add `LICENSES/CC-BY-4.0.txt`, `LICENSES/0BSD.txt` and a `site/brand/README.md` stating the reservation (or adopt REUSE) |
| S4 | Logo glyph provenance is unrecorded | The brand build scripts (project files `brand/src/*.py`) reference Noto Serif SC. OFL-1.1 permits glyph outlines in artwork, but a mark filed under R364 should have a written provenance | Record the source font and version; raise with the trademark lawyer (B3) |
| S5 | About 220 claude.ai artifact links in 55 tracked files are private | `docs/research/README.md` (most of the index), `docs/studies/*`, 14 `rfcs/*`, `spec/26-*`, `docs/development-laws.md` | Export the studies into `docs/research/` (overlaps #976 backups), or mark the links as private; public readers get 404s |
| S6 | Personal email addresses become public | Commit author on 576 commits (two personal domains); the GitHub noreply address is not used | Accept, because a rewrite would change every SHA cited in about 1,400 issues and PRs, and GitHub keeps `refs/pull/*` regardless. Turn on "Keep my email addresses private" and "Block command line pushes that expose my email" for new commits |
| S7 | Issue and PR text goes public with the repo | No secrets. Two comments carry local home paths (#334, #890); runner hostnames in #332, #341, #850, #978, #998, comment on #849; hosting prices in a #978 comment; "lawyer on employment IP" in #5, #390, #982 | Edit the employment-IP mentions before the flip (they advertise a possible claim); the rest is low risk |
| S8 | Actions pinned by tag, not SHA | 37 `uses:` lines across the 5 workflows and `.github/actions/cleanup` | Pin to SHAs and enable Dependabot for Actions (#975) |
| S9 | README "Required repository settings" is out of date | Tells readers to register a self-hosted runner; lists `conformance` as a required check, folded into `rust` | Rewrite with B1 |
| S10 | DCO is required by CONTRIBUTING but not checked | 8 of 1,705 commits carry `Signed-off-by` | Add a DCO check before accepting outside PRs |
| S11 | Hard-coded `atbraz/yan-lang` and the tap | `README.md`, `site/index.md`, `dist/install.sh`, `tools/release.py`, `release.yml` (`atbraz/homebrew-yan`) | Update in the transfer PR; GitHub redirects git and web URLs but the install script should name the new owner |

## Nice to have

| # | Finding | Fix |
| --- | --- | --- |
| N1 | No issue or PR templates | Add `.github/ISSUE_TEMPLATE/` (bug, feature, spec question) and a PR template with the stage line from CLAUDE.md |
| N2 | Playground loads Google Fonts from Google | Self-host the OFL fonts (no third-party requests from the playground) |
| N3 | 27 stale remote branches (`claude/*`, `conform/*`), several with retired names | Delete merged branches before the flip |
| N4 | Platform protections are off on a private Free repo | After the flip: secret scanning with push protection, Dependabot alerts, private vulnerability reporting (SECURITY.md points to it; #389), ruleset on `main` (#974) |
| N5 | gitleaks was not run | Binary download was blocked in the audit environment; the custom scan below covers the gitleaks core rules. Add a gitleaks job to CI once hosted runners work |
| N6 | `merged.yml` on fork PRs gets a read-only token | Its comment step will fail on fork PRs. It is injection-safe (title and body go through `env`); leave it or skip forks |

## Checked and clean

- Secrets, full history: Anthropic, OpenAI, GitHub, AWS, GCP, Slack, Stripe, Hugging Face, npm,
  DigitalOcean, Cloudflare, JWT, PEM private keys, generic `key/secret/token/password = "..."`,
  credentials in URLs, healthchecks ping URLs. One hit: `release.yml` line 115 builds a clone URL from
  `$TAP_TOKEN`, which comes from `secrets.HOMEBREW_TAP_TOKEN`; no literal value.
- `ANTHROPIC_API_KEY` appears only as an environment variable name in `bench/r567/`. No `OPENAI_*` key
  or value anywhere.
- No `.env`, key, certificate, `.netrc`, `.npmrc` or `.pypirc` file was ever committed. No blob over 2 MB.
- No IP addresses (other than loopback in prose), no home-directory paths, no healthcheck URLs, no
  provider account details in tracked files. `quant-dev` is named in
  `docs/research/2026-11-10-infrastructure-gaps.md` (hostname and size only; acceptable).
- Commit messages: 1,252 `Claude-Session` trailers link private claude.ai sessions. They are opaque
  links that only the owner can open. Leave them.
- `pull_request_target`: not used. No workflow passes secrets to a `pull_request` job. Untrusted PR
  fields are passed through `env`, never interpolated into `run:`.
- Workspace crates declare `Apache-2.0 WITH LLVM-exception`. All other dependency licences are
  permissive except S1.
- `SECURITY.md`, `CONTRIBUTING.md` (AI disclosure, DCO), `CODE_OF_CONDUCT.md` (Contributor Covenant),
  `LICENSE`, `CODEOWNERS` are present.
- Held-out evaluation tests are in a separate private repository; nothing from it is in this one.
- `CLAUDE.md`, `docs/automation.md`, `docs/development-laws.md` describe the agent workflow. They contain no
  secrets and match the AI policy in CONTRIBUTING, so they can ship.

## History rewrite

Not needed. If a secret is ever found, rotate it first. A rewrite is a backstop, not a fix. Then:
`git filter-repo --replace-text` (or `--invert-paths`) on a fresh mirror clone, force-push every
branch, and ask GitHub Support to purge cached views and `refs/pull/*`, which a push cannot change.
Forks and clones made while the repo was public keep the old objects.

## Method

`git fetch` of all branches and `refs/pull/*/head`; a Python scan over every blob from
`git rev-list --all --objects` and over `git log --all --format=%B`, using gitleaks-style regexes plus
personal-data patterns (emails, home paths, IPv4, Tailscale, claude.ai session, project and environment
IDs). Issues, PRs and comments were fetched with the REST API and run through the same patterns.
Licences come from `cargo metadata --locked`; `cargo tree -i malachite` gives the dependency path.
The Actions settings API is not reachable from the audit environment, so the fork-PR approval
setting is not verified.
