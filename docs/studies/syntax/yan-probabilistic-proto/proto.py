"""Prototype: models as functions over a `prob` handler; `given` conditions by address;
randomness keyed by (seed, address) so unchanged addresses keep their draws."""
import hashlib, math, random

def stream(seed: int, addr: str) -> random.Random:
    h = hashlib.blake2b(f"{seed}/{addr}".encode(), digest_size=8).digest()
    return random.Random(int.from_bytes(h))

class Handler:
    def __init__(self, seed: int, given: dict | None = None):
        self.seed, self.given, self.logw, self.trace = seed, given or {}, 0.0, {}
    def sample(self, addr: str, dist):
        if addr in self.given:
            v = self.given[addr]; self.logw += dist.logp(v)
        else:
            v = dist.draw(stream(self.seed, addr))
        self.trace[addr] = v
        return v

class Beta:
    def __init__(s, a, b): s.a, s.b = a, b
    def draw(s, r): return r.betavariate(s.a, s.b)
class Bernoulli:
    def __init__(s, p): s.p = p
    def draw(s, r): return r.random() < s.p
    def logp(s, v): return math.log(s.p if v else 1 - s.p)

def coin_bias(prob, n):
    p = prob.sample("p", Beta(1, 1))
    for i in range(n):
        prob.sample(f"flips[{i}]", Bernoulli(p))
    return p

def importance(model, args, given, draws, seed):
    ws, ps = [], []
    for d in range(draws):
        h = Handler(seed=hash((seed, d)) & 0xFFFFFFFF, given=given)
        ps.append(model(h, *args)); ws.append(h.logw)
    m = max(ws); w = [math.exp(x - m) for x in ws]
    return sum(a * b for a, b in zip(w, ps)) / sum(w)

data = {f"flips[{i}]": f for i, f in enumerate([True, True, False, True])}
a = importance(coin_bias, (4,), data, 4000, seed=7)
b = importance(coin_bias, (4,), data, 4000, seed=7)
print(f"posterior mean {a:.4f} (exact 4/6={4/6:.4f}); rerun identical: {a == b}")
# Address stability: prior draw of p is the same whether or not flips are conditioned.
h1, h2 = Handler(1), Handler(1, data)
coin_bias(h1, 4); coin_bias(h2, 4)
print("p stable across conditioning:", h1.trace["p"] == h2.trace["p"])
# Typo in a given address silently becomes latent in PyMC/Turing-style dict conditioning:
h3 = Handler(1, {"flip[0]": True}); coin_bias(h3, 4)
print("typo'd key used:", "flip[0]" in h3.trace, "(a trace type would reject it at compile time)")
