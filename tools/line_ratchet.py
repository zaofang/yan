"""DL13 gate: the three giant lowering files may not grow past the cap.

uv run python -m tools.line_ratchet

New lowering goes in a new module (CLAUDE.md practice 5). Lower the cap as the files split (#757).
"""

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CAP = 3000
FILES = [
    "crates/yan-elab/src/lower.rs",
    "crates/yan-hir/src/raise.rs",
    "crates/yan-mir/src/lower.rs",
]


def over(root: Path, files: list[str] = FILES, cap: int = CAP) -> list[str]:
    errors = []
    for rel in files:
        n = len((root / rel).read_text(encoding="utf-8").splitlines())
        if n > cap:
            errors.append(f"{rel}: {n} lines exceeds the cap of {cap}")
    return errors


def main() -> int:
    errors = over(ROOT)
    for e in errors:
        print(e, file=sys.stderr)
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
