"""Change-row registry: check spec/rows.toml, print free row numbers, render Appendix A.1.

rows.py check            fail on a row allocated twice, a cited row that is not registered,
                         a stale Appendix A.1, an A.1 section that does not exist, a code
                         listed twice in section 25, a code cited in spec/ but missing from
                         section 25, two RFCs with one number, or a study cited in spec/ or
                         rfcs/ that docs/research/README.md does not list
rows.py next [N]         print the next N free row numbers (default 1)
rows.py next-code FAM    print the next free diagnostic code in a family, e.g. E3 or W8
rows.py render           rewrite Appendix A.1 from spec/rows.toml
rows.py renumber --base REF [--check]
                         move this branch's new rows that REF already allocates above REF's
                         top, rewriting rows.toml, spec/ and SPEC_QUESTIONS.md, then render;
                         --check only reports (exit 1 on a collision). Run it before merging
                         main into the branch: after the merge old and new citations of one
                         number cannot be told apart.
"""

import argparse
import re
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
SPEC = ROOT / "spec"
REGISTRY = SPEC / "rows.toml"
APPENDIX = SPEC / "appendix-a.md"
CATALOGUE = SPEC / "25-diagnostics-catalogue.md"
RFCS = ROOT / "rfcs"
RESEARCH = ROOT / "docs" / "research" / "README.md"
BEGIN = "<!-- rows: generated from spec/rows.toml by tools/rows.py render; do not edit -->"
END = "<!-- /rows -->"
ID = re.compile(r"\b((?:R|SQ)\d+)(?:\s+to\s+((?:R|SQ)\d+))?\b")
CITED_ROW = re.compile(r"\bR(\d+)\b")
ROWS_HEADER = re.compile(r"^(?:#|--) rows:(.*)$", re.M)
CODE_CELL = re.compile(r"^\| ([A-Z]\d{2,3}) \|", re.M)
CODE_LIST_CELL = re.compile(r"^\| ([A-Z]\d{2,3}(?:, [A-Z]\d{2,3})*) \|", re.M)
CITED_CODE = re.compile(r"\b[EWHVP]\d{3}\b")
HEADING = re.compile(r"^#{2,4} (?:Appendix )?(\d+(?:\.\d+)*|[A-Z]\.\d+)[. ]", re.M)
SECTION_REF = re.compile(r"\b\d+(?:\.\d+)*\b")
ARTIFACT = re.compile(r"https://claude\.ai/artifact/[A-Za-z0-9]+")
RFC_FILE = re.compile(r"^(\d{4})-.+\.md$")

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
        if path.is_file() and path.suffix in {".yag", ".yn", ".yir"}:
            for header in ROWS_HEADER.findall(path.read_text(errors="replace")):
                add(path, [r for r in expand(header) if r.startswith("R")])
    return cited


def missing_sections(entries: list[Entry]) -> list[str]:
    headings = {h for path in SPEC.glob("*.md") for h in HEADING.findall(path.read_text())}
    errors = []
    for e in entries:
        refs = SECTION_REF.findall(re.sub(r"\(.*?\)", "", e["sections"]))
        errors += [f"{e['rows']}: section {s} does not exist" for s in refs if s not in headings]
    return errors


def duplicate_rfcs() -> list[str]:
    numbers = [m[1] for p in RFCS.glob("*.md") if (m := RFC_FILE.match(p.name))]
    return [
        f"RFC {n} used by more than one file" for n in sorted(set(numbers)) if numbers.count(n) > 1
    ]


def unindexed_studies() -> list[str]:
    indexed = set(ARTIFACT.findall(RESEARCH.read_text()))
    errors = []
    for path in [*SPEC.glob("*.md"), *RFCS.glob("*.md")]:
        for url in sorted(set(ARTIFACT.findall(path.read_text())) - indexed):
            errors.append(
                f"{url} cited in {path.relative_to(ROOT)} but not in {RESEARCH.relative_to(ROOT)}"
            )
    return errors


