"""Size metrics for the sample programs: uv run python docs/studies/syntax/plain-yan/metrics.py"""

import re
import sys
from pathlib import Path

LEXEME = re.compile(
    r'"(?:\\.|[^"\\])*"'
    r"|\d[\w.]*"
    r"|[A-Za-z_]\w*"
    r"|:=|->|==|!=|<=|>=|\+=|-=|\*=|//|\.\.|\S"
)
ROOT = Path(__file__).parent
DIRS = ["baseline", "p1-shorthand", "p2-plainspoken", "p3-wire", "p4-ledger", "p5-loom", "mix"]


def measure(text: str) -> tuple[int, int, int]:
    lines = [l for l in text.splitlines() if l.strip()]
    nonspace = sum(len(l.strip().replace(" ", "")) for l in lines)
    lexemes = sum(len(LEXEME.findall(l)) for l in lines)
    return len(lines), nonspace, lexemes


def main() -> None:
    base = measure("".join(p.read_text() for p in sorted((ROOT / "baseline").glob("t*.yn"))))
    print("| Variant | Lines | Non-space chars | Lexemes | Lexemes vs baseline |")
    print("| --- | --- | --- | --- | --- |")
    for d in DIRS:
        lines, chars, lex = measure("".join(p.read_text() for p in sorted((ROOT / d).glob("t*.yn"))))
        print(f"| {d} | {lines} | {chars} | {lex} | {100 * (lex - base[2]) / base[2]:+.0f}% |")
    if "-v" in sys.argv:
        for d in DIRS:
            print(d, [measure((ROOT / d / f"t{i}.yn").read_text())[2] for i in range(1, 5)])


if __name__ == "__main__":
    main()
