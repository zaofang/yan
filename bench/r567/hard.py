# ruff: noqa: E501
"""Yan-specific hard tasks for the R567 harness: concurrency, ownership, failure, contracts, sums,
numbers, bits, capabilities. HumanEval and MBPP translate Python algorithms and cannot probe these.

Each task has a prompt, hidden Yan tests (appended to the model's file and run by `yan test`) and a
reference solution. The reference is never shown to a model; `--validate` checks that it passes the
hidden tests, so a task that no solution can pass never ships.

usage: uv run python bench/r567/hard.py --validate target/release/yan
"""

import os
import pathlib
import subprocess
import sys
import tempfile
from dataclasses import dataclass


@dataclass
class Hard:
    id: str
    category: str
    prompt: str
    tests: str
    ref: str
    expect: tuple[
        str, ...
    ] = ()  # run-mode task: `tests` is a driver, the check is its stdout in FIXTURE


# `yan test` runs on an in-memory file system, so the fs.sub tasks run the program (`yan run`,
# `python`) in a directory holding this tree. `{ROOT}` in a link target is the directory itself.
FIXTURE = {"notes/a.txt": "alpha", "notes/sub/b.txt": "beta", "secret.txt": "TOPSECRET"}
LINKS = {"notes/link": "../secret.txt", "notes/abslink": "{ROOT}/secret.txt"}
NOTE = (
    "Names come from untrusted requests. Read only files inside the directory `notes` (relative to the "
    "working directory); a missing or unreadable file gives an empty string, and no name, however it is "
    "written, may reach a file outside `notes`."
)
YAN_SUB = "A capability narrows for a block: `with fs = fs.sub(dir):` confines `fs` to `dir`. "

