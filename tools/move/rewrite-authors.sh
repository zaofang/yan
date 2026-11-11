#!/usr/bin/env bash
# Replace personal author/committer emails in history with a noreply address, on a throwaway mirror.
# Dry run (default): clone, list distinct emails with counts, rewrite nothing.
# --apply: rewrite the mirror and print the before/after counts. Still no push.
# --push (implies --apply): force-push all branches and tags to the remote. Destroys every SHA.
# Needs git-filter-repo (pip install git-filter-repo). Touches commit metadata only; messages stay.
#
# usage: rewrite-authors.sh [--apply|--push] [--remote URL] [--to EMAIL]
set -euo pipefail
MODE=dry REMOTE="" TO="119127935+amteb@users.noreply.github.com"
while [[ $# -gt 0 ]]; do
  case $1 in
    --apply) MODE=apply; shift ;;
    --push) MODE=push; shift ;;
    --remote) REMOTE=$2; shift 2 ;;
    --to) TO=$2; shift 2 ;;
    *) echo "unknown $1" >&2; exit 2 ;;
  esac
done
REMOTE=${REMOTE:-$(git remote get-url origin)}
WORK=$(mktemp -d); trap 'rm -rf "$WORK"' EXIT
git clone -q --mirror "$REMOTE" "$WORK/m"
cd "$WORK/m"

count() { git log --all --format='%ae%n%ce' | sort | uniq -c | sort -rn; }
echo "== emails before =="; count
[[ $MODE == dry ]] && { echo "dry run; pass --apply to rewrite the mirror, --push to publish"; exit 0; }

command -v git-filter-repo >/dev/null || { echo "install git-filter-repo" >&2; exit 1; }
git filter-repo --force --email-callback "
return email if b'noreply' in email else b'$TO'
"
echo "== emails after =="; count

if [[ $MODE == push ]]; then
  git push --force "$REMOTE" 'refs/heads/*:refs/heads/*' 'refs/tags/*:refs/tags/*'
  echo "pushed. Old commits stay reachable via refs/pull/* and cached views: ask GitHub Support to purge (runbook B2)."
else
  echo "mirror rewritten in $WORK/m (deleted on exit); pass --push to publish"
fi
