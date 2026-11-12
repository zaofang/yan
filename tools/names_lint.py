"""DL12 gate: a retired name may not appear outside the allowlisted archive.

uv run python -m tools.names_lint

Scans tracked text files for the retired names in tools/names_allow.toml. A name joined to another
word by a hyphen or letters (Fan-out, Hyndman-Fan) is not a match.
"""

import re
import subprocess
import sys
import tomllib
from fnmatch import fnmatch
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CONFIG = ROOT / "tools" / "names_allow.toml"


def pattern(names: list[str]) -> re.Pattern[str]:
    return re.compile(r"(?<![-\w])(?:" + "|".join(map(re.escape, names)) + r")(?![-\w])")


def allowed(path: str, globs: list[str]) -> bool:
    return any(fnmatch(path, g) or fnmatch(path, g.replace("**", "*")) for g in globs)


def scan(files: list[str], root: Path, cfg: dict[str, list[str]]) -> list[str]:
    pat = pattern(cfg["retired"])
    errors = []
    for rel in files:
        if allowed(rel, cfg["allow"]):
            continue
        try:
            text = (root / rel).read_text(encoding="utf-8")
        except UnicodeDecodeError, OSError:
            continue
        for n, line in enumerate(text.splitlines(), 1):
            if m := pat.search(line):
                errors.append(f"{rel}:{n}: retired name {m.group()}")
    return errors


def tracked() -> list[str]:
    out = subprocess.run(
        ["git", "ls-files", "-z"], cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout
    return [f for f in out.split("\0") if f and f != "Cargo.lock"]


def main() -> int:
    cfg = tomllib.loads(CONFIG.read_text(encoding="utf-8"))
    errors = scan(tracked(), ROOT, cfg)
    for e in errors:
        print(e, file=sys.stderr)
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
