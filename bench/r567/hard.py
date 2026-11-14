"""Yan-specific hard tasks for the R567 harness: concurrency, ownership, failure, contracts, sums,
numbers, bits, capabilities. HumanEval and MBPP translate Python algorithms and cannot probe these.

Each task has a prompt, hidden Yan tests (appended to the model's file and run by `yan test`) and a
reference solution. The reference is never shown to a model; `--validate` checks that it passes the
hidden tests, so a task that no solution can pass never ships.

usage: uv run python bench/r567/hard.py --validate target/release/yan
"""

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


TASKS = [
    Hard(
        "hard/par_lowest_failure", "concurrency",
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
        "hard/par_squares", "concurrency",
        "Square every element in parallel.\n\n"
        "Given:\n\ndef square(x: int) -> int:\n    return x * x\n\n"
        "Function: squares(xs: list[int]) -> list[int]",
        "def test_hidden_0():\n    assert squares([1, 2, 3]) == [1, 4, 9]\n\n"
        "def test_hidden_1():\n    assert squares([]) == []\n",
        "def square(x: int) -> int:\n    return x * x\n\n"
        "def squares(xs: list[int]) -> list[int]:\n    return par [square(x) for x in xs]\n",
    ),
    Hard(
        "hard/channel_capacity", "concurrency",
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
        "hard/inout_bump", "ownership",
        "Add `by` to every element of the list, in place, so the caller sees the change.\n\n"
        "Function: bump_all(xs: list[int], by: int) -> None",
        "def test_hidden_0():\n    v = [1, 2, 3]\n    bump_all(&v, 10)\n    assert v == [11, 12, 13]\n\n"
        "def test_hidden_1():\n    v: list[int] = []\n    bump_all(&v, 1)\n    assert v == []\n",
        "def bump_all(var xs: list[int], by: int) -> None:\n"
        "    for i in range(xs.len()):\n        xs[i] += by\n",
    ),
    Hard(
        "hard/take_longest", "ownership",
        "Consume both lists and return the longer one; the first wins a tie.\n\n"
        "Function: longest(a: list[int], b: list[int]) -> list[int]",
        "def test_hidden_0():\n    assert longest([1], [1, 2]) == [1, 2]\n\n"
        "def test_hidden_1():\n    assert longest([3, 4], [5, 6]) == [3, 4]\n",
        "def longest(take a: list[int], take b: list[int]) -> list[int]:\n"
        "    if a.len() >= b.len():\n        return a\n    return b\n",
    ),
    Hard(
        "hard/swap_ends", "ownership",
        "Swap the first and last elements of the list in place. An empty or one-element list is "
        "unchanged.\n\nFunction: swap_ends(xs: list[int]) -> None",
        "def test_hidden_0():\n    v = [1, 2, 3, 4]\n    swap_ends(&v)\n    assert v == [4, 2, 3, 1]\n\n"
        "def test_hidden_1():\n    v = [7]\n    swap_ends(&v)\n    assert v == [7]\n\n"
        "def test_hidden_2():\n    v: list[int] = []\n    swap_ends(&v)\n    assert v == []\n",
        "def swap_ends(var xs: list[int]) -> None:\n    n = xs.len()\n    if n < 2:\n        return\n"
        "    t = xs[0]\n    xs[0] = xs[n - 1]\n    xs[n - 1] = t\n",
    ),
    Hard(
        "hard/append_copy", "ownership",
        "Append a copy of `s` to the borrowed list `dst` and return nothing. The list is the "
        "caller's, edited in place.\n\nFunction: add_name(dst: list[str], s: str) -> None",
        "def test_hidden_0():\n    v = [\"a\"]\n    add_name(&v, \"b\")\n    assert v == [\"a\", \"b\"]\n",
        "def add_name(var dst: list[str], s: str) -> None:\n    dst.append(s.copy())\n",
    ),
    Hard(
        "hard/safe_div", "failure",
        "Divide a by b with integer division. Dividing by zero is a failure carrying the numerator.\n\n"
        "Given:\n\nclass DivZero:\n    a: int\n\n"
        "Function: safe_div(a: int, b: int) -> int fail DivZero",
        "def test_hidden_0():\n    assert safe_div(a=7, b=2) == 3\n\n"
        "def test_hidden_1():\n    r = safe_div(a=7, b=0) else err -> err.a * -1\n    assert r == -7\n",
        "class DivZero:\n    a: int\n\ndef safe_div(a: int, b: int) -> int fail DivZero:\n"
        "    if b == 0:\n        fail DivZero(a=a)\n    return a // b\n",
    ),
    Hard(
        "hard/sum_parsed", "failure",
        "Parse every string as an integer and sum them. The first string that is not an integer "
        "fails with BadInput carrying that string.\n\n"
        "Given:\n\nclass BadInput:\n    text: str\n\n"
        "Function: sum_parsed(xs: list[str]) -> int fail BadInput",
        "def test_hidden_0():\n    assert sum_parsed([\"1\", \"22\", \"-3\"]) == 20\n\n"
        "def test_hidden_1():\n    r = sum_parsed([\"1\", \"x\", \"y\"]) else err -> err.text.char_count()\n    assert r == 1\n\n"
        "def test_hidden_2():\n    assert sum_parsed([]) == 0\n",
        "class BadInput:\n    text: str\n\ndef sum_parsed(xs: list[str]) -> int fail BadInput:\n"
        "    total = 0\n    for s in xs:\n        v = s.parse_int() else fail BadInput(text=s.copy())\n"
        "        total += v\n    return total\n",
    ),
    Hard(
        "hard/first_or", "failure",
        "Return the first element, or `default` for an empty list.\n\n"
        "Function: first_or(xs: list[int], default: int) -> int",
        "def test_hidden_0():\n    assert first_or([4, 5], default=9) == 4\n\n"
        "def test_hidden_1():\n    assert first_or([], default=9) == 9\n",
        "def first_or(xs: list[int], default: int) -> int:\n    return xs.get(0) else default\n",
    ),
    Hard(
        "hard/isqrt_contract", "contracts",
        "Integer square root: the largest r with r*r <= n. State the contract: a precondition that n is "
        "not negative and a postcondition that pins the result.\n\nFunction: isqrt(n: int) -> int",
        "def test_hidden_0():\n    assert isqrt(17) == 4\n\ndef test_hidden_1():\n    assert isqrt(16) == 4\n\n"
        "def test_hidden_2():\n    assert isqrt(0) == 0\n\ndef test_hidden_3():\n    assert isqrt(1000000) == 1000\n",
        "def isqrt(n: int) -> int:\n    pre n >= 0\n    post out * out <= n and (out + 1) * (out + 1) > n\n"
        "    r = 0\n    while (r + 1) * (r + 1) <= n:\n        r += 1\n    return r\n",
    ),
    Hard(
        "hard/shape_area", "sums",
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
        "hard/expr_eval", "sums",
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
        "hard/factorial_big", "numbers",
        "Factorial of n as an arbitrary-precision integer. int overflow panics in Yan, so n up to 25 "
        "needs bigint.\n\nFunction: fact(n: int) -> bigint",
        "def test_hidden_0():\n    assert f\"{fact(25)}\" == \"15511210043330985984000000\"\n\n"
        "def test_hidden_1():\n    assert f\"{fact(0)}\" == \"1\"\n",
        "def fact(n: int) -> bigint:\n    r: bigint = 1\n    for i in range(1, n + 1):\n"
        "        r = r * bigint(i)\n    return r\n",
    ),
    Hard(
        "hard/checked_add", "numbers",
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
        "hard/bit_parity", "bits",
        "True when the byte has an odd number of set bits.\n\nFunction: parity(x: uint8) -> bool",
        "def test_hidden_0():\n    assert parity(7) == True\n\ndef test_hidden_1():\n    assert parity(3) == False\n\n"
        "def test_hidden_2():\n    assert parity(0) == False\n",
        "def parity(x: uint8) -> bool:\n    return bits.count_ones(x) % 2 == 1\n",
    ),
    Hard(
        "hard/rotl8", "bits",
        "Rotate an 8-bit value left by k bits (k from 0 to 7), wrapping the high bits around.\n\n"
        "Function: rotl8(x: uint8, k: int) -> uint8",
        "def test_hidden_0():\n    assert rotl8(1, 3) == 8\n\ndef test_hidden_1():\n    assert rotl8(129, 1) == 3\n\n"
        "def test_hidden_2():\n    assert rotl8(22, 0) == 22\n",
        "def rotl8(x: uint8, k: int) -> uint8:\n    return bits.rotate_left(x, k)\n",
    ),
    Hard(
        "hard/shout_caps", "capabilities",
        "Print every string upper-cased on its own line. Declare the capability the function needs.\n\n"
        "Function: shout(xs: list[str]) -> None",
        "def test_hidden_0() uses io:\n    shout([\"a\", \"b\"])\n    assert True\n",
        "def shout(xs: list[str]) -> None uses io:\n    for s in xs:\n        io.print(s.upper())\n",
    ),
]


def task_file(t: Hard, body: str) -> str:
    return body + "\n\n" + t.tests


def validate(yan: str) -> int:
    bad = 0
    for t in TASKS:
        with tempfile.TemporaryDirectory() as d:
            pathlib.Path(d, "task.yag").write_text(task_file(t, t.ref))
            p = subprocess.run([yan, "test", "--no-cache"], cwd=d, capture_output=True, text=True)
        out = p.stdout + p.stderr
        ok = p.returncode == 0 and "tests passed" in out
        print(("ok  " if ok else "FAIL"), t.id, "" if ok else out.strip()[:300])
        bad += not ok
    return bad


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "--validate":
        sys.exit(1 if validate(sys.argv[2]) else 0)
    sys.exit(__doc__)
