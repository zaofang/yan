#!/usr/bin/env bash
# Proves GitHub-hosted runners start for a PUBLIC repo in the new org, before the real repo flips.
# Creates a throwaway public repo with a hello workflow. Dry run unless --apply. Needs org owner rights.
# usage: smoke-hosted.sh [--apply] [--org zaofang]
set -euo pipefail
APPLY=0 ORG=zaofang
while [[ $# -gt 0 ]]; do case $1 in --apply) APPLY=1; shift ;; --org) ORG=$2; shift 2 ;; *) exit 2 ;; esac; done
R="$ORG/runner-smoke"
if [[ $APPLY == 0 ]]; then
  cat <<TXT
+ gh repo create $R --public --add-readme
+ push .github/workflows/smoke.yml (runs-on: ubuntu-latest, macos-latest, windows-latest; echo ok)
+ gh run watch; expect conclusion=success on all three, steps > 0
+ gh repo delete $R --yes     (needs: gh auth refresh -s delete_repo)
dry run; pass --apply to create
TXT
  exit 0
fi
gh repo view "$R" >/dev/null 2>&1 || gh repo create "$R" --public --add-readme
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
gh repo clone "$R" "$tmp/r" -- -q
mkdir -p "$tmp/r/.github/workflows"
cat > "$tmp/r/.github/workflows/smoke.yml" <<'YML'
name: smoke
on: [push, workflow_dispatch]
jobs:
  hello:
    strategy: { matrix: { os: [ubuntu-latest, macos-latest, windows-latest] } }
    runs-on: ${{ matrix.os }}
    steps: [{ run: echo ok }]
YML
git -C "$tmp/r" add -A && git -C "$tmp/r" commit -qm "smoke" && git -C "$tmp/r" push -q
sleep 5
gh run list --repo "$R" --limit 1
gh run watch --repo "$R" --exit-status "$(gh run list --repo "$R" --limit 1 --json databaseId --jq '.[0].databaseId')"
echo "hosted runners OK. Delete with: gh repo delete $R --yes"