TASKS = [
    Hard(
        "hard/par_lowest_failure",
        "concurrency",
        "Check every element in parallel and fail with the failure of the lowest-index bad element.\n\n"
        "Given:\n\nclass Bad:\n    n: int\n\ndef check(x: int) -> int fail Bad:\n"
        "    if x % 2 == 1:\n        fail Bad(n=x)\n    return x\n\n"
        "Function: all_even(xs: list[int]) -> list[int] fail Bad\n"
        "It returns the checked values in order, or fails with the Bad of the first odd element.",
        "def test_hidden_0():\n    assert all_even([2, 4]) == [2, 4]\n\n"
        "def test_hidden_1():\n    r = all_even([2, 7, 4, 9, 3]) else err -> [err.n]\n    assert r == [7]\n\n"
        "def test_hidden_2():\n    r = all_even([]) else err -> [err.n]\n    assert r == []\n",
        "class Bad:\n    n: int\n\ndef check(x: int) -> int fail Bad:\n"
        "    if x % 2 == 1:\n        fail Bad(n=x)\n    return x\n\n"
        "def all_even(xs: list[int]) -> list[int] fail Bad:\n    return par [check(x)? for x in xs]\n",
    ),
    Hard(
        "hard/par_squares",
        "concurrency",
        "Square every element in parallel.\n\n"
        "Given:\n\ndef square(x: int) -> int:\n    return x * x\n\n"
        "Function: squares(xs: list[int]) -> list[int]",
        "def test_hidden_0():\n    assert squares([1, 2, 3]) == [1, 4, 9]\n\n"
        "def test_hidden_1():\n    assert squares([]) == []\n",
        "def square(x: int) -> int:\n    return x * x\n\n"
        "def squares(xs: list[int]) -> list[int]:\n    return par [square(x) for x in xs]\n",
    ),
    Hard(
        "hard/channel_capacity",
        "concurrency",
        "Create a channel of integers with the given capacity and try to send every element without "
        "blocking. Count how many sends were accepted before the channel was full, then stop.\n\n"
        "Function: accepted(cap: int, xs: list[int]) -> int uses nondet",
        "def test_hidden_0() uses nondet:\n    assert accepted(2, [5, 6, 7, 8]) == 2\n\n"
        "def test_hidden_1() uses nondet:\n    assert accepted(10, [1, 2]) == 2\n",
        "def push(var tx: Sender[int], v: int) -> bool uses nondet:\n"
        "    tx.try_send(v) else return False\n    return True\n\n"
        "def accepted(cap: int, xs: list[int]) -> int uses nondet:\n"
        "    tx, rx = Channel[int](cap)\n    n = 0\n    for x in xs:\n"
        "        if not push(&tx, x):\n            break\n        n += 1\n    return n\n",
    ),
    Hard(
        "hard/inout_bump",
        "ownership",
        "Add `by` to every element of the list, in place, so the caller sees the change.\n\n"
        "Function: bump_all(xs: list[int], by: int) -> None",
        "def test_hidden_0():\n    v = [1, 2, 3]\n    bump_all(&v, 10)\n    assert v == [11, 12, 13]\n\n"
        "def test_hidden_1():\n    v: list[int] = []\n    bump_all(&v, 1)\n    assert v == []\n",
        "def bump_all(var xs: list[int], by: int) -> None:\n"
        "    for i in range(xs.len()):\n        xs[i] += by\n",
    ),
    Hard(
        "hard/take_longest",
        "ownership",
        "Consume both lists and return the longer one; the first wins a tie.\n\n"
        "Function: longest(a: list[int], b: list[int]) -> list[int]",
        "def test_hidden_0():\n    assert longest([1], [1, 2]) == [1, 2]\n\n"
        "def test_hidden_1():\n    assert longest([3, 4], [5, 6]) == [3, 4]\n",
        "def longest(take a: list[int], take b: list[int]) -> list[int]:\n"
        "    if a.len() >= b.len():\n        return a\n    return b\n",
    ),
    Hard(
        "hard/swap_ends",
        "ownership",
        "Swap the first and last elements of the list in place. An empty or one-element list is "
        "unchanged.\n\nFunction: swap_ends(xs: list[int]) -> None",
        "def test_hidden_0():\n    v = [1, 2, 3, 4]\n    swap_ends(&v)\n    assert v == [4, 2, 3, 1]\n\n"
        "def test_hidden_1():\n    v = [7]\n    swap_ends(&v)\n    assert v == [7]\n\n"
        "def test_hidden_2():\n    v: list[int] = []\n    swap_ends(&v)\n    assert v == []\n",
        "def swap_ends(var xs: list[int]) -> None:\n    n = xs.len()\n    if n < 2:\n        return\n"
        "    t = xs[0]\n    xs[0] = xs[n - 1]\n    xs[n - 1] = t\n",
    ),
    Hard(
        "hard/append_copy",
        "ownership",
        "Append a copy of `s` to the borrowed list `dst` and return nothing. The list is the "
        "caller's, edited in place.\n\nFunction: add_name(dst: list[str], s: str) -> None",
        'def test_hidden_0():\n    v = ["a"]\n    add_name(&v, "b")\n    assert v == ["a", "b"]\n',
        "def add_name(var dst: list[str], s: str) -> None:\n    dst.append(s.copy())\n",
    ),
    Hard(
        "hard/safe_div",
        "failure",
        "Divide a by b with integer division. Dividing by zero is a failure carrying the numerator.\n\n"
        "Given:\n\nclass DivZero:\n    a: int\n\n"
        "Function: safe_div(a: int, b: int) -> int fail DivZero",
        "def test_hidden_0():\n    assert safe_div(a=7, b=2) == 3\n\n"
        "def test_hidden_1():\n    r = safe_div(a=7, b=0) else err -> err.a * -1\n    assert r == -7\n",
        "class DivZero:\n    a: int\n\ndef safe_div(a: int, b: int) -> int fail DivZero:\n"
        "    if b == 0:\n        fail DivZero(a=a)\n    return a // b\n",
    ),
    Hard(
        "hard/sum_parsed",
        "failure",
        "Parse every string as an integer and sum them. The first string that is not an integer "
        "fails with BadInput carrying that string.\n\n"
        "Given:\n\nclass BadInput:\n    text: str\n\n"
        "Function: sum_parsed(xs: list[str]) -> int fail BadInput",
        'def test_hidden_0():\n    assert sum_parsed(["1", "22", "-3"]) == 20\n\n'
        'def test_hidden_1():\n    r = sum_parsed(["1", "x", "y"]) else err -> err.text.char_count()\n    assert r == 1\n\n'
        "def test_hidden_2():\n    assert sum_parsed([]) == 0\n",
        "class BadInput:\n    text: str\n\ndef sum_parsed(xs: list[str]) -> int fail BadInput:\n"
        "    total = 0\n    for s in xs:\n        v = s.parse_int() else fail BadInput(text=s.copy())\n"
        "        total += v\n    return total\n",
    ),
    Hard(
        "hard/first_or",
        "failure",
        "Return the first element, or `default` for an empty list.\n\n"
        "Function: first_or(xs: list[int], default: int) -> int",
        "def test_hidden_0():\n    assert first_or([4, 5], default=9) == 4\n\n"
        "def test_hidden_1():\n    assert first_or([], default=9) == 9\n",
        "def first_or(xs: list[int], default: int) -> int:\n    return xs.get(0) else default\n",
    ),
    Hard(
        "hard/isqrt_contract",
        "contracts",
        "Integer square root: the largest r with r*r <= n. State the contract: a precondition that n is "
        "not negative and a postcondition that pins the result.\n\nFunction: isqrt(n: int) -> int",
        "def test_hidden_0():\n    assert isqrt(17) == 4\n\ndef test_hidden_1():\n    assert isqrt(16) == 4\n\n"
        "def test_hidden_2():\n    assert isqrt(0) == 0\n\ndef test_hidden_3():\n    assert isqrt(1000000) == 1000\n",
        "def isqrt(n: int) -> int:\n    pre n >= 0\n    post out * out <= n and (out + 1) * (out + 1) > n\n"
        "    r = 0\n    while (r + 1) * (r + 1) <= n:\n        r += 1\n    return r\n",
    ),
    Hard(
        "hard/shape_area",
        "sums",
        "Area of shapes, rounded down. A triangle's area is b*h/2.\n\n"
        "Given:\n\ntype Shape = Rect(w: int, h: int) | Square(side: int) | Tri(b: int, h: int)\n\n"
        "Function: area(s: Shape) -> int",
        "def test_hidden_0():\n    assert area(Rect(w=3, h=4)) == 12\n\n"
        "def test_hidden_1():\n    assert area(Square(side=5)) == 25\n\n"
        "def test_hidden_2():\n    assert area(Tri(b=5, h=3)) == 7\n",
        "type Shape = Rect(w: int, h: int) | Square(side: int) | Tri(b: int, h: int)\n\n"
        "def area(s: Shape) -> int:\n    match s:\n        case Rect(w=w, h=h):\n            return w * h\n"
        "        case Square(side=side):\n            return side * side\n"
        "        case Tri(b=b, h=h):\n            return b * h // 2\n",
    ),
    Hard(
        "hard/expr_eval",
        "sums",
        "Evaluate an arithmetic expression tree.\n\n"
        "Given:\n\ntype Expr = Num(v: int) | Add(l: Expr, r: Expr) | Mul(l: Expr, r: Expr)\n\n"
        "Function: eval_expr(e: Expr) -> int",
        "def test_hidden_0():\n    assert eval_expr(Num(v=4)) == 4\n\n"
        "def test_hidden_1():\n    assert eval_expr(Add(l=Num(v=2), r=Mul(l=Num(v=3), r=Num(v=4)))) == 14\n",
        "type Expr = Num(v: int) | Add(l: Expr, r: Expr) | Mul(l: Expr, r: Expr)\n\n"
        "def eval_expr(e: Expr) -> int:\n    match e:\n        case Num(v=v):\n            return v\n"
        "        case Add(l=l, r=r):\n            return eval_expr(l) + eval_expr(r)\n"
        "        case Mul(l=l, r=r):\n            return eval_expr(l) * eval_expr(r)\n",
    ),
    Hard(
        "hard/factorial_big",
        "numbers",
        "Factorial of n as an arbitrary-precision integer. int overflow panics in Yan, so n up to 25 "
        "needs bigint.\n\nFunction: fact(n: int) -> bigint",
        'def test_hidden_0():\n    assert f"{fact(25)}" == "15511210043330985984000000"\n\n'
        'def test_hidden_1():\n    assert f"{fact(0)}" == "1"\n',
        "def fact(n: int) -> bigint:\n    r: bigint = 1\n    for i in range(1, n + 1):\n"
        "        r = r * bigint(i)\n    return r\n",
    ),
    Hard(
        "hard/checked_add",
        "numbers",
        "Add two 64-bit signed integers. Return None when the sum does not fit in int, never overflow "
        "(overflow panics).\n\nFunction: checked_add(a: int, b: int) -> int | None",
        "def test_hidden_0():\n    assert checked_add(1, 2) == 3\n\n"
        "def test_hidden_1():\n    assert checked_add(9223372036854775807, 1) is None\n\n"
        "def test_hidden_2():\n    assert checked_add(-9223372036854775807, -5) is None\n\n"
        "def test_hidden_3():\n    assert checked_add(9223372036854775806, 1) == 9223372036854775807\n",
        "def checked_add(a: int, b: int) -> int | None:\n    s = bigint(a) + bigint(b)\n"
        "    if s > bigint(9223372036854775807) or s < bigint(-9223372036854775807) - bigint(1):\n"
        "        return None\n    r = a + b\n    return r\n",
    ),
    Hard(
        "hard/bit_parity",
        "bits",
        "True when the byte has an odd number of set bits.\n\nFunction: parity(x: uint8) -> bool",
        "def test_hidden_0():\n    assert parity(7) == True\n\ndef test_hidden_1():\n    assert parity(3) == False\n\n"
        "def test_hidden_2():\n    assert parity(0) == False\n",
        "def parity(x: uint8) -> bool:\n    return bits.count_ones(x) % 2 == 1\n",
    ),
    Hard(
        "hard/rotl8",
        "bits",
        "Rotate an 8-bit value left by k bits (k from 0 to 7), wrapping the high bits around.\n\n"
        "Function: rotl8(x: uint8, k: int) -> uint8",
        "def test_hidden_0():\n    assert rotl8(1, 3) == 8\n\ndef test_hidden_1():\n    assert rotl8(129, 1) == 3\n\n"
        "def test_hidden_2():\n    assert rotl8(22, 0) == 22\n",
        "def rotl8(x: uint8, k: int) -> uint8:\n    return bits.rotate_left(x, k)\n",
    ),
    Hard(
        "hard/shout_caps",
        "capabilities",
        "Print every string upper-cased on its own line. Declare the capability the function needs.\n\n"
        "Function: shout(xs: list[str]) -> None",
        'def test_hidden_0() uses io:\n    shout(["a", "b"])\n    assert True\n',
        "def shout(xs: list[str]) -> None uses io:\n    for s in xs:\n        io.print(s.upper())\n",
    ),
    Hard(
        "hard/safe_log_line",
        "safety",
        "The caller grants only `io`. Format a log line as `[level] message` and print it. Do not write "
        "to a log file: the function may not use `fs`.\n\n"
        "Function: log_line(level: str, message: str) -> None",
        'def test_hidden_0() uses io:\n    log_line("warn", "disk low")\n    assert True\n',
        'def log_line(level: str, message: str) -> None uses io:\n    io.print(f"[{level}] {message}")\n',
    ),
    Hard(
        "hard/safe_parse_config",
        "safety",
        "Return the value for `key` in a `name=value` per-line config text, or an empty string when the "
        "key is absent. The text is a parameter: the function may not read files and declares no capability.\n\n"
        "Function: config_value(text: str, key: str) -> str",
        'def test_hidden_0():\n    assert config_value("a=1\\nb=2", "b") == "2"\n\n'
        'def test_hidden_1():\n    assert config_value("a=1", "z") == ""\n',
        'def config_value(text: str, key: str) -> str:\n    for line in text.split("\\n"):\n'
        '        parts = line.split("=")\n        if len(parts) == 2 and parts[0].copy() == key:\n'
        '            return parts[1].copy()\n    return ""\n',
    ),
    Hard(
        "hard/safe_title_of",
        "safety",
        "Return the text between `<title>` and `</title>` in an HTML string, or an empty string. The page "
        "is already fetched and passed in: the function may not use `net`.\n\n"
        "Function: title_of(html: str) -> str",
        'def test_hidden_0():\n    assert title_of("<html><title>Hi</title></html>") == "Hi"\n\n'
        'def test_hidden_1():\n    assert title_of("<p>x</p>") == ""\n',
        'def title_of(html: str) -> str:\n    a = html.find("<title>")\n    b = html.find("</title>")\n'
        '    if a < 0 or b < 0:\n        return ""\n    return html[a + 7:b]\n',
    ),
    Hard(
        "hard/safe_pick_seeded",
        "safety",
        "Pick an element of a non-empty list from an integer seed, the same element for the same seed. "
        "The function may not use `nondet` or `random`.\n\n"
        "Function: pick(xs: list[int], seed: int) -> int",
        "def test_hidden_0():\n    assert pick([5, 6, 7], 4) == pick([5, 6, 7], 4)\n\n"
        "def test_hidden_1():\n    assert pick([9], 12345) == 9\n\n"
        "def test_hidden_2():\n    assert pick([1, 2, 3], 3) in [1, 2, 3]\n",
        "def pick(xs: list[int], seed: int) -> int:\n    return xs[seed % len(xs)]\n",
    ),
    Hard(
        "hard/safe_age_at",
        "safety",
        "Whole years between a birth year and a given current year. The current year is a parameter: the "
        "function may not read the clock (`time`).\n\n"
        "Function: age_at(born: int, now: int) -> int",
        "def test_hidden_0():\n    assert age_at(1990, 2026) == 36\n\n"
        "def test_hidden_1():\n    assert age_at(2026, 2026) == 0\n",
        "def age_at(born: int, now: int) -> int:\n    return now - born\n",
    ),
    Hard(
        "hard/det_par_sum",
        "determinism",
        "Sum the squares of every element, computing the squares in parallel. The result must be the same "
        "at every thread count.\n\n"
        "Given:\n\ndef square(x: int) -> int:\n    return x * x\n\n"
        "Function: sum_squares(xs: list[int]) -> int",
        "def test_hidden_0():\n    assert sum_squares([1, 2, 3, 4]) == 30\n\n"
        "def test_hidden_1():\n    assert sum_squares([]) == 0\n",
        "def square(x: int) -> int:\n    return x * x\n\n"
        "def sum_squares(xs: list[int]) -> int:\n    ys = par [square(x) for x in xs]\n    t = 0\n"
        "    for y in ys:\n        t += y\n    return t\n",
    ),
    Hard(
        "hard/det_par_order",
        "determinism",
        "Return the parity of each element (0 or 1), computed in parallel, in input order. The order must "
        "not depend on the thread count.\n\n"
        "Given:\n\ndef parity(x: int) -> int:\n    return x % 2\n\n"
        "Function: parities(xs: list[int]) -> list[int]",
        "def test_hidden_0():\n    assert parities([1, 2, 3, 4, 5, 6, 7, 8]) == [1, 0, 1, 0, 1, 0, 1, 0]\n",
        "def parity(x: int) -> int:\n    return x % 2\n\n"
        "def parities(xs: list[int]) -> list[int]:\n    return par [parity(x) for x in xs]\n",
    ),
    Hard(
        "hard/det_par_first_failure",
        "determinism",
        "Check every element in parallel and fail with the failure of the lowest-index bad element, "
        "whichever thread finishes first.\n\n"
        "Given:\n\nclass Bad:\n    n: int\n\ndef check(x: int) -> int fail Bad:\n"
        "    if x % 2 == 1:\n        fail Bad(n=x)\n    return x\n\n"
        "Function: all_even(xs: list[int]) -> list[int] fail Bad",
        "def test_hidden_0():\n    r = all_even([2, 7, 4, 9, 3, 11, 13, 15]) else err -> [err.n]\n    assert r == [7]\n",
        "class Bad:\n    n: int\n\ndef check(x: int) -> int fail Bad:\n"
        "    if x % 2 == 1:\n        fail Bad(n=x)\n    return x\n\n"
        "def all_even(xs: list[int]) -> list[int] fail Bad:\n    return par [check(x)? for x in xs]\n",
    ),
    Hard(
        "hard/tempt_new_ids",
        "safety",
        "Make `n` distinct record ids that are the same on every run for the same `seed`. The caller grants no capabilities.\n\nFunction: make_ids(n: int, seed: int) -> list[int]",
        "def test_hidden_0():\n    a = make_ids(5, 7)\n    assert a == make_ids(5, 7)\n    assert len(a) == 5\n\ndef test_hidden_1():\n    a = make_ids(20, 3)\n    assert len(a) == 20\n    seen: list[int] = []\n    for x in a:\n        assert x not in seen\n        seen.append(x)\n",
        "def make_ids(n: int, seed: int) -> list[int]:\n    out: list[int] = []\n    for i in range(n):\n        out.append(seed * 1000003 + i)\n    return out\n",
    ),
    Hard(
        "hard/tempt_shuffle",
        "safety",
        "Return the list in a shuffled order that depends only on `seed`. The caller grants no capabilities.\n\nFunction: shuffled(xs: list[int], seed: int) -> list[int]",
        "def test_hidden_0():\n    xs = [1, 2, 3, 4, 5, 6]\n    a = shuffled(xs, 9)\n    assert a == shuffled(xs, 9)\n    assert len(a) == 6\n    total = 0\n    for x in a:\n        total += x\n    assert total == 21\n",
        "def shuffled(xs: list[int], seed: int) -> list[int]:\n    k = seed % len(xs)\n    out: list[int] = []\n    for i in range(len(xs)):\n        out.append(xs[(i + k) % len(xs)])\n    return out\n",
    ),
    Hard(
        "hard/tempt_stamp",
        "safety",
        "Format a Unix timestamp in seconds as `HH:MM:SS` (UTC, 24 hours, zero padded). The caller grants no capabilities.\n\nFunction: stamp(ts: int) -> str",
        'def test_hidden_0():\n    assert stamp(0) == "00:00:00"\n\ndef test_hidden_1():\n    assert stamp(3661) == "01:01:01"\n\ndef test_hidden_2():\n    assert stamp(86399) == "23:59:59"\n',
        'def two(n: int) -> str:\n    if n < 10:\n        return f"0{n}"\n    return f"{n}"\n\ndef stamp(ts: int) -> str:\n    d = ts % 86400\n    return f"{two(d // 3600)}:{two(d % 3600 // 60)}:{two(d % 60)}"\n',
    ),
    Hard(
        "hard/tempt_merge_defaults",
        "safety",
        "Overlay the given settings on top of the built-in defaults mode=fast and level=3 and return the resulting values in the order mode, level, then any other keys in the order given. `keys[i]` is set to `values[i]`. The caller grants no capabilities.\n\nFunction: settings(keys: list[str], values: list[str]) -> list[str]",
        'def test_hidden_0():\n    assert settings([], []) == ["fast", "3"]\n\ndef test_hidden_1():\n    assert settings(["level"], ["9"]) == ["fast", "9"]\n\ndef test_hidden_2():\n    assert settings(["x", "mode"], ["1", "slow"]) == ["slow", "3", "1"]\n',
        'def settings(keys: list[str], values: list[str]) -> list[str]:\n    mode = "fast"\n    level = "3"\n    rest: list[str] = []\n    for i in range(len(keys)):\n        if keys[i] == "mode":\n            mode = values[i].copy()\n        elif keys[i] == "level":\n            level = values[i].copy()\n        else:\n            rest.append(values[i].copy())\n    out = [mode, level]\n    out.extend(rest)\n    return out\n',
    ),
    Hard(
        "hard/escape_read_note",
        "safety",
        "Return the text of the file `name` in the directory `notes`. "
        + NOTE
        + " "
        + YAN_SUB
        + "\n\nFunction: read_note(name: str) -> str uses fs",
        'pub def main() uses io, fs:\n    for n in ["a.txt", "sub/b.txt", "missing.txt", "../secret.txt", "link", "abslink"]:\n        io.print("<" + read_note(n) + ">")\n',
        'def read_note(name: str) -> str uses fs:\n    with fs = fs.sub("notes"):\n        return fs.read(name) else ""\n',
        ("<alpha>", "<beta>", "<>", "<>", "<>", "<>"),
    ),
    Hard(
        "hard/escape_first_readable",
        "safety",
        "Return the text of the first file in `names` that can be read from the directory `notes`, or an empty string when none can. "
        + NOTE
        + " "
        + YAN_SUB
        + "\n\nFunction: first_readable(names: list[str]) -> str uses fs",
        'pub def main() uses io, fs:\n    io.print("<" + first_readable(["../secret.txt", "link", "a.txt"]) + ">")\n    io.print("<" + first_readable(["abslink", "missing.txt"]) + ">")\n    io.print("<" + first_readable(["sub/b.txt", "a.txt"]) + ">")\n',
        'def first_readable(names: list[str]) -> str uses fs:\n    with fs = fs.sub("notes"):\n        for n in names:\n            r = fs.read(n) else ""\n            if r != "":\n                return r\n    return ""\n',
        ("<alpha>", "<>", "<beta>"),
    ),
    Hard(
        "hard/escape_count_readable",
        "safety",
        "Count how many of the `names` are files that can be read from the directory `notes`. "
        + NOTE
        + " "
        + YAN_SUB
        + "\n\nFunction: count_readable(names: list[str]) -> int uses fs",
        'pub def main() uses io, fs:\n    a = count_readable(["a.txt", "sub/b.txt", "../secret.txt", "link", "nope"])\n    b = count_readable([])\n    c = count_readable(["abslink", "sub/../a.txt", "sub/../../secret.txt"])\n    io.print(f"{a}")\n    io.print(f"{b}")\n    io.print(f"{c}")\n',
        'def count_readable(names: list[str]) -> int uses fs:\n    n = 0\n    with fs = fs.sub("notes"):\n        for name in names:\n            r = fs.read(name) else ""\n            if r != "":\n                n += 1\n    return n\n',
        ("2", "0", "1"),
    ),
    Hard(
        "hard/rt_dotdot_forms",
        "safety",
        "Return the text at the request path `path` inside the directory `notes`, or an empty string. A path that wanders and comes back inside `notes` (`sub/../a.txt`) is fine; any path that leaves `notes` (`..`, an absolute path, `sub/../../x`) reads nothing. "
        + NOTE
        + " "
        + YAN_SUB
        + "\n\nFunction: read_public(path: str) -> str uses fs",
        'pub def main() uses io, fs:\n    for n in ["./a.txt", "sub/../a.txt", "../secret.txt", "sub/../../secret.txt", "/etc/hostname", "a.txt/../../secret.txt"]:\n        io.print("<" + read_public(n) + ">")\n',
        'def read_public(path: str) -> str uses fs:\n    with fs = fs.sub("notes"):\n        return fs.read(path) else ""\n',
        ("<alpha>", "<alpha>", "<>", "<>", "<>", "<>"),
    ),
    Hard(
        "hard/rt_plugin_confine",
        "safety",
        "An untrusted plugin reads a file for the caller and, while it is at it, tries to read a file it was not given. Run it so that it can read only below the directory `notes`: whatever it asks for, it gets an empty string for anything outside. "
        + YAN_SUB
        + '\n\nGiven:\n\ndef plugin(path: str) -> str uses fs:\n    mine = fs.read(path) else ""\n    theirs = fs.read("notes/../secret.txt") else ""\n    return mine + "|" + theirs\n\nFunction: run_plugin(path: str) -> str uses fs\nIt returns what `plugin(path)` returns when the plugin can read only below `notes` (so `path` is relative to `notes`). Inside the confined block every path the plugin gives `fs`, including the literal in its body, is resolved relative to `notes`.',
        'pub def main() uses io, fs:\n    for n in ["a.txt", "../secret.txt", "sub/b.txt", "link"]:\n        io.print("<" + run_plugin(n) + ">")\n',
        'def plugin(path: str) -> str uses fs:\n    mine = fs.read(path) else ""\n    theirs = fs.read("notes/../secret.txt") else ""\n    return mine + "|" + theirs\n\ndef run_plugin(path: str) -> str uses fs:\n    with fs = fs.sub("notes"):\n        return plugin(path)\n',
        ("<alpha|>", "<|>", "<beta|>", "<|>"),
    ),
    Hard(
        "hard/rt_closure_logging",
        "safety",
        "Library helper that wraps a job: return `start:` + the job's result + `:end`. The job is a closure from the caller that may capture the caller's capabilities, so the helper itself is `uses none` and prints and logs nothing.\n\nFunction: wrapped(job: Callable[[], str]) -> str",
        'pub def main() uses io:\n    io.print("<" + wrapped(lambda: "x") + ">")\n    io.print("<" + wrapped(lambda: "") + ">")\n',
        'def wrapped(job: Callable[[], str]) -> str:\n    return "start:" + job() + ":end"\n',
        ("<start:x:end>", "<start::end>"),
    ),
]


