"""Change-row registry: check spec/rows.toml, print free row numbers, render Appendix A.1.

rows.py check      fail on a row allocated twice, a cited row that is not registered,
                   a stale Appendix A.1 or a diagnostic code listed twice in section 25
rows.py next [N]   print the next N free row numbers (default 1)
rows.py render     rewrite Appendix A.1 from spec/rows.toml
"""

import argparse
import re
import sys
import tomllib
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
SPEC = ROOT / "spec"
REGISTRY = SPEC / "rows.toml"
APPENDIX = SPEC / "appendix-a.md"
CATALOGUE = SPEC / "25-diagnostics-catalogue.md"
BEGIN = "<!-- rows: generated from spec/rows.toml by tools/rows.py render; do not edit -->"
END = "<!-- /rows -->"
ID = re.compile(r"\b((?:R|SQ)\d+)(?:\s+to\s+((?:R|SQ)\d+))?\b")
CITED_ROW = re.compile(r"\bR(\d+)\b")
ROWS_HEADER = re.compile(r"^(?:#|--) rows:(.*)$", re.M)
CODE_CELL = re.compile(r"^\| ([A-Z]\d{2,3}) \|", re.M)

type Entry = dict[str, Any]


def expand(label: str) -> list[str]:
    ids = []
    for first, last in ID.findall(label):
        prefix = first.rstrip("0123456789")
        lo = int(first[len(prefix) :])
        hi = int(last[len(prefix) :]) if last else lo
        ids += [f"{prefix}{n}" for n in range(lo, hi + 1)]
    return ids


def load() -> list[Entry]:
    entries: list[Entry] = tomllib.loads(REGISTRY.read_text())["entry"]
    return entries


def allocations(entries: list[Entry]) -> tuple[dict[str, int], list[str]]:
    owner: dict[str, int] = {}
    errors = []
    for i, e in enumerate(entries):
        also = set(e.get("also", []))
        for rid in expand(e["rows"]):
            if rid in also:
                continue
            if rid in owner:
                errors.append(f"{rid} allocated by entries {owner[rid]} and {i} ({e['rows']})")
            owner[rid] = i
    for i, e in enumerate(entries):
        for rid in e.get("also", []):
            if rid not in owner:
                errors.append(
                    f"entry {i} ({e['rows']}) lists {rid} in also, but no entry allocates it"
                )
    return owner, errors


def render(entries: list[Entry]) -> str:
    lines = [BEGIN, "", "| Rows | Topic | Sections |", "| --- | --- | --- |"]
    lines += [f"| {e['rows']} | {e['topic']} | {e['sections']} |" for e in entries]
    return "\n".join(lines + ["", END])


def block(text: str) -> str:
    start, end = text.index(BEGIN), text.index(END) + len(END)
    return text[start:end]


def cited_rows() -> dict[str, set[str]]:
    cited: dict[str, set[str]] = {}

    def add(path: Path, rows: list[str]) -> None:
        for r in rows:
            cited.setdefault(r, set()).add(str(path.relative_to(ROOT)))

    for path in SPEC.glob("*.md"):
        add(path, [f"R{n}" for n in CITED_ROW.findall(path.read_text())])
    add(
        ROOT / "SPEC_QUESTIONS.md",
        [f"R{n}" for n in CITED_ROW.findall((ROOT / "SPEC_QUESTIONS.md").read_text())],
    )
    for path in (ROOT / "conformance").rglob("*"):
        if path.is_file() and path.suffix in {".jian", ".wen", ".yong"}:
            for header in ROWS_HEADER.findall(path.read_text(errors="replace")):
                add(path, [r for r in expand(header) if r.startswith("R")])
    return cited


def check() -> list[str]:
    entries = load()
    owner, errors = allocations(entries)
    if block(APPENDIX.read_text()) != render(entries):
        errors.append("Appendix A.1 is stale: run tools/rows.py render")
    for rid, paths in sorted(cited_rows().items()):
        if rid not in owner:
            errors.append(f"{rid} cited in {', '.join(sorted(paths))} but not in spec/rows.toml")
    codes = CODE_CELL.findall(CATALOGUE.read_text())
    errors += [
        f"diagnostic code {c} listed twice in section 25"
        for c in sorted(set(codes))
        if codes.count(c) > 1
    ]
    return errors


def next_free(count: int) -> list[str]:
    owner, _ = allocations(load())
    top = max(int(r[1:]) for r in owner if r.startswith("R"))
    return [f"R{top + k}" for k in range(1, count + 1)]


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("check")
    nxt = sub.add_parser("next")
    nxt.add_argument("count", nargs="?", type=int, default=1)
    sub.add_parser("render")
    args = ap.parse_args(argv)
    match args.cmd:
        case "check":
            errors = check()
            for e in errors:
                print(e, file=sys.stderr)
            return 1 if errors else 0
        case "next":
            print(" ".join(next_free(args.count)))
        case "render":
            text = APPENDIX.read_text()
            APPENDIX.write_text(text.replace(block(text), render(load())))
    return 0


if __name__ == "__main__":
    sys.exit(main())
