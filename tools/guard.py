"""Fail when an agent-authored change touches tests or conformance cases.

spec/ and rfcs/ are open to agent PRs; CODEOWNERS routes them to the lead, who merges them.

Reads changed paths (one per line) on stdin. Exit 1 on any violation.
"""

import argparse
import sys

PROTECTED = ("tests/", "conformance/")
AGENT_BRANCH_PREFIXES = ("agent/", "claude/")
LEAD = "atbraz"
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
    paths: list[str], branch: str, author_type: str, author: str = "", author_id: str = ""
) -> list[str]:
    if not is_agent(branch, author_type, author, author_id):
        return []
    return [p for p in paths if p.startswith(PROTECTED)]


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--branch", required=True)
    ap.add_argument("--author-type", default="User", help="GitHub user type: User or Bot")
    ap.add_argument("--author", default="", help="PR author login; anyone but the lead is an agent")
    ap.add_argument("--author-id", default="", help="PR author numeric id; wins over --author")
    args = ap.parse_args(argv)
    paths = [line.strip() for line in sys.stdin if line.strip()]
    bad = violations(paths, args.branch, args.author_type, args.author, args.author_id)
    for p in bad:
        print(f"agent change touches protected path: {p}", file=sys.stderr)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
