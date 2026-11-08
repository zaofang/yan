"""Paired first-reply comparison across seeds with an exact two-sided sign test.

usage: paired.py DIR [DIR ...]   (each DIR holds the *.yag.json / *.py.json records of one seed)
"""

import json
import sys
from math import comb
from pathlib import Path


def sign_p(a: int, b: int) -> float:
    n = a + b
    if n == 0:
        return 1.0
    k = min(a, b)
    return min(1.0, 2 * sum(comb(n, i) for i in range(k + 1)) / 2**n)


def load(d: Path) -> dict[str, dict[str, bool]]:
    tasks: dict[str, dict[str, bool]] = {}
    for f in d.glob("*.json"):
        r = json.loads(f.read_text())
        if r.get("impossible"):
            continue
        tasks.setdefault(r["task"], {})[r["arm"]] = r["hidden_first"]["ok"]
    return {t: v for t, v in tasks.items() if len(v) == 2}


def row(name: str, c: tuple[int, int, int, int]) -> str:
    both, yag_only, py_only, neither = c
    return (
        f"{name}\tn={sum(c)}\tboth={both}\tyag_only={yag_only}\tpy_only={py_only}"
        f"\tneither={neither}\tp={sign_p(yag_only, py_only):.4f}"
    )


def main() -> None:
    tot = (0, 0, 0, 0)
    for d in map(Path, sys.argv[1:]):
        t = load(d).values()
        yag_only = sum(v["yag"] and not v["py"] for v in t)
        py_only = sum(v["py"] and not v["yag"] for v in t)
        both = sum(v["yag"] and v["py"] for v in t)
        c = (both, yag_only, py_only, len(t) - both - yag_only - py_only)
        tot = tuple(x + y for x, y in zip(tot, c, strict=True))
        print(row(d.name, c))
    print(row("pooled", tot))


if __name__ == "__main__":
    main()
