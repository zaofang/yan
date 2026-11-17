"""Fail when an agent-authored change touches tests or conformance cases.

spec/ and rfcs/ are open to agent PRs; CODEOWNERS routes them to the lead, who merges them.

Paired exception (CASE WITH ROW): with --base, an agent PR that adds spec rows may also add the
conformance cases that cite them. Allowed under conformance/, and nowhere else:
  - a new case (file or directory) whose `rows` cite a row this PR adds to spec/rows.toml,
  - conformance/manifest.json when it equals what the suite generates from the PR's cases,
  - conformance/catalogue.toml when the PR only adds lines to it.
Editing or deleting an existing case, or adding one that cites no new row, stays a violation.
tests/ is never open. CODEOWNERS still routes every conformance/ change to the lead.

Reads changed paths (one per line) on stdin. Exit 1 on any violation.
"""

import argparse
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))  # CI runs this file as a script, where `tools` is not importable

PROTECTED = ("tests/", "conformance/")
AGENT_BRANCH_PREFIXES = ("agent/", "claude/")
LEAD = "amteb"
LEAD_ID = "119127935"  # immutable: survives the login rename and cannot be squatted


def is_lead(author: str, author_id: str) -> bool:
    return author_id == LEAD_ID if author_id else author == LEAD


def is_agent(branch: str, author_type: str, author: str, author_id: str = "") -> bool:
    return (
        author_type == "Bot"
        or branch.startswith(AGENT_BRANCH_PREFIXES)
        or (author != "" and not is_lead(author, author_id))
    )


def violations(
    paths: list[str],
    branch: str,
    author_type: str,
    author: str = "",
    author_id: str = "",
    paired: frozenset[str] | set[str] = frozenset(),
) -> list[str]:
    if not is_agent(branch, author_type, author, author_id):
        return []
    return [
        p
        for p in paths
        if p.startswith(PROTECTED) and not (p.startswith("conformance/") and p in paired)
    ]


def _git(*args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout


def paired_paths(base: str, head: str, paths: list[str]) -> set[str]:
    """Changed conformance paths that this PR may add because it also adds the rows they cite."""
    from tools import conform, rows

    new_rows = rows.added_rows(base)
    if not new_rows:
        return set()
    added = set(
        _git("diff", "--diff-filter=A", "--name-only", "--no-renames", f"{base}...{head}").split(
            "\n"
        )
    )
    cases = conform.discover(ROOT / "conformance")
    allowed: set[str] = set()
    for c in cases:
        if not new_rows.intersection(r for label in c.rows for r in rows.expand(label)):
            continue
        rel = c.path.relative_to(ROOT).as_posix()
        allowed |= {p for p in paths if p in added and (p == rel or p.startswith(rel + "/"))}
    manifest = ROOT / "conformance" / "manifest.json"
    generated = json.dumps(conform.manifest(cases, ROOT / "conformance"), indent=1) + "\n"
    if "conformance/manifest.json" in paths and manifest.read_text() == generated:
        allowed.add("conformance/manifest.json")
    numstat = _git("diff", "--numstat", f"{base}...{head}", "--", "conformance/catalogue.toml")
    if numstat and numstat.split()[1] == "0":
        allowed.add("conformance/catalogue.toml")
    return allowed


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--branch", required=True)
    ap.add_argument("--author-type", default="User", help="GitHub user type: User or Bot")
    ap.add_argument("--author", default="", help="PR author login; anyone but the lead is an agent")
    ap.add_argument("--author-id", default="", help="PR author numeric id; wins over --author")
    ap.add_argument("--base", default="", help="base commit; enables the paired row and case rule")
    ap.add_argument("--head", default="HEAD", help="head commit, used with --base")
    args = ap.parse_args(argv)
    paths = [line.strip() for line in sys.stdin if line.strip()]
    paired = paired_paths(args.base, args.head, paths) if args.base else set()
    bad = violations(paths, args.branch, args.author_type, args.author, args.author_id, paired)
    for p in bad:
        print(f"agent change touches protected path: {p}", file=sys.stderr)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
