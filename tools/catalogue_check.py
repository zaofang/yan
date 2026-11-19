"""Diagnostic codes used in crates must match the section 25 catalogue (RFC 0082 D5, #1510).

uv run python -m tools.catalogue_check

Fails on a code written in crate source and absent from the catalogue, or listed in the catalogue
and absent from crate source, unless tools/catalogue_allow.toml records it as known drift.
"""

import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CATALOGUE = ROOT / "spec" / "25-diagnostics-catalogue.md"
ALLOW = ROOT / "tools" / "catalogue_allow.toml"
CODE = r"[EWHVPIN]\d{2,3}"


def expand(a: str, b: str) -> set[str]:
    width = len(a) - 1
    return {f"{a[0]}{n:0{width}d}" for n in range(int(a[1:]), int(b[1:]) + 1)}


def catalogued(text: str) -> tuple[set[str], set[str]]:
    """(codes with their own row, codes covered by a range row)"""
    rows = set(re.findall(rf"^\| ({CODE}) \|", text, re.M))
    ranges: set[str] = set()
    for a, b in re.findall(rf"^\| ({CODE}) to ({CODE}) \|", text, re.M):
        ranges |= expand(a, b)
    return rows, ranges


def used(root: Path) -> set[str]:
    files = subprocess.check_output(
        ["git", "ls-files", "crates/**.rs"], cwd=root, text=True
    ).split()
    out: set[str] = set()
    for f in files:
        if "selftest" in f or "/tests/" in f:
            continue
        out |= set(re.findall(rf'"({CODE})"', (root / f).read_text(encoding="utf-8")))
    return out


def check(
    rows: set[str], ranges: set[str], in_code: set[str], allow: dict[str, list[str]]
) -> list[str]:
    errors = []
    unlisted = sorted(in_code - rows - ranges)
    unused = sorted(rows - in_code)
    for c in unlisted:
        if c not in allow["used_not_catalogued"]:
            errors.append(f"{c}: used in crates, missing from the catalogue")
    for c in unused:
        if c not in allow["catalogued_not_used"]:
            errors.append(f"{c}: in the catalogue, not used in crates")
    for key, live in (("used_not_catalogued", unlisted), ("catalogued_not_used", unused)):
        for c in allow[key]:
            if c not in live:
                errors.append(f"{c}: stale entry in {ALLOW.name} [{key}]")
    return errors


def message_codes() -> set[str]:
    table = tomllib.loads((ROOT / "crates/yan-diag/messages.toml").read_text(encoding="utf-8"))
    return {k.split(".")[0] for k in table}


def main() -> int:
    rows, ranges = catalogued(CATALOGUE.read_text(encoding="utf-8"))
    missing = sorted(message_codes() - rows - ranges)
    if missing:
        print(
            "\n".join(f"{c}: has a message template, missing from the catalogue" for c in missing)
        )
        return 1
    allow = tomllib.loads(ALLOW.read_text(encoding="utf-8"))
    errors = check(rows, ranges, used(ROOT), allow)
    print("\n".join(errors))
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
