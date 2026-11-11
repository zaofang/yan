#!/usr/bin/env bash
# Point local clones at the new owner. Dry run unless --apply. Idempotent.
# usage: retarget-remotes.sh [--apply] [--old atbraz/yan-lang] [--new zaofang/yan] DIR...
set -euo pipefail
APPLY=0 OLD=atbraz/yan-lang NEW=zaofang/yan
while [[ "${1:-}" == --* ]]; do
  case $1 in
    --apply) APPLY=1; shift ;;
    --old) OLD=$2; shift 2 ;;
    --new) NEW=$2; shift 2 ;;
    *) echo "unknown $1" >&2; exit 2 ;;
  esac
done
[[ $# -ge 1 ]] || { echo "usage: $0 [--apply] DIR..." >&2; exit 2; }

for d in "$@"; do
  git -C "$d" rev-parse --git-dir >/dev/null 2>&1 || { echo "skip (not a repo): $d"; continue; }
  git -C "$d" remote | while read -r r; do
    for kind in "" "--push"; do
      url=$(git -C "$d" remote get-url $kind "$r")
      case $url in
        *"$OLD"*)
          new=${url//$OLD/$NEW}
          echo "$d [$r${kind:+ push}]: $url -> $new"
          [[ $APPLY == 1 ]] && git -C "$d" remote set-url $kind "$r" "$new" ;;
      esac
    done
  done
done
[[ $APPLY == 1 ]] || echo "dry run; pass --apply to write"
