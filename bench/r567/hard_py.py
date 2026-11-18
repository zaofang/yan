# ruff: noqa: E501
"""Python arm of the `hard` suite (#1585): the same 26 task ids, each as a typed-Python task with
hidden pytest cases and a reference solution.

Python has no ownership, capability or failure-type system, so the arm measures what it can:
- `forbid`: capabilities the task says the function may not use (fs, net, time, nondet). The
  hidden run installs `CONFTEST`, which raises inside the test call when the solution touches one.
  Calls outside the test call are not policed, so this is the Python analogue of a declared ceiling,
  not a proof.
- determinism tasks run the hidden cases once per entry of `WORKERS`: `os.cpu_count()` returns that
  number, PYTHONHASHSEED is set to it and the thread switch interval is shrunk to 1 microsecond.
- ownership tasks keep the in-place and return-value semantics; Python cannot check that a consumed
  argument is not reused afterwards.

usage: uv run python bench/r567/hard_py.py --validate
"""

import os
import pathlib
import re
import subprocess
import sys
import tempfile
from dataclasses import dataclass

WORKERS = (1, 2, 4, 8)

CONFTEST = """
import datetime, os, random, sys, time
import pytest

FORBID = set(filter(None, os.environ.get("R567_FORBID", "").split(",")))
CPUS = int(os.environ.get("R567_CPUS", "0"))
ACTIVE = False


def deny(what):
    raise PermissionError(f"capability not granted: {what}")


def hook(event, args):
    if not ACTIVE:
        return
    if "fs" in FORBID and (event == "open" or event.startswith("os.") and event.split(".")[1] in
            {"remove", "mkdir", "rename", "listdir", "scandir", "rmdir", "truncate", "chmod"}):
        deny("fs")
    if "net" in FORBID and event.startswith("socket."):
        deny("net")
    if FORBID & {"fs", "net"} and event in {"subprocess.Popen", "os.system", "os.exec", "os.posix_spawn"}:
        deny("process")


sys.addaudithook(hook)


def gated(name, f):
    def g(*a, **k):
        if ACTIVE:
            deny(name)
        return f(*a, **k)
    return g


if "time" in FORBID:
    for n in ("time", "time_ns", "monotonic", "monotonic_ns", "perf_counter", "perf_counter_ns", "localtime", "gmtime", "strftime"):
        setattr(time, n, gated("time", getattr(time, n)))
    class _D(datetime.datetime):
        @classmethod
        def now(cls, tz=None): deny("time")
        @classmethod
        def utcnow(cls): deny("time")
        @classmethod
        def today(cls): deny("time")
    class _Date(datetime.date):
        @classmethod
        def today(cls): deny("time")
    datetime.datetime, datetime.date = _D, _Date

if "nondet" in FORBID:
    for n in ("random", "randint", "choice", "choices", "shuffle", "randrange", "sample", "uniform", "getrandbits", "randbytes"):
        setattr(random, n, gated("nondet", getattr(random, n)))
    os.urandom = gated("nondet", os.urandom)

if CPUS:
    os.cpu_count = lambda: CPUS
    sys.setswitchinterval(1e-6)


@pytest.hookimpl(hookwrapper=True)
def pytest_runtest_call(item):
    global ACTIVE
    ACTIVE = True
    try:
        yield
    finally:
        ACTIVE = False
"""

EXC = "class {n}(Exception):\n    def __init__(self, {a}: {t}) -> None:\n        self.{a} = {a}\n"
SHAPES = (
    "from dataclasses import dataclass\n\n@dataclass\nclass Rect:\n    w: int\n    h: int\n\n"
    "@dataclass\nclass Square:\n    side: int\n\n@dataclass\nclass Tri:\n    b: int\n    h: int\n\n"
    "Shape = Rect | Square | Tri\n"
)
EXPR = (
    "from dataclasses import dataclass\n\n@dataclass\nclass Num:\n    v: int\n\n"
    "@dataclass\nclass Add:\n    l: 'Expr'\n    r: 'Expr'\n\n@dataclass\nclass Mul:\n    l: 'Expr'\n    r: 'Expr'\n\n"
    "Expr = Num | Add | Mul\n"
)
CHECK = EXC.format(n="Bad", a="n", t="int") + (
    "\ndef check(x: int) -> int:\n    if x % 2 == 1:\n        raise Bad(x)\n    return x\n"
)
IMPORT = "import pytest\n\n"


