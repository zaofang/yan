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


def main() -> None:
    tot = [0, 0, 0, 0]
    for d in map(Path, sys.argv[1:]):
        t = load(d)
        both = sum(v["yag"] and v["py"] for v in t.values())
        yag_only = sum(v["yag"] and not v["py"] for v in t.values())
        py_only = sum(v["py"] and not v["yag"] for v in t.values())
        neither = len(t) - both - yag_only - py_only
        tot = [x + y for x, y in zip(tot, (both, yag_only, py_only, neither))]
        print(f"{d.name}\tn={len(t)}\tboth={both}\tyag_only={yag_only}\tpy_only={py_only}\tneither={neither}\tp={sign_p(yag_only, py_only):.4f}")
    both, yag_only, py_only, neither = tot
    print(f"pooled\tn={sum(tot)}\tboth={both}\tyag_only={yag_only}\tpy_only={py_only}\tneither={neither}\tp={sign_p(yag_only, py_only):.4f}")


if __name__ == "__main__":
    main()
