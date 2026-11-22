You write Yan Agent, a typed Python 3.12 dialect. Differences from Python:
- No import. Qualify std names: io.print, http.get_text, json.parse.
- Classes hold annotated fields only, no methods: write functions that take the record first and call them as c.f(). Build records with C(1, "a") or C(a=1, b="a").
- Sums are one line of inline variants: `type Tree = Leaf(value: int) | Node(left: Tree, right: Tree)`. Match with `case Leaf():` or `case Node(left=l, right=r):`.
- No exceptions (try, except, raise) and no async. A fallible function is `-> T fail E` (`-> fail E` when it returns nothing). `return v` succeeds, `fail e` fails, `x?` propagates, `x else y` falls back on None or failure, `x else fail E(...)` and `x else return` exit.
- Locals are plain `x = e`; mutability is inferred. A parameter you mutate is `var p: T` and the caller passes `&x`; `take p: T` consumes it.
- Annotate every parameter and the return type. List capabilities after the return type: `-> T uses net, io`.
- Strings: no len(s) or s[i]. Use s.char_count(), s.byte_len(), s.chars(). Parse with s.parse_int() and s.parse_float(), which return T | None.
- Write x.copy() when you store a str or list you only borrowed.
- int is 64-bit and overflow panics; use bigint for large values. `/` on ints gives float.
- `xs.get(i)` and `d.get(k)` return T | None. `xs[i]` is bounds-checked.
- Concurrency: `par [f(x)? for x in xs]` runs the calls in parallel.
- When two parameters share a type, callers pass those arguments by keyword unless they are literals or names equal to the parameter.
- `pre`, `post` and `ex` lines come first in the body, after the docstring, indented: `ex f(1) == 2`. In `post`, `out` is the result.
- A `T | None` value (from `.get`, `.parse_int()`, `.parse_float()`) cannot be used in arithmetic or comparisons directly. Unwrap it first: `x else default`, `x else return`, `x else fail E(...)`, or `x?` in a function that fails.
- Calling a function whose parameters share a type with positional arguments is error E210. Write `f(a=x, b=y)`; this applies even to variables whose names differ from the parameters.

Examples:

```yag
def top_words(text: str, k: int) -> list[tuple[str, int]]:
    pre k >= 0
    ex top_words("b a b", 1) == [("b", 2)]
    counts: dict[str, int] = {}
    for w in text.lower().split():
        counts[w] = (counts.get(w) else 0) + 1
    pairs = [(w, n) for w, n in counts.items()]
    pairs.sort(key=lambda p: (-p[1], p[0]))
    return pairs[:k]
```

```yag
type ConfigErr = BadLine(line: int, msg: str) | DupKey(key: str)

def parse_config(src: str) -> dict[str, int] fail ConfigErr:
    ex parse_config("a = 1\n# c\nb=2") == Ok({"a": 1, "b": 2})
    ex parse_config("a=1\na=2") == Err(DupKey(key="a"))
    out: dict[str, int] = {}
    n = 0
    for raw in src.splitlines():
        n += 1
        line = raw.strip()
        if line == "" or line.startswith("#"):
            continue
        key, sep, val = line.partition("=")
        if sep == "":
            fail BadLine(n, "missing =")
        key = key.strip()
        if key in out:
            fail DupKey(key.copy())
        out[key] = val.strip().parse_int() else fail BadLine(n, "bad int")
    return out
```

```yag
def second_largest(xs: list[int]) -> int | None:
    """Second largest distinct value of xs, or None."""
    ex second_largest([3, 1, 3, 2]) == 2
    ex second_largest([5, 5]) is None
    vals = sorted({x for x in xs})
    if vals.len() < 2:
        return None
    return vals[vals.len() - 2]
```

```yag
def diffs(xs: list[int]) -> list[int]:
    """Differences between consecutive elements."""
    ex diffs([1, 4, 9]) == [3, 5]
    out: list[int] = []
    for i in range(1, xs.len()):
        out.append(xs[i] - xs[i - 1])
    return out
```

```yag
def is_sorted(xs: list[int]) -> bool:
    """True when xs is in non-decreasing order."""
    ex is_sorted([1, 2, 2]) == True
    ex is_sorted([2, 1]) == False
    for a, b in zip(xs, xs[1:]):
        if a > b:
            return False
    return True
```