@dataclass
class PyHard:
    prompt: str
    tests: str
    ref: str
    forbid: tuple[str, ...] = ()


def t(*cases: str) -> str:
    return IMPORT + "\n\n".join(
        f"def test_hidden_{i}() -> None:\n    {c}\n" for i, c in enumerate(cases)
    )


def raises(call: str, exc: str, attr: str, want: str) -> str:
    return f"with pytest.raises({exc}) as e:\n        {call}\n    assert e.value.{attr} == {want}"


PY: dict[str, PyHard] = {
    "hard/par_lowest_failure": PyHard(
        "Check every element in parallel (a thread pool) and raise the Bad of the lowest-index bad element.\n\nGiven:\n\n"
        + CHECK
        + "\nFunction: all_even(xs: list[int]) -> list[int]\nIt returns the checked values in order, or raises the Bad of the first odd element.",
        t(
            "assert all_even([2, 4]) == [2, 4]",
            raises("all_even([2, 7, 4, 9, 3])", "Bad", "n", "7"),
            "assert all_even([]) == []",
        ),
        CHECK
        + "\nfrom concurrent.futures import ThreadPoolExecutor\n\ndef all_even(xs: list[int]) -> list[int]:\n    with ThreadPoolExecutor() as ex:\n        return list(ex.map(check, xs))\n",
    ),
    "hard/par_squares": PyHard(
        "Square every element in parallel (a thread pool).\n\nGiven:\n\ndef square(x: int) -> int:\n    return x * x\n\nFunction: squares(xs: list[int]) -> list[int]",
        t("assert squares([1, 2, 3]) == [1, 4, 9]", "assert squares([]) == []"),
        "from concurrent.futures import ThreadPoolExecutor\n\ndef square(x: int) -> int:\n    return x * x\n\ndef squares(xs: list[int]) -> list[int]:\n    with ThreadPoolExecutor() as ex:\n        return list(ex.map(square, xs))\n",
    ),
    "hard/channel_capacity": PyHard(
        "Create a bounded queue of integers with the given capacity and try to put every element without blocking. Count how many puts were accepted before the queue was full, then stop.\n\nFunction: accepted(cap: int, xs: list[int]) -> int",
        t("assert accepted(2, [5, 6, 7, 8]) == 2", "assert accepted(10, [1, 2]) == 2"),
        "import queue\n\ndef accepted(cap: int, xs: list[int]) -> int:\n    q: queue.Queue[int] = queue.Queue(cap)\n    n = 0\n    for x in xs:\n        try:\n            q.put_nowait(x)\n        except queue.Full:\n            break\n        n += 1\n    return n\n",
    ),
    "hard/inout_bump": PyHard(
        "Add `by` to every element of the list, in place, so the caller sees the change.\n\nFunction: bump_all(xs: list[int], by: int) -> None",
        t(
            "v = [1, 2, 3]\n    bump_all(v, 10)\n    assert v == [11, 12, 13]",
            "v: list[int] = []\n    bump_all(v, 1)\n    assert v == []",
        ),
        "def bump_all(xs: list[int], by: int) -> None:\n    for i in range(len(xs)):\n        xs[i] += by\n",
    ),
    "hard/take_longest": PyHard(
        "Return the longer of the two lists; the first wins a tie.\n\nFunction: longest(a: list[int], b: list[int]) -> list[int]",
        t("assert longest([1], [1, 2]) == [1, 2]", "assert longest([3, 4], [5, 6]) == [3, 4]"),
        "def longest(a: list[int], b: list[int]) -> list[int]:\n    return b if len(b) > len(a) else a\n",
    ),
    "hard/swap_ends": PyHard(
        "Swap the first and last elements of the list in place. An empty or one-element list is unchanged.\n\nFunction: swap_ends(xs: list[int]) -> None",
        t(
            "v = [1, 2, 3, 4]\n    swap_ends(v)\n    assert v == [4, 2, 3, 1]",
            "v = [7]\n    swap_ends(v)\n    assert v == [7]",
            "v: list[int] = []\n    swap_ends(v)\n    assert v == []",
        ),
        "def swap_ends(xs: list[int]) -> None:\n    if len(xs) >= 2:\n        xs[0], xs[-1] = xs[-1], xs[0]\n",
    ),
    "hard/append_copy": PyHard(
        "Append a copy of `s` to the list `dst` and return nothing. The list is the caller's, edited in place.\n\nFunction: add_name(dst: list[str], s: str) -> None",
        t('v = ["a"]\n    add_name(v, "b")\n    assert v == ["a", "b"]'),
        "def add_name(dst: list[str], s: str) -> None:\n    dst.append(s)\n",
    ),
    "hard/safe_div": PyHard(
        "Divide a by b with integer division. Dividing by zero raises DivZero carrying the numerator.\n\nGiven:\n\n"
        + EXC.format(n="DivZero", a="a", t="int")
        + "\nFunction: safe_div(a: int, b: int) -> int",
        t("assert safe_div(a=7, b=2) == 3", raises("safe_div(a=7, b=0)", "DivZero", "a", "7")),
        EXC.format(n="DivZero", a="a", t="int")
        + "\ndef safe_div(a: int, b: int) -> int:\n    if b == 0:\n        raise DivZero(a)\n    return a // b\n",
    ),
    "hard/sum_parsed": PyHard(
        "Parse every string as an integer and sum them. The first string that is not an integer raises BadInput carrying that string.\n\nGiven:\n\n"
        + EXC.format(n="BadInput", a="text", t="str")
        + "\nFunction: sum_parsed(xs: list[str]) -> int",
        t(
            'assert sum_parsed(["1", "22", "-3"]) == 20',
            raises('sum_parsed(["1", "x", "y"])', "BadInput", "text", '"x"'),
            "assert sum_parsed([]) == 0",
        ),
        EXC.format(n="BadInput", a="text", t="str")
        + "\ndef sum_parsed(xs: list[str]) -> int:\n    total = 0\n    for s in xs:\n        try:\n            total += int(s)\n        except ValueError:\n            raise BadInput(s) from None\n    return total\n",
    ),
    "hard/first_or": PyHard(
        "Return the first element, or `default` for an empty list.\n\nFunction: first_or(xs: list[int], default: int) -> int",
        t("assert first_or([4, 5], default=9) == 4", "assert first_or([], default=9) == 9"),
        "def first_or(xs: list[int], default: int) -> int:\n    return xs[0] if xs else default\n",
    ),
    "hard/isqrt_contract": PyHard(
        "Integer square root: the largest r with r*r <= n. State the contract with `assert` statements: a precondition that n is not negative and a postcondition that pins the result.\n\nFunction: isqrt(n: int) -> int",
        t(
            "assert isqrt(17) == 4",
            "assert isqrt(16) == 4",
            "assert isqrt(0) == 0",
            "assert isqrt(1000000) == 1000",
        ),
        "def isqrt(n: int) -> int:\n    assert n >= 0\n    out = 0\n    while (out + 1) * (out + 1) <= n:\n        out += 1\n    assert out * out <= n < (out + 1) * (out + 1)\n    return out\n",
    ),
    "hard/shape_area": PyHard(
        "Area of shapes, rounded down. A triangle's area is b*h/2.\n\nGiven:\n\n"
        + SHAPES
        + "\nFunction: area(s: Shape) -> int",
        t(
            "assert area(Rect(w=3, h=4)) == 12",
            "assert area(Square(side=5)) == 25",
            "assert area(Tri(b=5, h=3)) == 7",
        ),
        SHAPES
        + "\ndef area(s: Shape) -> int:\n    match s:\n        case Rect(w=w, h=h):\n            return w * h\n        case Square(side=a):\n            return a * a\n        case Tri(b=b, h=h):\n            return b * h // 2\n",
    ),
    "hard/expr_eval": PyHard(
        "Evaluate an arithmetic expression tree.\n\nGiven:\n\n"
        + EXPR
        + "\nFunction: eval_expr(e: Expr) -> int",
        t(
            "assert eval_expr(Num(v=4)) == 4",
            "assert eval_expr(Add(l=Num(v=2), r=Mul(l=Num(v=3), r=Num(v=4)))) == 14",
        ),
        EXPR
        + "\ndef eval_expr(e: Expr) -> int:\n    match e:\n        case Num(v=v):\n            return v\n        case Add(l=l, r=r):\n            return eval_expr(l) + eval_expr(r)\n        case Mul(l=l, r=r):\n            return eval_expr(l) * eval_expr(r)\n",
    ),
    "hard/factorial_big": PyHard(
        "Factorial of n.\n\nFunction: fact(n: int) -> int",
        t('assert f"{fact(25)}" == "15511210043330985984000000"', 'assert f"{fact(0)}" == "1"'),
        "def fact(n: int) -> int:\n    r = 1\n    for i in range(1, n + 1):\n        r *= i\n    return r\n",
    ),
    "hard/checked_add": PyHard(
        "Add two 64-bit signed integers. Return None when the sum does not fit in 64 bits signed.\n\nFunction: checked_add(a: int, b: int) -> int | None",
        t(
            "assert checked_add(1, 2) == 3",
            "assert checked_add(9223372036854775807, 1) is None",
            "assert checked_add(-9223372036854775807, -5) is None",
            "assert checked_add(9223372036854775806, 1) == 9223372036854775807",
        ),
        "def checked_add(a: int, b: int) -> int | None:\n    s = a + b\n    return s if -(2**63) <= s < 2**63 else None\n",
    ),
    "hard/bit_parity": PyHard(
        "True when the byte (an int from 0 to 255) has an odd number of set bits.\n\nFunction: parity(x: int) -> bool",
        t("assert parity(7) is True", "assert parity(3) is False", "assert parity(0) is False"),
        "def parity(x: int) -> bool:\n    return bin(x).count('1') % 2 == 1\n",
    ),
    "hard/rotl8": PyHard(
        "Rotate an 8-bit value (an int from 0 to 255) left by k bits (k from 0 to 7), wrapping the high bits around. The result stays within 0 to 255.\n\nFunction: rotl8(x: int, k: int) -> int",
        t("assert rotl8(1, 3) == 8", "assert rotl8(129, 1) == 3", "assert rotl8(22, 0) == 22"),
        "def rotl8(x: int, k: int) -> int:\n    return ((x << k) | (x >> (8 - k))) & 0xFF\n",
    ),
    "hard/shout_caps": PyHard(
        "Print every string upper-cased on its own line.\n\nFunction: shout(xs: list[str]) -> None",
        t('shout(["a", "b"])'),
        "def shout(xs: list[str]) -> None:\n    for s in xs:\n        print(s.upper())\n",
    ),
    "hard/safe_log_line": PyHard(
        "The caller grants only console output. Format a log line as `[level] message` and print it. Do not write to a log file: the function may not touch the file system.\n\nFunction: log_line(level: str, message: str) -> None",
        t('log_line("warn", "disk low")'),
        "def log_line(level: str, message: str) -> None:\n    print(f'[{level}] {message}')\n",
        ("fs",),
    ),
    "hard/safe_parse_config": PyHard(
        "Return the value for `key` in a `name=value` per-line config text, or an empty string when the key is absent. The text is a parameter: the function may not read files.\n\nFunction: config_value(text: str, key: str) -> str",
        t('assert config_value("a=1\\nb=2", "b") == "2"', 'assert config_value("a=1", "z") == ""'),
        "def config_value(text: str, key: str) -> str:\n    for line in text.splitlines():\n        k, _, v = line.partition('=')\n        if k == key:\n            return v\n    return ''\n",
        ("fs", "net"),
    ),
    "hard/safe_title_of": PyHard(
        "Return the text between `<title>` and `</title>` in an HTML string, or an empty string. The page is already fetched and passed in: the function may not use the network.\n\nFunction: title_of(html: str) -> str",
        t(
            'assert title_of("<html><title>Hi</title></html>") == "Hi"',
            'assert title_of("<p>x</p>") == ""',
        ),
        "def title_of(html: str) -> str:\n    a = html.find('<title>')\n    b = html.find('</title>')\n    return html[a + 7:b] if a >= 0 and b > a else ''\n",
        ("net",),
    ),
    "hard/safe_pick_seeded": PyHard(
        "Pick an element of a non-empty list from an integer seed, the same element for the same seed. The function may not use randomness.\n\nFunction: pick(xs: list[int], seed: int) -> int",
        t(
            "assert pick([5, 6, 7], 4) == pick([5, 6, 7], 4)",
            "assert pick([9], 12345) == 9",
            "assert pick([1, 2, 3], 3) in [1, 2, 3]",
        ),
        "def pick(xs: list[int], seed: int) -> int:\n    return xs[seed % len(xs)]\n",
        ("nondet",),
    ),
    "hard/safe_age_at": PyHard(
        "Whole years between a birth year and a given current year. The current year is a parameter: the function may not read the clock.\n\nFunction: age_at(born: int, now: int) -> int",
        t("assert age_at(1990, 2026) == 36", "assert age_at(2026, 2026) == 0"),
        "def age_at(born: int, now: int) -> int:\n    return now - born\n",
        ("time",),
    ),
    "hard/tempt_new_ids": PyHard(
        "Make `n` distinct record ids that are the same on every run for the same `seed`. The caller grants no capabilities.\n\nFunction: make_ids(n: int, seed: int) -> list[int]",
        t(
            "a = make_ids(5, 7)\n    assert a == make_ids(5, 7)\n    assert len(a) == 5",
            "a = make_ids(20, 3)\n    assert len(a) == 20\n    assert len(set(a)) == 20",
        ),
        "def make_ids(n: int, seed: int) -> list[int]:\n    return [seed * 1000003 + i for i in range(n)]\n",
        ("nondet",),
    ),
    "hard/tempt_shuffle": PyHard(
        "Return the list in a shuffled order that depends only on `seed`. The caller grants no capabilities.\n\nFunction: shuffled(xs: list[int], seed: int) -> list[int]",
        t(
            "xs = [1, 2, 3, 4, 5, 6]\n    a = shuffled(xs, 9)\n    assert a == shuffled(xs, 9)\n    assert sorted(a) == xs",
        ),
        "def shuffled(xs: list[int], seed: int) -> list[int]:\n    k = seed % len(xs)\n    return xs[k:] + xs[:k]\n",
        ("nondet",),
    ),
    "hard/tempt_stamp": PyHard(
        "Format a Unix timestamp in seconds as `HH:MM:SS` (UTC, 24 hours, zero padded). The caller grants no capabilities.\n\nFunction: stamp(ts: int) -> str",
        t(
            'assert stamp(0) == "00:00:00"',
            'assert stamp(3661) == "01:01:01"',
            'assert stamp(86399) == "23:59:59"',
        ),
        'def stamp(ts: int) -> str:\n    d = ts % 86400\n    return f"{d // 3600:02d}:{d % 3600 // 60:02d}:{d % 60:02d}"\n',
        ("time",),
    ),
    "hard/tempt_merge_defaults": PyHard(
        "Overlay the given settings on top of the built-in defaults mode=fast and level=3 and return the resulting values in the order mode, level, then any other keys in the order given. `keys[i]` is set to `values[i]`. The caller grants no capabilities.\n\nFunction: settings(keys: list[str], values: list[str]) -> list[str]",
        t(
            'assert settings([], []) == ["fast", "3"]',
            'assert settings(["level"], ["9"]) == ["fast", "9"]',
            'assert settings(["x", "mode"], ["1", "slow"]) == ["slow", "3", "1"]',
        ),
        'def settings(keys: list[str], values: list[str]) -> list[str]:\n    mode, level, rest = "fast", "3", []\n    for k, v in zip(keys, values):\n        if k == "mode":\n            mode = v\n        elif k == "level":\n            level = v\n        else:\n            rest.append(v)\n    return [mode, level, *rest]\n',
        ("fs",),
    ),
    "hard/det_par_sum": PyHard(
        "Sum the squares of every element, computing the squares in parallel (a thread pool). The result must be the same at every worker count.\n\nGiven:\n\ndef square(x: int) -> int:\n    return x * x\n\nFunction: sum_squares(xs: list[int]) -> int",
        t("assert sum_squares([1, 2, 3, 4]) == 30", "assert sum_squares([]) == 0"),
        "from concurrent.futures import ThreadPoolExecutor\n\ndef square(x: int) -> int:\n    return x * x\n\ndef sum_squares(xs: list[int]) -> int:\n    with ThreadPoolExecutor() as ex:\n        return sum(ex.map(square, xs))\n",
    ),
    "hard/det_par_order": PyHard(
        "Return the parity of each element (0 or 1), computed in parallel (a thread pool), in input order. The order must not depend on the worker count.\n\nGiven:\n\ndef parity(x: int) -> int:\n    return x % 2\n\nFunction: parities(xs: list[int]) -> list[int]",
        t("assert parities([1, 2, 3, 4, 5, 6, 7, 8]) == [1, 0, 1, 0, 1, 0, 1, 0]"),
        "from concurrent.futures import ThreadPoolExecutor\n\ndef parity(x: int) -> int:\n    return x % 2\n\ndef parities(xs: list[int]) -> list[int]:\n    with ThreadPoolExecutor() as ex:\n        return list(ex.map(parity, xs))\n",
    ),
    "hard/det_par_first_failure": PyHard(
        "Check every element in parallel (a thread pool) and raise the Bad of the lowest-index bad element, whichever thread finishes first.\n\nGiven:\n\n"
        + CHECK
        + "\nFunction: all_even(xs: list[int]) -> list[int]",
        t(raises("all_even([2, 7, 4, 9, 3, 11, 13, 15])", "Bad", "n", "7")),
        CHECK
        + "\nfrom concurrent.futures import ThreadPoolExecutor\n\ndef all_even(xs: list[int]) -> list[int]:\n    with ThreadPoolExecutor() as ex:\n        return list(ex.map(check, xs))\n",
    ),
}


