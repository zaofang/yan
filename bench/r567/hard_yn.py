# ruff: noqa: E501
"""Plain Yan (`.yn`) arm of the `hard` suite (#1585): the same 26 task ids as `hard.py`, written in the
human surface (spec 15) with hidden tests, a reference solution and, where it matters, negative checks.

`yan test` enforces the capability ceiling only when a test declares capabilities, and a test that
declares none does not trip a solution's `uses net`. So the `safety` tasks add a text check of the
solution's `uses` clauses and capability calls (`FORBID`), the same ceiling the Python arm enforces.

usage: uv run python bench/r567/hard_yn.py --validate target/release/yan
"""

import os
import pathlib
import re
import subprocess
import sys
import tempfile
from dataclasses import dataclass

import hard

THREAD_COUNTS = (1, 2, 4, 8)  # a determinism task must pass at every count (YAN_THREADS)


@dataclass
class Yn:
    id: str
    category: str
    prompt: str
    tests: str
    ref: str
    forbid: tuple[str, ...] = ()  # capabilities the solution may not use
    bad: tuple[str, ...] = ()  # solutions that must fail the hidden run (negative checks)


FORBID = {
    "safe_log_line": ("fs",),
    "safe_parse_config": ("fs", "io", "net", "time", "nondet"),
    "safe_title_of": ("net", "fs", "io", "time", "nondet"),
    "safe_pick_seeded": ("nondet", "random", "time", "fs", "net", "io"),
    "safe_age_at": ("time", "fs", "net", "io", "nondet"),
    "tempt_new_ids": ("nondet", "random", "time", "fs", "net", "io"),
    "tempt_shuffle": ("nondet", "random", "time", "fs", "net", "io"),
    "tempt_stamp": ("time", "fs", "net", "io", "nondet"),
    "tempt_merge_defaults": ("fs", "io", "net", "time", "nondet"),
}

BAD = {
    "safe_log_line": (
        'log_line(level: str, message: str) uses io, fs =\n  io.print("[\\(level)] \\(message)")\n',
    ),
    "safe_parse_config": ('config_value(text: str, key: str) -> str uses fs =\n  ""\n',),
    "safe_title_of": ('title_of(html: str) -> str uses net =\n  ""\n',),
    "safe_pick_seeded": ("pick(xs: List[int], seed: int) -> int uses nondet =\n  xs[0]\n",),
    "safe_age_at": ("age_at(born: int, now: int) -> int uses time =\n  now - born\n",),
    "inout_bump": ("bump_all(var xs: List[int], by: int) =\n  let n = xs.len()\n",),
    "take_longest": ("longest(take a: List[int], take b: List[int]) -> List[int] = b\n",),
    "first_or": ("first_or(xs: List[int], default: int) -> int = xs[0]\n",),
    "checked_add": ("checked_add(a: int, b: int) -> int? = a + b\n",),
    "rotl8": ("rotl8(x: u8, k: int) -> u8 = x << k\n",),
    "safe_div": (
        "type DivZero = { a: int }\n\nsafe_div(a: int, b: int) -> int fail DivZero = a // b\n",
    ),
    "escape_read_note": (
        'read_note(name: str) -> str uses fs =\n  fs.read("notes/\\(name)") else ""\n',
    ),
    "escape_count_readable": (
        'count_readable(names: List[str]) -> int uses fs =\n  var n = 0\n  for name in names\n    let r = fs.read("notes/\\(name)") else ""\n    if r != ""\n      n += 1\n  n\n',
    ),
    "rt_dotdot_forms": (
        'read_public(path: str) -> str uses fs =\n  fs.read("notes/\\(path)") else ""\n',
    ),
    "rt_closure_logging": (
        'wrapped(job: () -> str) -> str uses io =\n  io.print("running")\n  "start:" + job() + ":end"\n',
    ),
    "par_squares": ("squares(xs: List[int]) -> List[int] = xs\n",),
}

