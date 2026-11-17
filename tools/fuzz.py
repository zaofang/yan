"""Generated-program differential fuzzing (DL5): a seeded generator of small well-typed Yag
programs, each run through every backend by the tools.difftest runner and compared to the
interpreter. A seed always yields the same program, so a failure reproduces from its number.

    uv run python -m tools.fuzz --seeds 200            # seeds 0..199
    uv run python -m tools.fuzz --start 1000 --seeds 50 --keep out/
    uv run python -m tools.fuzz --show 17              # print the program of seed 17

Programs use ints, strs, lists, records, sums, loops and functions. Arithmetic stays far from
overflow (every value is reduced `% 1000`), so a panic or refusal is a finding, not noise. A
mismatching program is reduced by line deletion before it is reported. Divergences already
found are filed as GitHub issues with the minimized program; there is no allowlist.
"""

import argparse
import random
import sys
import tempfile
from collections.abc import Callable
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from tools import difftest

HEADER = """type Shape = Circle(r: int) | Rect(w: int, h: int) | Dot()

class Pt:
    x: int
    y: int

def area(s: Shape) -> int:
    match s:
        case Circle(r):
            return 3 * r * r
        case Rect(w, h):
            return w * h
        case Dot():
            return 0
"""


class Gen:
    def __init__(self, seed: int) -> None:
        self.r = random.Random(seed)
        self.ints: list[str] = []
        self.mut: list[str] = []
        self.strs: list[str] = []
        self.lists: list[str] = []
        self.pts: list[str] = []
        self.shapes: list[str] = []
        self.fns: list[str] = []
        self.n = 0

    def fresh(self, p: str) -> str:
        self.n += 1
        return f"{p}{self.n}"

    def int_expr(self, d: int = 0) -> str:
        r = self.r
        opts: list[Callable[[], str]] = [lambda: str(r.randint(0, 50))]
        if self.ints:
            opts += [lambda: r.choice(self.ints)] * 3
        if self.pts:
            opts.append(lambda: f"{r.choice(self.pts)}.{r.choice('xy')}")
        if self.strs:
            opts.append(lambda: f"len({r.choice(self.strs)})")
        if self.lists:
            opts.append(lambda: f"len({r.choice(self.lists)})")
            opts.append(lambda: f"sum({r.choice(self.lists)})")
        if self.shapes:
            opts.append(lambda: f"area({r.choice(self.shapes)})")
        if d < 2:
            opts.append(
                lambda: (
                    f"(({self.int_expr(d + 1)} {r.choice('+-*')} {self.int_expr(d + 1)}) % 1000)"
                )
            )
            opts.append(lambda: f"({self.int_expr(d + 1)} // {r.randint(1, 9)})")
            if self.fns:
                opts.append(
                    lambda: f"{r.choice(self.fns)}({self.int_expr(d + 1)}, {self.int_expr(d + 1)})"
                )
        return r.choice(opts)()

    def str_lit(self) -> str:
        return '"' + self.r.choice(["a", "bc", "def", ""]) + '"'

    def str_expr(self) -> str:
        r = self.r
        opts: list[Callable[[], str]] = [self.str_lit]
        if self.strs:
            opts += [lambda: f"{r.choice(self.strs)}.copy()"] * 2
            opts.append(lambda: f"{r.choice(self.strs)}.copy() + {self.str_lit()}")
            opts.append(lambda: f"{r.choice(self.strs)}.copy()[::-1]")
        return r.choice(opts)()

    def shape_expr(self) -> str:
        r = self.r
        opts: list[Callable[[], str]] = [
            lambda: f"Circle(r={self.int_expr(1)})",
            lambda: f"Rect(w={self.int_expr(1)}, h={self.int_expr(1)})",
            lambda: "Dot()",
        ]
        return r.choice(opts)()

    def function(self) -> list[str]:
        name = self.fresh("f")
        saved = self.scope()
        self.ints, self.mut, self.strs, self.lists, self.pts, self.shapes = (
            ["a", "b"],
            ["t"],
            [],
            [],
            [],
            [],
        )
        body = [f"def {name}(a: int, b: int) -> int:", "    t = a"]
        self.ints.append("t")
        for _ in range(self.r.randint(1, 3)):
            body += ["    " + ln for ln in self.stmt(depth=1, in_fn=True)]
        body.append(f"    return (t + {self.int_expr(1)}) % 1000")
        self.ints, self.mut, self.strs, self.lists, self.pts, self.shapes = saved
        self.fns.append(name)
        return body + [""]

    def block(self, depth: int, in_fn: bool) -> list[str]:
        """A nested body: names it binds do not outlive it (E017)."""
        saved = [list(x) for x in self.scope()]
        out = self.stmt(depth, in_fn) or ["pass"]
        for dst, old in zip(self.scope(), saved, strict=True):
            dst[:] = old
        return out

    def scope(self) -> tuple[list[str], ...]:
        return self.ints, self.mut, self.strs, self.lists, self.pts, self.shapes

    def stmt(self, depth: int = 0, in_fn: bool = False) -> list[str]:
        r = self.r
        kinds = ["int", "int", "assign", "loop", "while"]
        if not in_fn:
            kinds += ["str", "list", "append", "pt", "shape", "print", "print"]
        k = r.choice(kinds)
        if k == "int":
            v = self.fresh("n")
            out = [f"{v} = {self.int_expr()}"]
            self.ints.append(v)
            self.mut.append(v)
            return out
        if k == "assign" and self.mut:
            v = r.choice(self.mut)
            return [f"{v} = ({v} + {self.int_expr()}) % 1000"]
        if k == "loop" and depth < 2:
            i = self.fresh("i")
            self.ints.append(i)
            inner = self.block(depth + 1, in_fn)
            self.ints.remove(i)
            return [f"for {i} in range({r.randint(0, 5)}):"] + ["    " + ln for ln in inner]
        if k == "while" and depth < 2:
            c = self.fresh("w")
            inner = self.block(depth + 1, in_fn)
            return [f"{c} = 0", f"while {c} < {r.randint(0, 4)}:", f"    {c} = {c} + 1"] + [
                "    " + ln for ln in inner
            ]
        if k == "str":
            v = self.fresh("s")
            out = [f"{v} = {self.str_expr()}"]
            self.strs.append(v)
            return out
        if k == "list":
            v = self.fresh("xs")
            items = ", ".join(self.int_expr(1) for _ in range(r.randint(1, 4)))
            self.lists.append(v)
            return [f"{v} = [{items}]"]
        if k == "append" and self.lists:
            return [f"{r.choice(self.lists)}.append({self.int_expr(1)})"]
        if k == "pt":
            v = self.fresh("p")
            out = [f"{v} = Pt(x={self.int_expr(1)}, y={self.int_expr(1)})"]
            self.pts.append(v)
            return out
        if k == "shape":
            v = self.fresh("sh")
            out = [f"{v} = {self.shape_expr()}"]
            self.shapes.append(v)
            return out
        if k == "print":
            parts = [f"{{{self.int_expr()}}}"]
            if self.strs:
                parts.append(f"{{{r.choice(self.strs)}}}")
            if self.lists:
                parts.append(f"{{{r.choice(self.lists)}}}")
            if self.pts:
                parts.append(f"{{{r.choice(self.pts)}}}")
            if self.shapes:
                parts.append(f"{{{r.choice(self.shapes)}}}")
            return [f'io.print(f"{" ".join(parts)}")']
        return []

    def program(self) -> str:
        lines = HEADER.splitlines() + [""]
        for _ in range(self.r.randint(0, 3)):
            lines += self.function()
        lines.append("pub def main() uses io:")
        body: list[str] = []
        for _ in range(self.r.randint(6, 16)):
            body += self.stmt()
        body.append(f'io.print(f"{{{self.int_expr()}}}")')
        lines += ["    " + ln for ln in body]
        return "\n".join(lines) + "\n"


