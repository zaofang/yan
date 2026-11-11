"""Propose (and with --apply perform) the owner/login sweep for the repo move.

sweep.py --phase transfer   atbraz/yan-lang, atbraz/homebrew-yan, `brew install atbraz/yan/yan`
                            -> NEW_ORG/...   (run in the PR that lands right after the transfer)
sweep.py --phase rename     @atbraz and github.com/atbraz profile links -> NEW_USER
                            (run in the PR that lands right after the user rename)
sweep.py --phase audit      list every remaining `atbraz` for manual review

Dry run unless --apply. Idempotent: a second run finds nothing. Reads and writes files only.
"""

import argparse
import re
import subprocess
import sys
from pathlib import Path

# Historical records and this tooling keep the old names on purpose.
SKIP_PREFIXES = (
    "docs/studies/",
    "docs/research/",
    "docs/reports/",
    "docs/runbooks/repo-move",
    "tools/move/",
    "target/",
)
SKIP_NAMES = {"Cargo.lock", "uv.lock"}


def rules(phase: str, org: str, user: str, old: str) -> list[tuple[re.Pattern[str], str]]:
    if phase == "transfer":
        return [
            (re.compile(rf"\b{old}/yan-lang\b"), f"{org}/yan-lang"),
            (re.compile(rf"\b{old}/homebrew-yan\b"), f"{org}/homebrew-yan"),
            (re.compile(rf"\b{old}/yan/yan\b"), f"{org}/yan/yan"),
        ]
    if phase == "rename":
        return [
            (re.compile(rf"@{old}\b"), f"@{user}"),
            (re.compile(rf"github\.com/{old}(?=[)\s\"']|$)"), f"github.com/{user}"),
        ]
    return []


def tracked_files() -> list[str]:
    out = subprocess.run(["git", "ls-files", "-z"], capture_output=True, check=True).stdout
    return [p for p in out.decode().split("\0") if p]


def eligible(path: str) -> bool:
    return Path(path).name not in SKIP_NAMES and not path.startswith(SKIP_PREFIXES)


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("--phase", choices=["transfer", "rename", "audit"], required=True)
    ap.add_argument("--apply", action="store_true")
    ap.add_argument("--old-user", default="atbraz")
    ap.add_argument("--new-org", default="zaofang")
    ap.add_argument("--new-user", default="amteb")
    args = ap.parse_args()

    rs = rules(args.phase, args.new_org, args.new_user, args.old_user)
    audit = re.compile(re.escape(args.old_user))
    changed = 0
    for path in filter(eligible, tracked_files()):
        try:
            lines = Path(path).read_text().splitlines(keepends=True)
        except UnicodeDecodeError, FileNotFoundError:
            continue
        new_lines = []
        for n, line in enumerate(lines, 1):
            new = line
            for pat, rep in rs:
                new = pat.sub(rep, new)
            if args.phase == "audit":
                if audit.search(line):
                    print(f"{path}:{n}: {line.strip()[:140]}")
                    changed += 1
            elif new != line:
                pad = " " * len(path)
                print(f"{path}:{n}: {line.strip()[:110]}\n{pad}   -> {new.strip()[:110]}")
                changed += 1
            new_lines.append(new)
        if args.apply and args.phase != "audit" and new_lines != lines:
            Path(path).write_text("".join(new_lines))

    verb = "rewrote" if args.apply else "would rewrite"
    what = "to review" if args.phase == "audit" else f"{verb} ({args.phase})"
    print(f"\n{changed} line(s) {what}")
    if not args.apply and args.phase != "audit":
        print("dry run; pass --apply to write")
    return 0


if __name__ == "__main__":
    sys.exit(main())