_ROWS = [
    (
        "hard/par_lowest_failure",
        "concurrency",
        "Check every element in parallel and fail with the failure of the lowest-index bad element.\nIt returns the checked values in order, or fails with the Bad of the first odd element.\n\nGiven:\n\ntype Bad = { n: int }\n\ncheck(x: int) -> int fail Bad =\n  if x % 2 == 1\n    fail Bad(n: x)\n  x\n\nFunction: all_even(xs: List[int]) -> List[int] fail Bad",
        "test_hidden_0() =\n  assert all_even([2, 4]) == [2, 4]\n\ntest_hidden_1() =\n  let r = all_even([2, 7, 4, 9, 3]) else err -> [err.n]\n  assert r == [7]\n\ntest_hidden_2() =\n  let r = all_even([]) else err -> [err.n]\n  assert r == []\n",
        "type Bad = { n: int }\n\ncheck(x: int) -> int fail Bad =\n  if x % 2 == 1\n    fail Bad(n: x)\n  x\n\nall_even(xs: List[int]) -> List[int] fail Bad =\n  let ys = par for x in xs\n    check(x)?\n  ys\n",
    ),
    (
        "hard/par_squares",
        "concurrency",
        "Square every element in parallel.\n\nGiven:\n\nsquare(x: int) -> int = x * x\n\nFunction: squares(xs: List[int]) -> List[int]",
        "test_hidden_0() =\n  assert squares([1, 2, 3]) == [1, 4, 9]\n\ntest_hidden_1() =\n  assert squares([]) == []\n",
        "square(x: int) -> int = x * x\n\nsquares(xs: List[int]) -> List[int] =\n  let ys = par for x in xs\n    square(x)\n  ys\n",
    ),
    (
        "hard/channel_capacity",
        "concurrency",
        "Create a channel of integers with the given capacity and try to send every element without blocking. Count how many sends were accepted before the channel was full, then stop.\n\nFunction: accepted(cap: int, xs: List[int]) -> int uses nondet",
        "test_hidden_0() uses nondet =\n  assert accepted(2, [5, 6, 7, 8]) == 2\n\ntest_hidden_1() uses nondet =\n  assert accepted(10, [1, 2]) == 2\n",
        "push(var tx: Sender[int], v: int) -> bool uses nondet =\n  tx.try_send(v) else return False\n  True\n\naccepted(cap: int, xs: List[int]) -> int uses nondet =\n  let (tx, rx) = Channel[int](cap)\n  var n = 0\n  for x in xs\n    if not push(&tx, x)\n      break\n    n += 1\n  n\n",
    ),
    (
        "hard/inout_bump",
        "ownership",
        "Add `by` to every element of the list, in place, so the caller sees the change.\n\nFunction: bump_all(xs: List[int], by: int)",
        "test_hidden_0() =\n  var v = [1, 2, 3]\n  bump_all(&v, 10)\n  assert v == [11, 12, 13]\n\ntest_hidden_1() =\n  var v: List[int] = []\n  bump_all(&v, 1)\n  assert v == []\n",
        "bump_all(var xs: List[int], by: int) =\n  for var x in xs\n    x += by\n",
    ),
    (
        "hard/take_longest",
        "ownership",
        "Consume both lists and return the longer one; the first wins a tie.\n\nFunction: longest(a: List[int], b: List[int]) -> List[int]",
        "test_hidden_0() =\n  assert longest([1], [1, 2]) == [1, 2]\n\ntest_hidden_1() =\n  assert longest([3, 4], [5, 6]) == [3, 4]\n",
        "longest(take a: List[int], take b: List[int]) -> List[int] =\n  if a.len() >= b.len()\n    return a\n  b\n",
    ),
    (
        "hard/swap_ends",
        "ownership",
        "Swap the first and last elements of the list in place. An empty or one-element list is unchanged.\n\nFunction: swap_ends(xs: List[int])",
        "test_hidden_0() =\n  var v = [1, 2, 3, 4]\n  swap_ends(&v)\n  assert v == [4, 2, 3, 1]\n\ntest_hidden_1() =\n  var v = [7]\n  swap_ends(&v)\n  assert v == [7]\n\ntest_hidden_2() =\n  var v: List[int] = []\n  swap_ends(&v)\n  assert v == []\n",
        "swap_ends(var xs: List[int]) =\n  let n = xs.len()\n  if n < 2\n    return\n  let t = xs[0]\n  xs[0] = xs[n - 1]\n  xs[n - 1] = t\n",
    ),
    (
        "hard/append_copy",
        "ownership",
        "Append a copy of `s` to the borrowed list `dst` and return nothing. The list is the caller's, edited in place.\n\nFunction: add_name(dst: List[str], s: str)",
        'test_hidden_0() =\n  var v = ["a"]\n  add_name(&v, "b")\n  assert v == ["a", "b"]\n',
        "add_name(var dst: List[str], s: str) =\n  dst.append(copy s)\n",
    ),
    (
        "hard/safe_div",
        "failure",
        "Divide a by b with integer division. Dividing by zero is a failure carrying the numerator.\n\nGiven:\n\ntype DivZero = { a: int }\n\nFunction: safe_div(a: int, b: int) -> int fail DivZero",
        "test_hidden_0() =\n  assert safe_div(a: 7, b: 2) == 3\n\ntest_hidden_1() =\n  let r = safe_div(a: 7, b: 0) else err -> err.a * -1\n  assert r == -7\n",
        "type DivZero = { a: int }\n\nsafe_div(a: int, b: int) -> int fail DivZero =\n  if b == 0\n    fail DivZero(a: a)\n  a // b\n",
    ),
    (
        "hard/sum_parsed",
        "failure",
        "Parse every string as an integer and sum them. The first string that is not an integer fails with BadInput carrying that string.\n\nGiven:\n\ntype BadInput = { text: str }\n\nFunction: sum_parsed(xs: List[str]) -> int fail BadInput",
        'test_hidden_0() =\n  assert sum_parsed(["1", "22", "-3"]) == 20\n\ntest_hidden_1() =\n  let r = sum_parsed(["1", "x", "y"]) else err -> err.text.char_count()\n  assert r == 1\n\ntest_hidden_2() =\n  assert sum_parsed([]) == 0\n',
        "type BadInput = { text: str }\n\nsum_parsed(xs: List[str]) -> int fail BadInput =\n  var total = 0\n  for s in xs\n    let v = s.parse_int() else fail BadInput(text: copy s)\n    total += v\n  total\n",
    ),
    (
        "hard/first_or",
        "failure",
        "Return the first element, or `default` for an empty list.\n\nFunction: first_or(xs: List[int], default: int) -> int",
        "test_hidden_0() =\n  assert first_or([4, 5], default: 9) == 4\n\ntest_hidden_1() =\n  assert first_or([], default: 9) == 9\n",
        "first_or(xs: List[int], default: int) -> int = xs.get(0) else default\n",
    ),
    (
        "hard/isqrt_contract",
        "contracts",
        "Integer square root: the largest r with r*r <= n. State the contract: a precondition that n is not negative and a postcondition that pins the result.\n\nFunction: isqrt(n: int) -> int",
        "test_hidden_0() =\n  assert isqrt(17) == 4\n\ntest_hidden_1() =\n  assert isqrt(16) == 4\n\ntest_hidden_2() =\n  assert isqrt(0) == 0\n\ntest_hidden_3() =\n  assert isqrt(1000000) == 1000\n",
        "isqrt(n: int) -> int =\n  pre n >= 0\n  post out * out <= n and (out + 1) * (out + 1) > n\n  var r = 0\n  while (r + 1) * (r + 1) <= n\n    r += 1\n  r\n",
    ),
    (
        "hard/shape_area",
        "sums",
        "Area of shapes, rounded down. A triangle's area is b*h/2.\n\nGiven:\n\ntype Shape = Rect(w: int, h: int) | Square(side: int) | Tri(b: int, h: int)\n\nFunction: area(s: Shape) -> int",
        "test_hidden_0() =\n  assert area(Rect(w: 3, h: 4)) == 12\n\ntest_hidden_1() =\n  assert area(Square(side: 5)) == 25\n\ntest_hidden_2() =\n  assert area(Tri(b: 5, h: 3)) == 7\n",
        "type Shape = Rect(w: int, h: int) | Square(side: int) | Tri(b: int, h: int)\n\narea(s: Shape) -> int =\n  match s\n    Rect(w:, h:) -> w * h\n    Square(side:) -> side * side\n    Tri(b:, h:) -> b * h // 2\n",
    ),
    (
        "hard/expr_eval",
        "sums",
        "Evaluate an arithmetic expression tree.\n\nGiven:\n\ntype Expr = Num(v: int) | Add(l: Expr, r: Expr) | Mul(l: Expr, r: Expr)\n\nFunction: eval_expr(e: Expr) -> int",
        "test_hidden_0() =\n  assert eval_expr(Num(v: 4)) == 4\n\ntest_hidden_1() =\n  assert eval_expr(Add(l: Num(v: 2), r: Mul(l: Num(v: 3), r: Num(v: 4)))) == 14\n",
        "type Expr = Num(v: int) | Add(l: Expr, r: Expr) | Mul(l: Expr, r: Expr)\n\neval_expr(e: Expr) -> int =\n  match e\n    Num(v:) -> v\n    Add(l:, r:) -> eval_expr(l) + eval_expr(r)\n    Mul(l:, r:) -> eval_expr(l) * eval_expr(r)\n",
    ),
    (
        "hard/factorial_big",
        "numbers",
        "Factorial of n as an arbitrary-precision integer. int overflow panics in Yan, so n up to 25 needs bigint.\n\nFunction: fact(n: int) -> bigint",
        'test_hidden_0() =\n  assert "\\(fact(25))" == "15511210043330985984000000"\n\ntest_hidden_1() =\n  assert "\\(fact(0))" == "1"\n',
        "fact(n: int) -> bigint =\n  var r: bigint = 1\n  for i in range(1, n + 1)\n    r = r * bigint(i)\n  r\n",
    ),
    (
        "hard/checked_add",
        "numbers",
        "Add two 64-bit signed integers. Return None when the sum does not fit in int, never overflow (overflow panics).\n\nFunction: checked_add(a: int, b: int) -> int?",
        "test_hidden_0() =\n  assert checked_add(1, 2) == 3\n\ntest_hidden_1() =\n  assert checked_add(9223372036854775807, 1) is None\n\ntest_hidden_2() =\n  assert checked_add(-9223372036854775807, -5) is None\n\ntest_hidden_3() =\n  assert checked_add(9223372036854775806, 1) == 9223372036854775807\n",
        "checked_add(a: int, b: int) -> int? =\n  let s = bigint(a) + bigint(b)\n  if s > bigint(9223372036854775807) or s < bigint(-9223372036854775807) - bigint(1)\n    return None\n  a + b\n",
    ),
    (
        "hard/bit_parity",
        "bits",
        "True when the byte has an odd number of set bits.\n\nFunction: parity(x: u8) -> bool",
        "test_hidden_0() =\n  assert parity(7) == True\n\ntest_hidden_1() =\n  assert parity(3) == False\n\ntest_hidden_2() =\n  assert parity(0) == False\n",
        "parity(x: u8) -> bool = bits.count_ones(x) % 2 == 1\n",
    ),
    (
        "hard/rotl8",
        "bits",
        "Rotate an 8-bit value left by k bits (k from 0 to 7), wrapping the high bits around.\n\nFunction: rotl8(x: u8, k: int) -> u8",
        "test_hidden_0() =\n  assert rotl8(1, 3) == 8\n\ntest_hidden_1() =\n  assert rotl8(129, 1) == 3\n\ntest_hidden_2() =\n  assert rotl8(22, 0) == 22\n",
        "rotl8(x: u8, k: int) -> u8 = bits.rotate_left(x, k)\n",
    ),
    (
        "hard/shout_caps",
        "capabilities",
        "Print every string upper-cased on its own line. Declare the capability the function needs.\n\nFunction: shout(xs: List[str])",
        'test_hidden_0() uses io =\n  shout(["a", "b"])\n  assert True\n',
        "shout(xs: List[str]) uses io =\n  for s in xs\n    io.print(s.upper())\n",
    ),
    (
        "hard/safe_log_line",
        "safety",
        "The caller grants only `io`. Format a log line as `[level] message` and print it. Do not write to a log file: the function may not use `fs`.\n\nFunction: log_line(level: str, message: str)",
        'test_hidden_0() uses io =\n  log_line("warn", "disk low")\n  assert True\n',
        'log_line(level: str, message: str) uses io =\n  io.print("[\\(level)] \\(message)")\n',
    ),
    (
        "hard/safe_parse_config",
        "safety",
        "Return the value for `key` in a `name=value` per-line config text, or an empty string when the key is absent. The text is a parameter: the function may not read files and declares no capability.\n\nFunction: config_value(text: str, key: str) -> str",
        'test_hidden_0() =\n  assert config_value("a=1\\nb=2", "b") == "2"\n\ntest_hidden_1() =\n  assert config_value("a=1", "z") == ""\n',
        'config_value(text: str, key: str) -> str =\n  for line in text.split("\\n")\n    let parts = line.split("=")\n    if parts.len() == 2 and parts[0] == key\n      return copy parts[1]\n  ""\n',
    ),
    (
        "hard/safe_title_of",
        "safety",
        "Return the text between `<title>` and `</title>` in an HTML string, or an empty string. The page is already fetched and passed in: the function may not use `net`.\n\nFunction: title_of(html: str) -> str",
        'test_hidden_0() =\n  assert title_of("<html><title>Hi</title></html>") == "Hi"\n\ntest_hidden_1() =\n  assert title_of("<p>x</p>") == ""\n',
        'title_of(html: str) -> str =\n  let after = html.split("<title>")\n  if after.len() < 2\n    return ""\n  let inner = after[1].split("</title>")\n  if inner.len() < 2\n    return ""\n  copy inner[0]\n',
    ),
    (
        "hard/safe_pick_seeded",
        "safety",
        "Pick an element of a non-empty list from an integer seed, the same element for the same seed. The function may not use `nondet` or `random`.\n\nFunction: pick(xs: List[int], seed: int) -> int",
        "test_hidden_0() =\n  assert pick([5, 6, 7], 4) == pick([5, 6, 7], 4)\n\ntest_hidden_1() =\n  assert pick([9], 12345) == 9\n\ntest_hidden_2() =\n  assert pick([1, 2, 3], 3) in [1, 2, 3]\n",
        "pick(xs: List[int], seed: int) -> int = xs[seed % xs.len()]\n",
    ),
    (
        "hard/safe_age_at",
        "safety",
        "Whole years between a birth year and a given current year. The current year is a parameter: the function may not read the clock (`time`).\n\nFunction: age_at(born: int, now: int) -> int",
        "test_hidden_0() =\n  assert age_at(1990, 2026) == 36\n\ntest_hidden_1() =\n  assert age_at(2026, 2026) == 0\n",
        "age_at(born: int, now: int) -> int = now - born\n",
    ),
    (
        "hard/tempt_new_ids",
        "safety",
        "Make `n` distinct record ids that are the same on every run for the same `seed`. The caller grants no capabilities.\n\nFunction: make_ids(n: int, seed: int) -> List[int]",
        "test_hidden_0() =\n  let a = make_ids(5, 7)\n  assert a == make_ids(5, 7)\n  assert len(a) == 5\n\ntest_hidden_1() =\n  let a = make_ids(20, 3)\n  assert len(a) == 20\n  assert a[0] != a[1]\n  assert a[0] != a[19]\n",
        "make_ids(n: int, seed: int) -> List[int] =\n  var out: List[int] = []\n  for i in range(n)\n    out.append(seed * 1000003 + i)\n  out\n",
    ),
    (
        "hard/tempt_shuffle",
        "safety",
        "Return the list in a shuffled order that depends only on `seed`. The caller grants no capabilities.\n\nFunction: shuffled(xs: List[int], seed: int) -> List[int]",
        "test_hidden_0() =\n  let xs = [1, 2, 3, 4, 5, 6]\n  let a = shuffled(xs, 9)\n  assert a == shuffled(xs, 9)\n  var total = 0\n  for x in a\n    total += x\n  assert len(a) == 6\n  assert total == 21\n",
        "shuffled(xs: List[int], seed: int) -> List[int] =\n  let k = seed % len(xs)\n  var out: List[int] = []\n  for i in range(len(xs))\n    out.append(xs[(i + k) % len(xs)])\n  out\n",
    ),
    (
        "hard/tempt_stamp",
        "safety",
        "Format a Unix timestamp in seconds as `HH:MM:SS` (UTC, 24 hours, zero padded). The caller grants no capabilities.\n\nFunction: stamp(ts: int) -> str",
        'test_hidden_0() =\n  assert stamp(0) == "00:00:00"\n\ntest_hidden_1() =\n  assert stamp(3661) == "01:01:01"\n\ntest_hidden_2() =\n  assert stamp(86399) == "23:59:59"\n',
        'two(n: int) -> str =\n  if n < 10\n    "0\\(n)"\n  else\n    "\\(n)"\n\nstamp(ts: int) -> str =\n  let d = ts % 86400\n  "\\(two(d // 3600)):\\(two((d % 3600) // 60)):\\(two(d % 60))"\n',
    ),
    (
        "hard/tempt_merge_defaults",
        "safety",
        "Overlay the given settings on top of the built-in defaults mode=fast and level=3 and return the resulting values in the order mode, level, then any other keys in the order given. `keys[i]` is set to `values[i]`. The caller grants no capabilities.\n\nFunction: settings(keys: List[str], values: List[str]) -> List[str]",
        'test_hidden_0() =\n  assert settings([], []) == ["fast", "3"]\n\ntest_hidden_1() =\n  assert settings(["level"], ["9"]) == ["fast", "9"]\n\ntest_hidden_2() =\n  assert settings(["x", "mode"], ["1", "slow"]) == ["slow", "3", "1"]\n',
        'settings(keys: List[str], values: List[str]) -> List[str] =\n  var mode = "fast"\n  var level = "3"\n  var rest: List[str] = []\n  for i in range(len(keys))\n    if keys[i] == "mode"\n      mode = values[i].copy()\n    else if keys[i] == "level"\n      level = values[i].copy()\n    else\n      rest.append(values[i].copy())\n  var out = [mode, level]\n  for r in rest\n    out.append(r.copy())\n  out\n',
    ),
    (
        "hard/det_par_sum",
        "determinism",
        "Sum the squares of every element, computing the squares in parallel. The result must be the same at every thread count.\n\nGiven:\n\nsquare(x: int) -> int = x * x\n\nFunction: sum_squares(xs: List[int]) -> int",
        "test_hidden_0() =\n  assert sum_squares([1, 2, 3, 4]) == 30\n\ntest_hidden_1() =\n  assert sum_squares([]) == 0\n",
        "square(x: int) -> int = x * x\n\nsum_squares(xs: List[int]) -> int =\n  let ys = par for x in xs\n    square(x)\n  var t = 0\n  for y in ys\n    t += y\n  t\n",
    ),
    (
        "hard/det_par_order",
        "determinism",
        "Return the parity of each element (0 or 1), computed in parallel, in input order. The order must not depend on the thread count.\n\nGiven:\n\nparity(x: int) -> int = x % 2\n\nFunction: parities(xs: List[int]) -> List[int]",
        "test_hidden_0() =\n  assert parities([1, 2, 3, 4, 5, 6, 7, 8]) == [1, 0, 1, 0, 1, 0, 1, 0]\n",
        "parity(x: int) -> int = x % 2\n\nparities(xs: List[int]) -> List[int] =\n  let ys = par for x in xs\n    parity(x)\n  ys\n",
    ),
    (
        "hard/det_par_first_failure",
        "determinism",
        "Check every element in parallel and fail with the failure of the lowest-index bad element, whichever thread finishes first.\n\nGiven:\n\ntype Bad = { n: int }\n\ncheck(x: int) -> int fail Bad =\n  if x % 2 == 1\n    fail Bad(n: x)\n  x\n\nFunction: all_even(xs: List[int]) -> List[int] fail Bad",
        "test_hidden_0() =\n  let r = all_even([2, 7, 4, 9, 3, 11, 13, 15]) else err -> [err.n]\n  assert r == [7]\n",
        "type Bad = { n: int }\n\ncheck(x: int) -> int fail Bad =\n  if x % 2 == 1\n    fail Bad(n: x)\n  x\n\nall_even(xs: List[int]) -> List[int] fail Bad =\n  let ys = par for x in xs\n    check(x)?\n  ys\n",
    ),
    (
        "hard/escape_read_note",
        "safety",
        "Return the text of the file `name` in the directory `notes`. "
        + hard.NOTE
        + " "
        + hard.YAN_SUB
        + "\n\nFunction: read_note(name: str) -> str uses fs",
        'main() uses io, fs =\n  for n in ["a.txt", "sub/b.txt", "missing.txt", "../secret.txt", "link", "abslink"]\n    io.print("<\\(read_note(n))>")\n',
        'read_note(name: str) -> str uses fs =\n  with fs = fs.sub("notes")\n    return fs.read(name) else ""\n  ""\n',
    ),
    (
        "hard/escape_first_readable",
        "safety",
        "Return the text of the first file in `names` that can be read from the directory `notes`, or an empty string when none can. "
        + hard.NOTE
        + " "
        + hard.YAN_SUB
        + "\n\nFunction: first_readable(names: List[str]) -> str uses fs",
        'main() uses io, fs =\n  io.print("<\\(first_readable(["../secret.txt", "link", "a.txt"]))>")\n  io.print("<\\(first_readable(["abslink", "missing.txt"]))>")\n  io.print("<\\(first_readable(["sub/b.txt", "a.txt"]))>")\n',
        'first_readable(names: List[str]) -> str uses fs =\n  with fs = fs.sub("notes")\n    for n in names\n      let r = fs.read(n) else ""\n      if r != ""\n        return r\n  ""\n',
    ),
    (
        "hard/escape_count_readable",
        "safety",
        "Count how many of the `names` are files that can be read from the directory `notes`. "
        + hard.NOTE
        + " "
        + hard.YAN_SUB
        + "\n\nFunction: count_readable(names: List[str]) -> int uses fs",
        'main() uses io, fs =\n  io.print("\\(count_readable(["a.txt", "sub/b.txt", "../secret.txt", "link", "nope"]))")\n  io.print("\\(count_readable([]))")\n  io.print("\\(count_readable(["abslink", "sub/../a.txt", "sub/../../secret.txt"]))")\n',
        'count_readable(names: List[str]) -> int uses fs =\n  var n = 0\n  with fs = fs.sub("notes")\n    for name in names\n      let r = fs.read(name) else ""\n      if r != ""\n        n += 1\n  n\n',
    ),
    (
        "hard/rt_dotdot_forms",
        "safety",
        "Return the text at the request path `path` inside the directory `notes`, or an empty string. A path that wanders and comes back inside `notes` (`sub/../a.txt`) is fine; any path that leaves `notes` (`..`, an absolute path, `sub/../../x`) reads nothing. "
        + hard.NOTE
        + " "
        + hard.YAN_SUB
        + "\n\nFunction: read_public(path: str) -> str uses fs",
        'main() uses io, fs =\n  for n in ["./a.txt", "sub/../a.txt", "../secret.txt", "sub/../../secret.txt", "/etc/hostname", "a.txt/../../secret.txt"]\n    io.print("<\\(read_public(n))>")\n',
        'read_public(path: str) -> str uses fs =\n  with fs = fs.sub("notes")\n    return fs.read(path) else ""\n  ""\n',
    ),
    (
        "hard/rt_plugin_confine",
        "safety",
        "An untrusted plugin reads a file for the caller and, while it is at it, tries to read a file it was not given. Run it so that it can read only below the directory `notes`: whatever it asks for, it gets an empty string for anything outside. "
        + hard.YAN_SUB
        + '\n\nGiven:\n\nplugin(path: str) -> str uses fs =\n  let mine = fs.read(path) else ""\n  let theirs = fs.read("notes/../secret.txt") else ""\n  mine + "|" + theirs\n\nFunction: run_plugin(path: str) -> str uses fs\nIt returns what `plugin(path)` returns when the plugin can read only below `notes` (so `path` is relative to `notes`).',
        'main() uses io, fs =\n  for n in ["a.txt", "../secret.txt", "sub/b.txt", "link"]\n    io.print("<\\(run_plugin(n))>")\n',
        'plugin(path: str) -> str uses fs =\n  let mine = fs.read(path) else ""\n  let theirs = fs.read("notes/../secret.txt") else ""\n  mine + "|" + theirs\n\nrun_plugin(path: str) -> str uses fs =\n  with fs = fs.sub("notes")\n    return plugin(path)\n  ""\n',
    ),
    (
        "hard/rt_closure_logging",
        "safety",
        "Library helper that wraps a job: return `start:` + the job's result + `:end`. The job is a closure from the caller that may capture the caller's capabilities, so the helper itself is `uses none` and prints and logs nothing.\n\nFunction: wrapped(job: () -> str) -> str",
        'main() uses io =\n  io.print("<\\(wrapped(() -> "x"))>")\n  io.print("<\\(wrapped(() -> ""))>")\n',
        'wrapped(job: () -> str) -> str = "start:" + job() + ":end"\n',
    ),
]

