"""Release notes from merged PRs and spec rows (#872).

uv run python -m tools.changelog FROM TO     markdown for the changes in FROM..TO

PRs are the GitHub merge commits in the range, grouped by branch prefix. Rows are the R numbers
that spec/rows.toml gained between the two refs.
"""

import argparse
import re
import subprocess
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MERGE = re.compile(r"Merge pull request #(\d+) from [^/\s]+/(?:([\w-]+)/)?\S+")
SECTIONS = {
    "feat": "Features",
    "fix": "Fixes",
    "spec": "Spec",
    "rfc": "Spec",
    "conform": "Conformance",
    "perf": "Performance",
    "docs": "Documentation",
}


def git(*args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout


def prs(log: str) -> list[tuple[str, int, str]]:
    """(section, number, title) from `git log --merges` records of subject, body."""
    found = []
    for record in log.split("\x00"):
        subject, _, body = record.strip().partition("\n")
        match = MERGE.match(subject)
        if match:
            title = body.strip().splitlines()[0] if body.strip() else subject
            found.append((SECTIONS.get(match[2] or "", "Other"), int(match[1]), title))
    return found


def row_ids(text: str) -> set[int]:
    """Every R number named by the `rows` cells ("R1, R2", "R450 to R459")."""
    ids: set[int] = set()
    for entry in tomllib.loads(text)["entry"]:
        cell = entry["rows"]
        for lo, hi in re.findall(r"R(\d+)(?: to R(\d+))?", cell):
            ids.update(range(int(lo), int(hi or lo) + 1))
    return ids


def render(changes: list[tuple[str, int, str]], new_rows: set[int]) -> str:
    out = []
    for section in [*dict.fromkeys(SECTIONS.values()), "Other"]:
        items = [
            f"- {title} (#{n})"
            for s, n, title in sorted(changes, key=lambda c: c[1])
            if s == section
        ]
        if items:
            out += [f"## {section}", "", *items, ""]
    if new_rows:
        out += ["## Spec rows added", "", ", ".join(f"R{r}" for r in sorted(new_rows)), ""]
    return "\n".join(out)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("base")
    parser.add_argument("head")
    args = parser.parse_args()
    span = f"{args.base}..{args.head}"
    log = git("log", "--merges", "--first-parent", "--format=%s%n%b%x00", span)
    old = row_ids(git("show", f"{args.base}:spec/rows.toml"))
    new = row_ids(git("show", f"{args.head}:spec/rows.toml"))
    print(render(prs(log), new - old))


if __name__ == "__main__":
    main()
