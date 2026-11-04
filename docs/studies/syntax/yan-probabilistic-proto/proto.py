"""Prototype: models as functions over a `prob` handler; `given` conditions by address;
randomness keyed by (seed, address) so unchanged addresses keep their draws."""

import hashlib
import math
import random
from collections.abc import Callable
from typing import Protocol


class Dist(Protocol):
    def draw(self, r: random.Random) -> float | bool: ...
    def logp(self, v: float | bool) -> float: ...


def stream(seed: int, addr: str) -> random.Random:
    h = hashlib.blake2b(f"{seed}/{addr}".encode(), digest_size=8).digest()
    return random.Random(int.from_bytes(h))


class Handler:
    def __init__(self, seed: int, given: dict[str, float | bool] | None = None) -> None:
        self.seed = seed
        self.given = given or {}
        self.logw = 0.0
        self.trace: dict[str, float | bool] = {}

    def sample(self, addr: str, dist: Dist) -> float | bool:
        if addr in self.given:
            v = self.given[addr]
            self.logw += dist.logp(v)
        else:
            v = dist.draw(stream(self.seed, addr))
        self.trace[addr] = v
        return v


class Beta:
    def __init__(self, a: float, b: float) -> None:
        self.a, self.b = a, b

    def draw(self, r: random.Random) -> float:
        return r.betavariate(self.a, self.b)

    def logp(self, v: float | bool) -> float:
        raise NotImplementedError


class Bernoulli:
    def __init__(self, p: float) -> None:
        self.p = p

    def draw(self, r: random.Random) -> bool:
        return r.random() < self.p

    def logp(self, v: float | bool) -> float:
        return math.log(self.p if v else 1 - self.p)


def coin_bias(prob: Handler, n: int) -> float:
    p = float(prob.sample("p", Beta(1, 1)))
    for i in range(n):
        prob.sample(f"flips[{i}]", Bernoulli(p))
    return p


Model = Callable[[Handler, int], float]


def importance(
    model: Model, n: int, given: dict[str, float | bool], draws: int, seed: int
) -> float:
    ws, ps = [], []
    for d in range(draws):
        h = Handler(seed=hash((seed, d)) & 0xFFFFFFFF, given=given)
        ps.append(model(h, n))
        ws.append(h.logw)
    m = max(ws)
    w = [math.exp(x - m) for x in ws]
    return sum(a * b for a, b in zip(w, ps, strict=True)) / sum(w)


data: dict[str, float | bool] = {f"flips[{i}]": f for i, f in enumerate([True, True, False, True])}
a = importance(coin_bias, 4, data, 4000, seed=7)
b = importance(coin_bias, 4, data, 4000, seed=7)
print(f"posterior mean {a:.4f} (exact 4/6={4 / 6:.4f}); rerun identical: {a == b}")

# Address stability: the prior draw of p is the same with or without conditioning.
h1, h2 = Handler(1), Handler(1, data)
coin_bias(h1, 4)
coin_bias(h2, 4)
print("p stable across conditioning:", h1.trace["p"] == h2.trace["p"])

# A misspelled key is ignored silently, as in dict-based conditioning elsewhere.
h3 = Handler(1, {"flip[0]": True})
coin_bias(h3, 4)
print("typo'd key used:", "flip[0]" in h3.trace)
