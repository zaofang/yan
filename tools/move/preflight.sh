#!/usr/bin/env bash
# Read-only checklist for the public flip. Prints PASS/TODO/MANUAL; changes nothing.
# Run from the repo root with gh authenticated. Prints no secret values.
set -uo pipefail
REPO=${REPO:-$(gh repo view --json nameWithOwner --jq .nameWithOwner)}
ok() { echo "PASS   $*"; }; todo() { echo "TODO   $*"; }; man() { echo "MANUAL $*"; }

n=$(grep -rn "runs-on: *self-hosted\|do-yan\|do-ci" .github/workflows | wc -l)
[[ $n == 0 ]] && ok "no self-hosted runs-on in workflows" || todo "$n self-hosted references in .github/workflows (fork PRs must never reach them; #423 #461)"

for i in 423 461 462; do
  s=$(gh issue view $i --repo "$REPO" --json state --jq .state 2>/dev/null || echo unknown)
  [[ $s == CLOSED ]] && ok "#$i closed" || todo "#$i is $s"
done

if command -v gitleaks >/dev/null; then
  gitleaks detect --source . --log-opts="--all" --redact --no-banner >/dev/null 2>&1 \
    && ok "gitleaks: no findings in full history" || todo "gitleaks reported findings (rerun without redirect)"
else
  todo "install gitleaks and run: gitleaks detect --source . --log-opts=--all --redact"
fi

echo "secrets present (names only):"; gh secret list --repo "$REPO" 2>/dev/null | awk '{print "  " $1}'
echo "self-hosted runners registered:"; gh api "repos/$REPO/actions/runners" --jq '.runners[] | "  \(.name) \(.status)"' 2>/dev/null || echo "  (cannot read; needs admin)"
echo "forks allowed to run workflows without approval? check Settings > Actions > General > Fork pull request workflows"

man "hosted runner starts: run tools/move/smoke-hosted.sh in the new org BEFORE flipping (a billing lock can block public repos too)"
man "employment IP check with lawyer; trademark search Yan/Yong (CNIPA, USPTO, EUIPO)"
man "private vulnerability reporting enabled (Settings > Code security); SECURITY.md matches"
man "commit author emails (~620 commits) become public; accepted? (rewrite needs force push, do not do it casually)"
man "no .github secrets echoed in logs of past runs (logs become public with the repo); delete old runs if unsure"
