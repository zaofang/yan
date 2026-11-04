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
    lines = [line for line in text.splitlines() if line.strip()]
    nonspace = sum(len(line.strip().replace(" ", "")) for line in lines)
    lexemes = sum(len(LEXEME.findall(line)) for line in lines)
    return len(lines), nonspace, lexemes


def variant(d: str) -> str:
    return "".join(p.read_text() for p in sorted((ROOT / d).glob("t*.yn")))


def main() -> None:
    base = measure(variant("baseline"))
    print("| Variant | Lines | Non-space chars | Lexemes | Lexemes vs baseline |")
    print("| --- | --- | --- | --- | --- |")
    for d in DIRS:
        lines, chars, lex = measure(variant(d))
        print(f"| {d} | {lines} | {chars} | {lex} | {100 * (lex - base[2]) / base[2]:+.0f}% |")
    if "-v" in sys.argv:
        for d in DIRS:
            print(d, [measure((ROOT / d / f"t{i}.yn").read_text())[2] for i in range(1, 5)])


if __name__ == "__main__":
    main()