def generate(seed: int) -> str:
    return Gen(seed).program()


def diverges(yan: Path, names: list[str], path: Path) -> list[difftest.Mismatch]:
    oracle = difftest.run_one(yan, difftest.ORACLE, str(path), [])
    found: list[difftest.Mismatch] = []
    for b in names[1:]:
        found += difftest.compare(str(path), b, oracle, difftest.run_one(yan, b, str(path), []))
    return found


def minimize(yan: Path, names: list[str], src: str, path: Path) -> str:
    """Delete lines while the program still diverges. Compile errors diverge nowhere, so they
    are rejected for free."""
    lines = src.splitlines()
    protected = len(HEADER.splitlines())
    changed = True
    while changed:
        changed = False
        for i in range(len(lines) - 1, protected, -1):
            trial = lines[:i] + lines[i + 1 :]
            path.write_text("\n".join(trial) + "\n", encoding="utf-8")
            if diverges(yan, names, path):
                lines, changed = trial, True
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return "\n".join(lines) + "\n"


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("--yan", type=Path, default=difftest.ROOT / "target" / "release" / "yan")
    ap.add_argument("--seeds", type=int, default=100, help="how many seeds to run")
    ap.add_argument("--start", type=int, default=0, help="first seed")
    ap.add_argument("--jobs", type=int, default=4)
    ap.add_argument("--keep", type=Path, help="write each mismatching program here")
    ap.add_argument("--show", type=int, metavar="SEED", help="print the program of SEED and exit")
    ap.add_argument("--no-minimize", action="store_true")
    args = ap.parse_args(argv)

    if args.show is not None:
        sys.stdout.write(generate(args.show))
        return 0

    names = difftest.backends(args.yan)
    work = Path(tempfile.mkdtemp(prefix="yanfuzz-"))

    def one(seed: int) -> tuple[int, list[difftest.Mismatch], str]:
        path = work / f"seed{seed}.yag"
        src = generate(seed)
        path.write_text(src, encoding="utf-8")
        found = diverges(args.yan, names, path)
        if found and not args.no_minimize:
            src = minimize(args.yan, names, src, path)
            found = diverges(args.yan, names, path)
        return seed, found, src

    seeds = range(args.start, args.start + args.seeds)
    with ThreadPoolExecutor(args.jobs) as pool:
        results = list(pool.map(one, seeds))
    bad = [(s, ms, src) for s, ms, src in results if ms]
    for seed, ms, src in bad:
        print(f"DIVERGE seed {seed}: {', '.join(sorted({f'{m.backend}/{m.field}' for m in ms}))}")
        for m in ms:
            print(f"  {m.backend} {m.field}: oracle={m.oracle[:200]!r} got={m.got[:200]!r}")
        print("--- minimized program ---")
        print(src, end="")
        if args.keep:
            args.keep.mkdir(parents=True, exist_ok=True)
            (args.keep / f"seed{seed}.yag").write_text(src, encoding="utf-8")
    print(
        f"fuzz: seeds {seeds.start}..{seeds.stop - 1} x {names[1:]} vs {difftest.ORACLE}: "
        f"{len(bad)} diverging"
    )
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