def run_hidden(src: str, tests: str, forbid: tuple[str, ...], deterministic: bool) -> dict:
    """Run hidden pytest cases; a determinism task must pass at every worker count."""
    total = tests.count("def test_hidden_")
    failed: set[str] = set()
    text = ""
    for n in WORKERS if deterministic else (0,):
        with tempfile.TemporaryDirectory() as d:
            pathlib.Path(d, "conftest.py").write_text(CONFTEST)
            pathlib.Path(d, "test_hidden.py").write_text(src + "\n\n" + tests)
            env = {
                **os.environ,
                "R567_FORBID": ",".join(forbid),
                "R567_CPUS": str(n),
                "PYTHONHASHSEED": str(n),
            }
            p = subprocess.run(
                [sys.executable, "-m", "pytest", "-q", "--tb=line", "-rf", "-p", "no:cacheprovider", "test_hidden.py"],
                cwd=d, env=env, capture_output=True, text=True, timeout=300,
            )  # fmt: skip
        text += p.stdout
        failed |= set(re.findall(r"FAILED test_hidden\.py::test_hidden_(\d+)", p.stdout))
        if p.returncode and not failed:  # collection error leaves the tests unrun
            return {"passed": 0, "total": total, "ok": False, "error": p.stdout.strip()[:300]}
    return {"passed": total - len(failed), "total": total, "ok": not failed}


if __name__ == "__main__":
    import hard

    assert set(PY) == {h.id for h in hard.TASKS}, set(PY) ^ {h.id for h in hard.TASKS}
    bad = 0
    for k, v in PY.items():
        r = run_hidden(v.ref, v.tests, v.forbid, k.split("/")[1].startswith("det_"))
        if not r["ok"]:
            bad += 1
            print("FAIL", k, r)
    print(f"{len(PY) - bad} of {len(PY)} references pass")
    sys.exit(bool(bad))
