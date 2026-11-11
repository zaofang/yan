# Runbook: move to the zaofang org, rename the user, go public

Status: draft, nothing executed. Tracking: #982 (under #449). Stage claimed: working (docs and scripts only).
Written 2026-11-11 against main. Org name `zaofang` and new login `amteb` are Antonio's picks; every script takes them as flags.

Target state: repo `zaofang/yan-lang`, public; Antonio's login `amteb`; CI on GitHub-hosted runners; no self-hosted runner reachable from fork PRs.

## Order and why

1. Land open PRs #994-#997 first. They hardcode the `atbraz` login and would be left pointing at a stale name.
2. Preflight and hosted-runner smoke test (nothing irreversible yet).
3. Rewrite author emails out of history (decided, see B0). Before the transfer so no org redirect or runner state has to be redone.
3b. Transfer `atbraz/yan-lang` to `zaofang`. Old URLs redirect.
4. Sweep PR phase 1 (repo slug). Move the Homebrew tap.
5. Machine side (section C).
6. Rename the user `atbraz` -> `amteb`. After the transfer so no repo URL depends on the old login, and after the tap moves so the old name cannot be squatted under a live tap.
7. Sweep PR phase 2 (login).
8. Flip to public. Last, once every precondition is ticked.

Nothing below is reversible for free once public: logs, history, issues and emails become visible and may be cached.

## A. Preconditions for the public flip (checklist)

Run `tools/move/preflight.sh` for the automatable ones.