def uncatalogued_codes() -> list[str]:
    listed = {c for cell in CODE_LIST_CELL.findall(CATALOGUE.read_text()) for c in cell.split(", ")}
    errors = []
    # Appendix A records history, including codes later moved or retired (R379).
    for path in sorted(p for p in SPEC.glob("*.md") if p != APPENDIX):
        for code in sorted(set(CITED_CODE.findall(path.read_text())) - listed):
            errors.append(f"{code} cited in {path.relative_to(ROOT)} but not listed in section 25")
    return errors


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
    return (
        errors
        + uncatalogued_codes()
        + missing_sections(entries)
        + duplicate_rfcs()
        + unindexed_studies()
    )


def base_entries(ref: str) -> list[Entry]:
    text = subprocess.run(
        ["git", "show", f"{ref}:spec/rows.toml"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    entries: list[Entry] = tomllib.loads(text)["entry"]
    return entries


def row_numbers(entries: list[Entry]) -> list[int]:
    return [int(r[1:]) for e in entries for r in expand(e["rows"]) if r.startswith("R")]


def renumbering(base: list[Entry], head: list[Entry]) -> dict[str, str]:
    """Old to new ids for every row of a new head entry that shares a number with base."""
    base_owner, _ = allocations(base)
    known = {(e["rows"], e["topic"]) for e in base}
    top = max(row_numbers(base) + row_numbers(head))
    mapping: dict[str, str] = {}
    for e in head:
        if (e["rows"], e["topic"]) in known:
            continue
        own = [r for r in expand(e["rows"]) if r.startswith("R") and r not in e.get("also", [])]
        if any(r in base_owner for r in own):
            for r in own:
                top += 1
                mapping[r] = f"R{top}"
    return mapping


def apply_renumbering(mapping: dict[str, str]) -> None:
    def sub(m: re.Match[str]) -> str:
        return mapping.get(m.group(0), m.group(0))

    pattern = re.compile(r"\bR\d+\b")
    for path in [REGISTRY, ROOT / "SPEC_QUESTIONS.md", *SPEC.glob("*.md")]:
        text = path.read_text()
        new = pattern.sub(sub, text)
        if new != text:
            path.write_text(new)
    stale = {p for rid, paths in cited_rows().items() if rid in mapping for p in paths}
    for p in sorted(p for p in stale if p.startswith("conformance/")):
        print(f"conformance is lead-owned: update the old row ids in {p}", file=sys.stderr)


def next_free(count: int) -> list[str]:
    owner, _ = allocations(load())
    top = max(int(r[1:]) for r in owner if r.startswith("R"))
    return [f"R{top + k}" for k in range(1, count + 1)]


def next_code(family: str) -> str:
    taken = [c for c in CODE_CELL.findall(CATALOGUE.read_text()) if c.startswith(family)]
    width = 3 - len(family) + 1
    top = max((int(c[len(family) :]) for c in taken), default=-1)
    return f"{family}{top + 1:0{width}d}"


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("check")
    nxt = sub.add_parser("next")
    nxt.add_argument("count", nargs="?", type=int, default=1)
    code = sub.add_parser("next-code")
    code.add_argument("family")
    sub.add_parser("render")
    ren = sub.add_parser("renumber")
    ren.add_argument("--base", required=True)
    ren.add_argument("--check", action="store_true")
    args = ap.parse_args(argv)
    match args.cmd:
        case "check":
            errors = check()
            for e in errors:
                print(e, file=sys.stderr)
            return 1 if errors else 0
        case "next":
            print(" ".join(next_free(args.count)))
        case "next-code":
            print(next_code(args.family))
        case "renumber":
            mapping = renumbering(base_entries(args.base), load())
            for old, new in mapping.items():
                print(f"{old} -> {new}")
            if mapping and args.check:
                return 1
            if mapping:
                apply_renumbering(mapping)
                APPENDIX.write_text(
                    APPENDIX.read_text().replace(block(APPENDIX.read_text()), render(load()))
                )
        case "render":
            text = APPENDIX.read_text()
            APPENDIX.write_text(text.replace(block(text), render(load())))
    return 0


if __name__ == "__main__":
    sys.exit(main())