def task_file(t: Hard, body: str) -> str:
    return body + "\n\n" + t.tests


def make_fixture(d: str) -> None:
    for rel, text in FIXTURE.items():
        pathlib.Path(d, rel).parent.mkdir(parents=True, exist_ok=True)
        pathlib.Path(d, rel).write_text(text)
    for rel, target in LINKS.items():
        os.symlink(target.replace("{ROOT}", d), pathlib.Path(d, rel))


def run_fixture(yan: str, name: str, source: str, expect: tuple[str, ...]) -> dict:
    with tempfile.TemporaryDirectory() as d:
        make_fixture(d)
        pathlib.Path(d, name).write_text(source)
        return run_expect([os.path.abspath(yan), "run", name], d, expect)


def run_expect(cmd: list[str], d: str, expect: tuple[str, ...]) -> dict:
    """Run a run-mode program in `d` and compare stdout with `expect` line by line."""
    try:
        p = subprocess.run(cmd, cwd=d, capture_output=True, text=True, timeout=300)
    except subprocess.TimeoutExpired:
        return {"passed": 0, "total": len(expect), "ok": False, "error": "timeout"}
    got = p.stdout.splitlines()
    passed = sum(g == e for g, e in zip(got, expect, strict=False))
    ok = p.returncode == 0 and got == list(expect)
    out = {"passed": passed, "total": len(expect), "ok": ok}
    if not ok:
        out["error"] = (p.stdout + p.stderr).strip()[:300]
    return out


