#!/usr/bin/env bash
# Self-hosted runner lifecycle for the move. Run on the machine that hosts the runners.
# Dry run unless --apply. Tokens are fetched at run time into an env var and never written to disk.
#
#   runners.sh list
#   runners.sh [--apply] retire                     stop services and deregister every runner found
#   runners.sh [--apply] register --url https://github.com/zaofang/yan [--labels L] [--prefix P]
#                                                   re-register every runner found against a new URL
#
# Needs `gh` authenticated as an owner/admin of the target (fine-grained PAT: Administration rw on
# the repo, or org self-hosted runners rw). Registration tokens expire in 1 hour.
set -euo pipefail
APPLY=0
[[ "${1:-}" == --apply ]] && { APPLY=1; shift; }
CMD=${1:-}; shift || true
URL="" LABELS="" PREFIX=""
while [[ $# -gt 0 ]]; do
  case $1 in
    --url) URL=$2; shift 2 ;;
    --labels) LABELS=$2; shift 2 ;;
    --prefix) PREFIX=$2; shift 2 ;;
    *) echo "unknown $1" >&2; exit 2 ;;
  esac
done

run() { echo "+ $*"; [[ $APPLY == 1 ]] && "$@"; return 0; }

runner_dirs() { find "$HOME" /opt /srv -maxdepth 4 -name .runner -type f -printf '%h\n' 2>/dev/null || \
                find "$HOME" -maxdepth 4 -name .runner -type f -exec dirname {} \; ; }

# gh api path for a registration/removal token, from a github URL
token_path() {
  local url=$1 path=${1#https://github.com/}
  if [[ $path == */* ]]; then echo "repos/$path/actions/runners/$2"; else echo "orgs/$path/actions/runners/$2"; fi
}

current_url() { grep -o '"gitHubUrl": *"[^"]*"' "$1/.runner" | cut -d'"' -f4; }
current_name() { grep -o '"agentName": *"[^"]*"' "$1/.runner" | cut -d'"' -f4; }

stop_service() {
  local d=$1
  if [[ -x $d/svc.sh ]]; then run sudo "$d/svc.sh" stop || true; run sudo "$d/svc.sh" uninstall || true; fi
}

case $CMD in
  list)
    runner_dirs | while read -r d; do echo "$d  $(current_name "$d")  $(current_url "$d")"; done ;;
  retire)
    runner_dirs | while read -r d; do
      echo "== $d ($(current_name "$d")) =="
      stop_service "$d"
      if [[ $APPLY == 1 ]]; then
        tok=$(gh api -X POST "$(token_path "$(current_url "$d")" remove-token)" --jq .token)
        (cd "$d" && ./config.sh remove --token "$tok") || echo "deregister failed: remove it in Settings > Actions > Runners"
        unset tok
      else
        echo "+ gh api -X POST $(token_path "$(current_url "$d")" remove-token)   (token held in memory only)"
        echo "+ (cd $d && ./config.sh remove --token <token>)"
      fi
    done ;;
  register)
    [[ -n $URL ]] || { echo "--url required" >&2; exit 2; }
    runner_dirs | while read -r d; do
      name=$(current_name "$d")
      [[ "$(current_url "$d")" == "$URL" ]] && { echo "$d: already on $URL, skip"; continue; }
      echo "== $d ($name) -> $URL =="
      stop_service "$d"
      if [[ $APPLY == 1 ]]; then
        old=$(gh api -X POST "$(token_path "$(current_url "$d")" remove-token)" --jq .token 2>/dev/null || true)
        [[ -n $old ]] && (cd "$d" && ./config.sh remove --token "$old") || true
        tok=$(gh api -X POST "$(token_path "$URL" registration-token)" --jq .token)
        (cd "$d" && ./config.sh --unattended --replace --url "$URL" --token "$tok" \
           --name "${PREFIX}${name}" ${LABELS:+--labels "$LABELS"})
        unset tok old
        [[ -x $d/svc.sh ]] && { sudo "$d/svc.sh" install; sudo "$d/svc.sh" start; }
      else
        echo "+ deregister from $(current_url "$d") (token in memory only)"
        echo "+ (cd $d && ./config.sh --unattended --replace --url $URL --token <token> --name ${PREFIX}${name}${LABELS:+ --labels $LABELS})"
        echo "+ sudo $d/svc.sh install && start"
      fi
    done ;;
  *) sed -n 2,12p "$0"; exit 2 ;;
esac
[[ $APPLY == 1 || $CMD == list ]] || echo "dry run; pass --apply (before the subcommand) to write"
