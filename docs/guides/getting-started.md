# Getting started with Yan

This tutorial writes and runs six programs in Yan Agent (`.yag`), the Python-shaped surface that is built today.
Every program on this page is a file in [`examples/real/`](../../examples/real/README.md) or is run by
`docs/guides/check.py`, on the interpreter, `--backend machine`, `--backend cranelift` and `--ub`. Spec
sections are cited by number; the spec in [`spec/`](../../spec/README.md) is the authority, this page only teaches.

## 1. Install and run

Install with the script or Homebrew (see the [README](../../README.md#install)), or build from a checkout with
`cargo build -p yan-cli` and use `target/debug/yan`. Then:

```yag run
def main() uses io:
    io.print("hello")
```

```text out
hello
```

Save it as `hello.yag` and run `yan run hello.yag`. Two things differ from Python:

- There is no `import`. Standard names are qualified: `io.print`, `json.parse`.
- `uses io` declares the capability the function needs (spec 12). A function that prints without declaring it is
  rejected by `yan check` with E301; nothing in Yan reaches the outside world implicitly.

## 2. Types, functions and examples

Annotate every parameter and return type. `ex` lines come first in a body; they are examples that `yan test`
runs. This program counts words:

```yag include=examples/real/wordfreq.yag
# Word frequency over an embedded text: top words, longest word, line stats.

def corpus() -> str:
    return """the quick brown fox jumps over the lazy dog
the dog barks and the fox runs away
a quick end to a quick story"""

def top_words(text: str, k: int) -> list[tuple[str, int]]:
    ex top_words("b a b", 1) == [("b", 2)]
    counts: dict[str, int] = {}
    for w in text.lower().split():
        counts[w] = (counts.get(w) else 0) + 1
    pairs = [(w, n) for w, n in counts.items()]
    pairs.sort(key=lambda p: (-p[1], p[0]))
    return pairs[:k]

def longest(text: str) -> str:
    best = ""
    for w in text.split():
        if w.char_count() > best.char_count():
            best = w.copy()
    return best

def main() uses io:
    for w, n in top_words(corpus(), 4):
        io.print(f"{w:<6} {n}")
    io.print(f"longest: {longest(corpus())}")
    lines = corpus().splitlines()
    io.print(f"lines: {lines.len()}")
    for i, l in enumerate(lines):
        io.print(f"{i + 1}: {l.split().len()} words")
```

```text out
the    4
quick  3
a      2
dog    2
longest: quick
lines: 3
1: 9 words
2: 8 words
3: 7 words
```

Points that trip up Python writers:

- `d.get(k)` returns `int | None`. A maybe-value is unwrapped with `else`: `(d.get(k) else 0) + 1`.
- Strings have no `len(s)`: use `s.char_count()` or `s.byte_len()`.
- `.copy()` is explicit when you keep a `str` or list you only borrowed (`best = w.copy()`).
- `int` is 64-bit and overflow panics.

## 3. Failure is a value

There are no exceptions. A function that can fail says so in its signature, `-> T fail E`; `x?` propagates, and
`x else y` handles. `E` is a sum type, written on one line:

```yag include=examples/real/csv_report.yag
# Parse a small CSV of sales, aggregate per region, print a report.

type RowErr = Short(line: int) | BadNum(line: int, field: str)

class Sale:
    region: str
    item: str
    qty: int
    price: float

data: str = """region,item,qty,price
north,widget,3,2.50
south,gadget,1,10.00
north,gadget,2,10.00
east,widget,10,2.25
south,widget,4,2.50"""

def parse_row(n: int, line: str) -> Sale fail RowErr:
    parts = line.split(",")
    if parts.len() != 4:
        fail Short(n)
    qty = parts[2].parse_int() else fail BadNum(n, "qty")
    price = parts[3].parse_float() else fail BadNum(n, "price")
    return Sale(region=parts[0].copy(), item=parts[1].copy(), qty=qty, price=price)

def parse_all(src: str) -> list[Sale] fail RowErr:
    out: list[Sale] = []
    n = 0
    for line in src.splitlines():
        n += 1
        if n == 1:
            continue
        out.append(parse_row(n, line)?)
    return out

def totals(rows: list[Sale]) -> dict[str, float]:
    t: dict[str, float] = {}
    for r in rows:
        t[r.region.copy()] = (t.get(r.region) else 0.0) + r.qty * r.price
    return t

def report(src: str) uses io:
    rows = parse_all(src) else e -> []
    if rows.len() == 0:
        io.print("no rows")
        return
    t = totals(rows)
    for k in sorted(t.keys()):
        io.print(f"{k}: {t[k]}")

def main() uses io:
    report(data)
    report("h\na,b,c")
```

```text out
east: 22.5
north: 27.5
south: 20.0
no rows
```

`class` holds fields only; behaviour is a function whose first parameter is the record, callable as `r.f()`.
Records and sums are values; `match` on a sum is exhaustive.

## 4. Parallelism without data races

`par [f(x) for x in xs]` runs the calls in parallel and returns results in order, so the output is the same on
every run (determinism is a design law, not a best effort):

```yag include=examples/real/primes_par.yag
# Sieve then count primes per range in parallel.

def sieve(n: int) -> list[bool]:
    flags = [True for _ in range(n + 1)]
    flags[0] = False
    flags[1] = False
    i = 2
    while i * i <= n:
        if flags[i]:
            j = i * i
            while j <= n:
                flags[j] = False
                j += i
        i += 1
    return flags

def count(flags: list[bool], lo: int, hi: int) -> int:
    c = 0
    for i in range(lo, hi):
        if flags[i]:
            c += 1
    return c

def main() uses io:
    flags = sieve(10000)
    ranges = [(0, 2500), (2500, 5000), (5000, 7500), (7500, 10001)]
    counts = par [count(flags, lo, hi) for lo, hi in ranges]
    io.print(f"{counts}")
    total = 0
    for c in counts:
        total += c
    io.print(f"{total}")
```

```text out
[367, 302, 281, 279]
1229
```

## 5. Channels and actors

A pipeline is functions joined by channels. A parameter you mutate is declared `var` and the caller passes
`&x`:

```yag include=examples/real/pipeline.yag
# Concurrent pipeline: producer -> workers -> collector over channels.

def square_sum(n: int) -> int:
    total = 0
    for i in range(1, n + 1):
        total += i * i
    return total

def producer(var tx: Sender[int], n: int):
    for i in range(1, n + 1):
        tx.send(i) else e -> None
    tx.send(-1) else e -> None

def worker(var rx: Receiver[int], var out: Sender[int]):
    while True:
        v = (rx.recv() else None) else -1
        if v < 0:
            return
        out.send(square_sum(v)) else e -> None

def main() uses io:
    with sched = sequential():
        tx, rx = Channel[int](2)
        otx, orx = Channel[int](2)
        par concurrent:
            producer(&tx, 5)
            worker(&rx, &otx)
            for _ in range(5):
                v = (orx.recv() else None) else -1
                io.print(f"{v}")
```

```text out
1
5
14
30
55
```

`with sched = sequential()` picks the scheduler. The interpreter, `machine` and `cranelift` print the same
output for this program; `tools.difftest` compares backends over the conformance run cases, the bench programs and the examples it lists.

State owned by one task is an actor. Calls go through the handle and can fail, hence `else`:

```yag include=examples/real/actor_counter.yag
# Actor holding a counter; several callers via par.

class Counter:
    n: int

def add(var self: Counter, k: int) -> int:
    self.n += k
    return self.n

def get(var self: Counter) -> int:
    return self.n

def main() uses io:
    a = actor.spawn(Counter(n=0))
    r1 = a.add(5) else -1
    r2 = a.add(7) else -1
    io.print(f"{r1} {r2}")
    io.print(f"{a.get() else -1}")
```

```text out
5 12
12
```

## 6. The checker is the teacher

`yan check FILE` prints diagnostics with a stable code. `yan explain CODE` shows the rule, the spec section and a
conformance example with its fix. A type error:

```yag fail=E201
def main() uses io:
    x: int = "a"
    io.print(f"{x}")
```

Other everyday commands:

| Command | Does |
|---|---|
| `yan check FILE` | diagnostics only; add `--format json` for tools |
| `yan fmt FILE` | canonical form; there is one |
| `yan test` | the package's `ex` lines and `test_` functions |
| `yan run --backend machine FILE` | the register VM; also `cranelift` for native code |
| `yan run --ub FILE` | abstract-byte machine with the 6.7 undefined-behavior checks |
| `yan new --template cli NAME` | a package: `package.yag` and `src/main.yag` |
| `yan view --as yag FILE` | print a module in another surface |

Next: [known gaps](known-gaps.md) lists what these programs cannot do yet, and the
[contributor guide](contributing.md) shows how to fix one.
