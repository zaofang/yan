#!/usr/bin/env bash
# Read-only. Run on each Remote Control machine before and after the move.
# Lists everything on this machine that names the old owner. Prints no tokens.
set -uo pipefail
OLD="${OLD_USER:-atbraz}"
ROOTS="${SEARCH_ROOTS:-$HOME}"

echo "== host: $(hostname) =="
echo "== git remotes naming $OLD (repos under $ROOTS, depth 4) =="
find $ROOTS -maxdepth 4 -name .git -type d 2>/dev/null | while read -r g; do
  d=$(dirname "$g")
  git -C "$d" remote -v 2>/dev/null | grep -i "$OLD" | sed "s|^|$d: |"
done | sort -u

echo "== gh auth (masked by gh) =="
gh auth status 2>&1 | sed 's/Token: .*/Token: <masked>/' | head -8

echo "== runners (dirs with .runner) =="
find "$HOME" /opt /srv -maxdepth 4 -name .runner -type f 2>/dev/null | while read -r f; do
  d=$(dirname "$f")
  printf '%s  url=%s name=%s\n' "$d" \
    "$(grep -o '"gitHubUrl": *"[^"]*"' "$f" | cut -d'"' -f4)" \
    "$(grep -o '"agentName": *"[^"]*"' "$f" | cut -d'"' -f4)"
done

echo "== runner services =="
if command -v systemctl >/dev/null; then systemctl list-units --all --no-legend 'actions.runner.*' 2>/dev/null | awk '{print $1, $3, $4}'; fi
if command -v launchctl >/dev/null; then launchctl list 2>/dev/null | grep -i actions.runner; fi

echo "== crontab / timers / scripts naming $OLD =="
crontab -l 2>/dev/null | grep -i "$OLD\|yan" | sed 's/^/cron: /'
grep -rIl -i "$OLD" "$HOME/bin" "$HOME/.local/bin" /etc/cron.d /etc/systemd/system "$HOME/.config/systemd" 2>/dev/null | sed 's/^/file: /'

echo "== env files / shell rc naming $OLD (names only, values not shown) =="
grep -l -i "$OLD" "$HOME"/.bashrc "$HOME"/.zshrc "$HOME"/.zprofile "$HOME"/.profile "$HOME"/.config/gh/hosts.yml 2>/dev/null