- [ ] Hosted runners start. Run `tools/move/smoke-hosted.sh --apply` after the org exists: it creates a public `zaofang/runner-smoke`, runs ubuntu/macos/windows, then you delete it. If it fails with zero steps (the #973 signature) it is a billing/account lock; fix with GitHub support before anything else.
- [ ] Full-history secret scan. A regex scan on 2026-11-10 found nothing (1107 commits). Re-run with `gitleaks detect --source . --log-opts=--all --redact` right before the flip (new commits since).
- [ ] Old Actions run logs checked or deleted: logs of past runs become public with the repo.
- [ ] #423 and #461: fork-PR safety. Workflows take `runs-on: ${{ vars.CI_RUNNER || 'ubuntu-latest' }}`: set the variable `CI_RUNNER=self-hosted` before merging the safety PR (private repo, hosted runners down, #973), delete it at the flip. `gh variable list` must show no `CI_RUNNER` when public. Lead merges `.github/`.
- [ ] #462 closed.
- [ ] Lawyer: employment IP check. Trademark search and filing for Yan/Yong (CNIPA, USPTO, EUIPO). LG-decision from 2026-10-31: go public after both.
- [ ] Private vulnerability reporting on (#389); `SECURITY.md` and CODE_OF_CONDUCT contact path match.
- [ ] Commit author emails (~620 commits): DECIDED 2026-11-11 to rewrite history first (B0). Ticked when `rewrite-authors.sh` shows only noreply addresses and GitHub Support has purged the old objects.
- [ ] Settings > Actions > General: fork PR workflows require approval for all outside contributors.
- [ ] Secrets inventory (`gh secret list`): `HOMEBREW_TAP_TOKEN` and any others reviewed; none readable by fork PRs.

## B. GitHub side (Antonio, in order)

0. Rewrite author emails (decided 2026-11-11). Preconditions: #994-#997 merged, every other branch and PR either merged or closed (their commits are not rewritten and would reintroduce the old SHAs), all machines and cloud sessions have no unpushed work. Then: `tools/move/rewrite-authors.sh` (dry run lists emails), `--apply` (rewrites a temp mirror, prints counts), `--push` (force-pushes branches and tags). Needs `pip install git-filter-repo`. Expect: every SHA changes, so SHAs cited in issues, reports and memory go stale (PR and issue numbers do not); local clones must be re-cloned (`git fetch && git reset --hard` per branch, or fresh clone, on mbp, quant-dev and any other machine); open PR branches must be recreated. The old commits stay reachable by direct URL and through `refs/pull/*` of past PRs, so ask GitHub Support to purge unreachable objects and clear cached views before the flip; until they confirm, treat the emails as still exposed. Also check that `Co-Authored-By` and `Assisted-by` trailers in messages hold no personal address (the script does not edit messages). The noreply address defaults to `119127935+amteb@users.noreply.github.com`; pass `--to` to change it.
1. Create org `zaofang` (Free plan). Check the name is available. Add yourself as owner. Enable 2FA requirement.
2. Install the Claude GitHub App on the org with access to the repo: https://github.com/apps/claude/installations/select_target
3. Transfer: repo Settings > Danger zone > Transfer ownership > `zaofang`, keep the name `yan-lang`. Do not create another repo named `atbraz/yan-lang` afterwards (it would break the redirect).
4. Verify after transfer (each may need fixing, none is automatic to rely on): Actions secrets and variables present; branch and environment settings; CODEOWNERS still resolves; milestones and labels; issue numbers unchanged; GitHub Project 1 (user-owned, auto-add) still receives issues, else recreate the project under the org; webhooks and deploy keys; Pages not in use yet.
5. Transfer the tap `atbraz/homebrew-yan` to `zaofang` (becomes `zaofang/homebrew-yan`, `brew install zaofang/yan/yan`). Create a new fine-grained PAT with resource owner `zaofang`, only the tap repo, Contents rw, and store it as the `HOMEBREW_TAP_TOKEN` secret. The old PAT (owned by the user account) does not cover org repos; delete it.
6. Rotate the runner-registration PAT (Administration rw on `zaofang/yan-lang` only) if self-hosted runners are kept for any period (section C). Never commit it; scripts read tokens through `gh` at run time.
7. Project settings in claude.ai: [Project settings](#project-settings/resources) > repositories: add `zaofang/yan-lang`, remove the old link. Check the cloud environment definitions that name the repo, and reconnect GitHub at https://claude.ai/connect-github after the user rename (step 10).
8. Sweep PR phase 1 (below). Lead merges `.github/` and CODEOWNERS changes.
9. Rename user: Settings > Account > Change username > `amteb`. Effects to expect: profile URL redirects are not guaranteed forever; `@atbraz` mentions and CODEOWNERS entries stop resolving (hence phase 2); commit attribution is by email and is unaffected; PATs, SSH keys and App installs survive; `gh` stores the old name in `~/.config/gh/hosts.yml` (fix in section C).
10. Sweep PR phase 2 (below). Update branch protections/rulesets that name users.
11. Flip public: repo Settings > Danger zone > Change visibility. Then: ruleset on main (#974), Pages for the site (or Cloudflare Pages per D3), secret scanning + push protection, Dependabot alerts, private vulnerability reporting. Tick #982.
12. Games repos: separate org, same procedure (#982 item 7), out of scope here.

### Sweep (repo contents)

```
uv run python tools/move/sweep.py --phase audit      # everything that names the old login
uv run python tools/move/sweep.py --phase transfer   # dry run
uv run python tools/move/sweep.py --phase transfer --apply
uv run python tools/move/sweep.py --phase rename --apply   # only after the user rename
```

Hardcoded today (2026-11-11): `.github/CODEOWNERS` (7 lines, @atbraz), `.github/workflows/release.yml:115` (tap clone URL), `tools/release.py:14` (`REPO`), `NOTICE:5`, `tools/guard.py` (`LEAD`, with `LEAD_ID` as the real key) and its selftest, `dist/install.sh:3,8`, `README.md:39,40,61`, `site/index.md:57,58`, `CONTRIBUTING.md:3`, `CODE_OF_CONDUCT.md:49`. `docs/studies/syntax/yan-delegate.md:283` is a historical review record and stays; studies, research and reports are skipped on purpose. Everything else in the repo uses `$GITHUB_REPOSITORY`. Outside the repo: the Homebrew formula in the tap (its `homepage` and `url` lines), any claude.ai study or Doc that links the old slug, domain/DNS records, package-registry reservations.

The sweep PR touches `.github/`, so it needs the lead's merge and fails `tools/guard.py` when authored from a `claude/` branch, as usual. Rows and codes are not involved.

## C. Machine side (Remote Control)

Machines: `mbp` env_01KUrcBFJoT66nTFr1mFVXUo, repo `/Users/antonio/Code/Projects/yan-lang`; `quant-dev` env_01MsxpfHcFfwEa3Yz4X88P33, repo `/home/antonio/yan-lang`, runners `do-yan*`.
Scripts in `tools/move/` (all dry-run by default, idempotent, no token ever written to disk). Copy by checking out this branch on the machine, or `curl` raw files after the PR merges.

Do the following on each machine, after the transfer (B3) and again after the user rename (B9) where noted.

1. Audit: `tools/move/machine-audit.sh` before and after. Lists git remotes, `gh` auth state, runner directories and services, crontab, systemd units and rc files that name the old login.
2. Remotes (after B3): `tools/move/retarget-remotes.sh /Users/antonio/Code/Projects/yan-lang` (mbp) or `/home/antonio/yan-lang` (quant-dev); review the output; rerun with `--apply`. GitHub redirects make old remotes work for a while; do not rely on it.
3. gh auth (after B9): `gh auth status`. If the stored login is stale: `gh auth refresh`, or `gh auth logout` then `gh auth login`. PATs on the machines owned by the user account (runner registration, HOMEBREW_TAP_TOKEN, any `GH_TOKEN` in rc files or cron) must be recreated with resource owner `zaofang`; keep them in the keychain or a root-only file, not in the repo. The audit script names the files that mention the old login, never prints values.
4. Runners. Decide once:
   - Decided: retire. After the flip hosted runners are free and fork-safe. `tools/move/runners.sh list`, then `runners.sh --apply retire` on quant-dev (do-yan*) and on mbp (the old `mbp-ben*` and `mbp-ceteris` runners, launchd `actions.runner.atbraz-yong.*`). Also remove them in Settings > Actions > Runners if deregistration fails. Do this before the flip so no fork PR can ever queue on them.
   - Fallback (not chosen; use only if hosted runners cannot be made to work or a job needs the box (callgrind, `bench` label)): keep them as an org-level runner group scoped to the one repo, with "Allow public repositories" left off, and jobs gated to `push` on main and `workflow_dispatch`, never `pull_request`. Re-register: `tools/move/runners.sh --apply register --url https://github.com/zaofang --labels self-hosted,linux,x64,do-ci,ephemeral`. Repo-level runners must be assumed to need re-registration after a transfer; verify with `list` and the runners page.
5. Services: launchd on mbp (`launchctl list | grep actions.runner`) and systemd on quant-dev (`systemctl list-units 'actions.runner.*'`). `runners.sh` stops and uninstalls through each runner's `svc.sh`; the unit name embeds the old owner, so a re-registered runner gets a new unit name. Check that nothing else (Remote Control wrappers, cron) invokes the old unit names.
6. Cron and health checks on quant-dev: the audit lists cron lines and units mentioning the owner. Update any healthcheck that polls `repos/atbraz/yan-lang/actions/runners` to the new slug, or delete it when the runners are retired. Remove the "both runners offline" heartbeat from the DO plan if the box is retired.
7. Claude Code environments: the Remote Control sessions for both machines are tied to a folder path, not a remote, so they keep working; after step 2 new sessions clone the new slug. For cloud environments and the project, update the repo link (B7). Threads' standing memory entries naming `atbraz/yan-lang` get updated by the coordinator after the sweep.
8. Verify: a PR from a throwaway `claude/` branch gets its checks on hosted runners; `git push` from both machines works; `gh run list -R zaofang/yan-lang` works from both.

## D. Rollback and failure notes

- Transfer: transfer back from the org's Settings; the redirect stays. Nothing else depends on it until the sweep merges, so merge the sweep PRs only after verifying each step.
- User rename: reversible while `atbraz` is unclaimed by someone else. Do it in one sitting with the sweep PR ready.
- Public flip: not reversible in effect. Flipping back to private does not retract clones, caches or the Actions logs already seen.
- If hosted runners still do not start on the org: stop at step 2 of the order; the move can proceed privately, the flip waits.

## E. Decisions (Antonio, 2026-11-11)

- Tap: transfer with the repo to `zaofang/homebrew-yan`.
- Runners: retire all self-hosted runners after the flip (no bench group).
- Author emails: rewrite history first (B0).
