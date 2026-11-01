"""Fail when an agent-authored change touches tests or conformance cases.

spec/ and rfcs/ are open to agent PRs; CODEOWNERS routes them to the lead, who merges them.

Reads changed paths (one per line) on stdin. Exit 1 on any violation.
"""

import argparse
import sys

PROTECTED = ("tests/", "conformance/")
AGENT_BRANCH_PREFIXES = ("agent/", "claude/")


def is_agent(branch: str, author_type: str) -> bool:
    return author_type == "Bot" or branch.startswith(AGENT_BRANCH_PREFIXES)


def violations(paths: list[str], branch: str, author_type: str) -> list[str]:
    if not is_agent(branch, author_type):
        return []
    return [p for p in paths if p.startswith(PROTECTED)]


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--branch", required=True)
    ap.add_argument("--author-type", default="User", help="GitHub user type: User or Bot")
    args = ap.parse_args(argv)
    paths = [line.strip() for line in sys.stdin if line.strip()]
    bad = violations(paths, args.branch, args.author_type)
    for p in bad:
        print(f"agent change touches protected path: {p}", file=sys.stderr)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
