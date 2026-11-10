"""DL14 gate: figures carry a commit and a date, and generated coverage is fresh.

uv run python -m tools.lint_reports [--today YYYY-MM-DD]

Reports (docs/reports/*.md) must name a commit and a date in their first lines (a date-prefixed
file name counts). A line in any README that states a figure ("N of M", "N%") outside a table must
carry a commit and a date itself. conformance/COVERAGE.md must equal fresh output of the generator.
Reports older than STALE_DAYS are listed as stale; that is a label, not a failure.
"""

import argparse
import re
import sys
import tomllib
from datetime import date
from pathlib import Path

from tools import conform

ROOT = Path(__file__).resolve().parent.parent
STALE_DAYS = 7
HEAD_LINES = 12

COMMIT = re.compile(r"\b(?=[0-9a-f]*\d)(?=[0-9a-f]*[a-f])[0-9a-f]{7,40}\b")
DATE = re.compile(r"\b\d{4}-\d{2}-\d{2}\b")
FIGURE = re.compile(r"\b\d+ of \d+\b|\b\d+(?:\.\d+)? ?%")


def report_date(path: Path, text: str) -> date | None:
    head = "\n".join(text.splitlines()[:HEAD_LINES])
    m = DATE.search(path.name) or DATE.search(head)
    return date.fromisoformat(m.group()) if m else None


def lint_report(path: Path, text: str) -> list[str]:
    head = "\n".join(text.splitlines()[:HEAD_LINES])
    errors = []
    if not COMMIT.search(head):
        errors.append(f"{path}: no commit in the first {HEAD_LINES} lines")
    if report_date(path, text) is None:
        errors.append(f"{path}: no date in the file name or the first {HEAD_LINES} lines")
    return errors


def lint_readme(path: Path, text: str) -> list[str]:
    errors = []
    for n, line in enumerate(text.splitlines(), 1):
        if line.lstrip().startswith("|") or not FIGURE.search(line):
            continue
        if not (COMMIT.search(line) and DATE.search(line)):
            errors.append(f"{path}:{n}: figure without a commit and a date on the line")
    return errors


def stale(path: Path, text: str, today: date) -> bool:
    d = report_date(path, text)
    return d is not None and (today - d).days > STALE_DAYS


def coverage_fresh(root: Path = ROOT) -> list[str]:
    conf = root / "conformance"
    catalogue = tomllib.loads((conf / "catalogue.toml").read_text(encoding="utf-8"))
    fresh = conform.coverage(conform.discover(conf), catalogue)
    if fresh != (conf / "COVERAGE.md").read_text(encoding="utf-8"):
        return ["conformance/COVERAGE.md is stale: regenerate with `tools/conform.py --coverage`"]
    return []


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--today", type=date.fromisoformat, default=date.today())
    args = ap.parse_args(argv)
    errors = coverage_fresh()
    for path in sorted((ROOT / "docs" / "reports").glob("*.md")):
        text = path.read_text(encoding="utf-8")
        errors += lint_report(path.relative_to(ROOT), text)
        if stale(path, text, args.today):
            print(f"stale: {path.relative_to(ROOT)}")
    for path in sorted(ROOT.rglob("README.md")):
        rel = path.relative_to(ROOT)
        if rel.parts[0] in {"target", "node_modules", ".git"}:
            continue
        errors += lint_readme(rel, path.read_text(encoding="utf-8"))
    for e in errors:
        print(e, file=sys.stderr)
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
