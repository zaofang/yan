"""Prototype of the default calculus proposed for Yan Law, run on task T2.

A rule is (label, condition, value, overrides). Evaluation: among applicable rules, keep those not
overridden by another applicable rule; one survivor is the value, none is Gap, more than one is
Conflict. `check` sweeps a grid of inputs and reports every Gap and Conflict region.
"""

from dataclasses import dataclass
from decimal import Decimal, ROUND_HALF_EVEN
from itertools import product
from typing import Callable


@dataclass(frozen=True)
class Rule:
    label: str
    when: Callable[[dict], bool]
    value: Callable[[dict], Decimal]
    over: frozenset[str] = frozenset()


def evaluate(rules: list[Rule], env: dict) -> tuple[str, Decimal | None, list[str]]:
    live = [r for r in rules if r.when(env)]
    beaten = {o for r in live for o in r.over}
    top = [r for r in live if r.label not in beaten]
    if not top:
        return "Gap", None, []
    if len(top) > 1 and len({r.value(env) for r in top}) > 1:
        return "Conflict", None, [r.label for r in top]
    return "Ok", top[0].value(env), [top[0].label]


def check(rules: list[Rule], grid: dict[str, list], given=lambda e: True) -> dict[str, list[dict]]:
    found: dict[str, list[dict]] = {"Gap": [], "Conflict": []}
    for combo in product(*grid.values()):
        env = dict(zip(grid, combo))
        if not given(env):
            continue
        status, _, _ = evaluate(rules, env)
        if status != "Ok":
            found[status].append(env)
    return found


def half(d: Decimal) -> Decimal:
    return (d / 2).quantize(Decimal("1"), ROUND_HALF_EVEN)


base = lambda e: Decimal(250) * e["children"]

naive = [
    Rule("s4.1", lambda e: e["children"] > 0, base),
    Rule("s4.2", lambda e: e["children"] > 0 and e["income"] > 80000, lambda e: half(base(e)), frozenset({"s4.1"})),
    Rule("s4.3", lambda e: e["disabled"], lambda e: base(e) + 100, frozenset({"s4.1"})),
]
fixed = naive[:2] + [
    Rule("s4.3", lambda e: e["disabled"], lambda e: base(e) + 100, frozenset({"s4.1", "s4.2"})),
    Rule("s4.4", lambda e: e["children"] == 0, lambda e: Decimal(0)),
]
grid = {"children": [0, 1, 3], "income": [40000, 80000, 80001, 120000], "disabled": [False, True]}

if __name__ == "__main__":
    given = lambda e: not e["disabled"] or e["children"] > 0
    runs = [("naive", naive, lambda e: True), ("fixed", fixed, lambda e: True), ("fixed+given", fixed, given)]
    for name, rules, g in runs:
        found = check(rules, grid, g)
        print(name, {k: len(v) for k, v in found.items()})
        for k, envs in found.items():
            for env in envs[:2]:
                print(" ", k, env, evaluate(rules, env)[2])
    print("explain", evaluate(fixed, {"children": 2, "income": 90000, "disabled": True}))
