"""N7 as a lens law pair (16.5, R996): one seeded property test, parameterised by surface.

PutGet: S(elaborate(F)) == F for canonical F in S.
GetPut: elaborate(S(W)) == elaborate(W) by item hash for Yan W in the subset of S.
"""

import random
import tempfile
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

from tools.conform import LawExecutor, Result, ToolFailure

type Items = list[str]


@dataclass(frozen=True)
class Surface:
    name: str
    suffix: str
    view: str  # the `yan view --as` key


SURFACES = [Surface("Yan Agent", ".yag", "yag")]


def _expr(rng: random.Random, atoms: list[str], arity: list[int], depth: int = 2) -> str:
    if depth == 0 or rng.random() < 0.3:
        if arity and rng.random() < 0.3:
            f = rng.randrange(len(arity))
            return f"f{f}({', '.join(str(rng.randint(0, 9)) for _ in range(arity[f]))})"
        return rng.choice(atoms)
    op = rng.choice("+-*")
    return f"{_expr(rng, atoms, arity, depth - 1)} {op} {_expr(rng, atoms, arity, depth - 1)}"


def generate(rng: random.Random) -> Items:
    """Plain Yan items: int functions over arithmetic, calls to earlier items and clauses."""
    items: Items = []
    arity: list[int] = []
    for n in range(rng.randint(1, 6)):
        params = [f"p{i}" for i in range(rng.randint(1, 3))]
        atoms = [*params, *(str(rng.randint(0, 9)) for _ in range(2))]
        if len(params) == 1 and rng.random() < 0.3:
            body = f"\n  0 -> {_expr(rng, atoms, arity, 1)}\n  _ -> {_expr(rng, atoms, arity)}"
        else:
            body = f" {_expr(rng, atoms, arity)}"
        sig = ", ".join(f"{p}: int" for p in params)
        items.append(f"f{n}({sig}) -> int ={body}")
        arity.append(len(params))
    return items


def module(items: Items) -> str:
    return "\n\n".join(items) + "\n"


class Unsupported(Exception):
    """A construct the implementation does not raise yet: a gap, not a law violation."""


def _view(ex: LawExecutor, path: Path, s: Surface) -> str | None:
    try:
        return ex.view(path, s.view)
    except ToolFailure as e:
        if "not supported yet" in str(e):
            raise Unsupported from e
        raise


def violation(ex: LawExecutor, s: Surface, items: Items) -> str | None:
    """The first lens-law violation for the module, or None. Modules outside the subset hold."""
    with tempfile.TemporaryDirectory() as tmp:
        w = Path(tmp) / "w.yn"
        w.write_text(module(items), encoding="utf-8")
        base = ex.hash(w)
        try:
            projected = _view(ex, w, s)
        except Unsupported:
            return None
        if base is None or projected is None:
            return None
        f = Path(tmp) / f"w{s.suffix}"
        f.write_text(projected, encoding="utf-8")
        got = ex.hash(f)
        if got != base:
            return "GetPut: elaborate(S(W)) != elaborate(W)"
        try:
            again = _view(ex, f, s)
        except Unsupported:
            return None
        if again != projected:
            return "PutGet: S(elaborate(F)) != F"
    return None


def shrink(items: Items, fails: Callable[[Items], bool]) -> Items:
    """Drop items one at a time while the failure persists."""
    changed = True
    while changed:
        changed = False
        for i in range(len(items)):
            rest = items[:i] + items[i + 1 :]
            if rest and fails(rest):
                items, changed = rest, True
                break
    return items


def check(ex: LawExecutor, s: Surface, seed: int, samples: int) -> Result:
    rng = random.Random(seed)
    name = f"N7 {s.name} seed={seed}"
    for _ in range(samples):
        items = generate(rng)
        what = violation(ex, s, items)
        if what:
            small = shrink(items, lambda it: violation(ex, s, it) is not None)
            return Result(
                name, ex.name, "fail", f"{what}; smallest counterexample:\n{module(small)}"
            )
    return Result(name, ex.name, "pass")
