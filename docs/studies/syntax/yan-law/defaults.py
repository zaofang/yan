"""Prototype of the default calculus proposed for Yan Law, run on task T2.

A rule is (label, condition, value, defeats). Evaluation: among applicable rules, keep those no
other applicable rule defeats; one survivor is the value, none is Gap, more than one with different
values is Conflict. `check` sweeps a grid of inputs and reports every Gap and Conflict.
"""

from collections.abc import Callable
from dataclasses import dataclass
from decimal import ROUND_HALF_EVEN, Decimal
from itertools import product

Env = dict


@dataclass(frozen=True)
class Rule:
    label: str
    when: Callable[[Env], bool]
    value: Callable[[Env], Decimal]
    defeats: frozenset[str] = frozenset()


def evaluate(rules: list[Rule], env: Env) -> tuple[str, Decimal | None, list[str]]:
    live = [r for r in rules if r.when(env)]
    beaten = {d for r in live for d in r.defeats}
    top = [r for r in live if r.label not in beaten]
    if not top:
        return "Gap", None, []
    if len({r.value(env) for r in top}) > 1:
        return "Conflict", None, [r.label for r in top]
    return "Ok", top[0].value(env), [top[0].label]


def check(rules: list[Rule], grid: dict[str, list], given: Callable[[Env], bool]) -> dict:
    found: dict[str, list[Env]] = {"Gap": [], "Conflict": []}
    for combo in product(*grid.values()):
        env = dict(zip(grid, combo, strict=True))
        if given(env):
            status, _, _ = evaluate(rules, env)
            if status != "Ok":
                found[status].append(env)
    return found


def base(e: Env) -> Decimal:
    return Decimal(250) * e["children"]


def half(e: Env) -> Decimal:
    return (base(e) / 2).quantize(Decimal(1), ROUND_HALF_EVEN)


def plus_100(e: Env) -> Decimal:
    return base(e) + 100


def anything(e: Env) -> bool:
    return True


def disabled_has_child(e: Env) -> bool:
    return not e["disabled"] or e["children"] > 0


s4_1 = Rule("s4(1)", lambda e: e["children"] > 0, base)
s4_2 = Rule(
    "s4(2)", lambda e: e["children"] > 0 and e["income"] > 80000, half, frozenset({"s4(1)"})
)
naive = [s4_1, s4_2, Rule("s4(3)", lambda e: e["disabled"], plus_100, frozenset({"s4(1)"}))]
fixed = [
    s4_1,
    s4_2,
    Rule("s4(3)", lambda e: e["disabled"], plus_100, frozenset({"s4(1)", "s4(2)"})),
    Rule("s4(4)", lambda e: e["children"] == 0, lambda e: Decimal(0)),
]
grid = {"children": [0, 1, 3], "income": [40000, 80000, 80001, 120000], "disabled": [False, True]}

if __name__ == "__main__":
    runs = [("naive", naive, anything), ("fixed", fixed, anything)]
    runs.append(("fixed+given", fixed, disabled_has_child))
    for name, rules, given in runs:
        found = check(rules, grid, given)
        print(name, {k: len(v) for k, v in found.items()})
        for k, envs in found.items():
            for env in envs[:2]:
                print(" ", k, env, evaluate(rules, env)[2])
    print("explain", evaluate(fixed, {"children": 2, "income": 90000, "disabled": True}))