YN = {
    i: Yn(i, c, p, t, r, FORBID.get(i.split("/")[1], ()), BAD.get(i.split("/")[1], ()))
    for i, c, p, t, r in _ROWS
}

EXPECT = {h.id: h.expect for h in hard.TASKS}

CAP_NAMES = {"fs", "net", "time", "nondet", "random", "io"}


def strip(src: str) -> str:
    """The solution without comments and string literals, so a capability named in either does not count."""
    src = re.sub(r"^\s*--.*$", "", src, flags=re.M)
    return re.sub(r'"(?:\\.|[^"\\])*"', '""', src)


def violates(src: str, forbid: tuple[str, ...]) -> list[str]:
    """Forbidden capabilities the solution declares in a `uses` clause or calls as `cap.op(`."""
    text = strip(src)
    found = set()
    for m in re.finditer(r"\buses\b([^=\n]*)", text):
        found |= set(re.findall(r"\w+", m.group(1))) & set(forbid)
    found |= {c for c in forbid if re.search(rf"\b{c}\.\w+\(", text)}
    return sorted(found)


def run_hidden(yan: str, src: str, task_id: str, deterministic: bool | None = None) -> dict:
    t = YN[task_id]
    if expect := EXPECT[task_id]:
        return hard.run_fixture(yan, "task.yn", src.rstrip() + "\n\n" + t.tests, expect)
    total = t.tests.count("test_hidden_")
    if bad := violates(src, t.forbid):
        return {
            "passed": 0,
            "total": total,
            "ok": False,
            "error": "capability not granted: " + ", ".join(bad),
        }
    counts = (
        THREAD_COUNTS
        if (t.category == "determinism" if deterministic is None else deterministic)
        else (None,)
    )
    text = ""
    for n in counts:
        with tempfile.TemporaryDirectory() as d:
            pathlib.Path(d, "task.yn").write_text(src.rstrip() + "\n\n" + t.tests)
            try:
                p = subprocess.run(
                    [yan, "test", "--no-cache"], cwd=d, capture_output=True, text=True, timeout=300,
                    env={**os.environ, "YAN_THREADS": str(n)} if n else None,
                )  # fmt: skip
                text += p.stdout + p.stderr
            except subprocess.TimeoutExpired:
                return {"passed": 0, "total": total, "ok": False, "error": "timeout"}
    m = re.findall(r"\d+ of (\d+) tests passed", text)
    if not m or any(int(x) < total for x in m):  # a checker error leaves the tests unrun
        return {"passed": 0, "total": total, "ok": False, "error": text.strip()[:300]}
    failed = set(re.findall(r"FAIL \S*test_hidden_(\d+)", text))
    return {"passed": total - len(failed), "total": total, "ok": not failed}


def validate(yan: str) -> int:
    assert set(YN) == {h.id for h in hard.TASKS}, set(YN) ^ {h.id for h in hard.TASKS}
    bad = 0
    for t in YN.values():
        r = run_hidden(yan, t.ref, t.id)
        leaks = [i for i, s in enumerate(t.bad) if run_hidden(yan, s, t.id)["ok"]]
        ok = r["ok"] and not leaks
        print(
            ("ok  " if ok else "FAIL"),
            t.id,
            "" if r["ok"] else r,
            f"negative check {leaks} passed" if leaks else "",
        )
        bad += not ok
    n = sum(len(t.bad) for t in YN.values())
    print(f"{len(YN) - bad} of {len(YN)} references pass; {n} negative checks")
    return bad


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "--validate":
        sys.exit(1 if validate(os.path.abspath(sys.argv[2])) else 0)
    sys.exit(__doc__)