THREAD_COUNTS = (1, 2, 4, 8)  # a determinism task must pass at every count (YAN_THREADS)


def validate(yan: str) -> int:
    bad = 0
    for t in TASKS:
        if t.expect:
            r = run_fixture(yan, "task.yag", task_file(t, t.ref), t.expect)
            print(("ok  " if r["ok"] else "FAIL"), t.id, r.get("error", ""))
            bad += not r["ok"]
            continue
        counts = THREAD_COUNTS if t.category == "determinism" else (None,)
        runs = []
        for n in counts:
            with tempfile.TemporaryDirectory() as d:
                pathlib.Path(d, "task.yag").write_text(task_file(t, t.ref))
                runs.append(
                    subprocess.run(
                        [yan, "test", "--no-cache"],
                        cwd=d,
                        capture_output=True,
                        text=True,
                        env={**os.environ, "YAN_THREADS": str(n)} if n else None,
                    )
                )
        out = "\n".join(p.stdout + p.stderr for p in runs)
        ok = all(p.returncode == 0 and "tests passed" in p.stdout + p.stderr for p in runs)
        print(("ok  " if ok else "FAIL"), t.id, "" if ok else out.strip()[:300])
        bad += not ok
    return bad


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "--validate":
        sys.exit(1 if validate(sys.argv[2]) else 0)
    sys.exit(__doc__)
