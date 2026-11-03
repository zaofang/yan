"""Differential testing of the Lean model of Yan-K against a Yan executor (formal core, phase 0).

    uv run python formal/drt.py cases [--yan CMD] [--yancore CMD]
    uv run python formal/drt.py gen --count N [--seed S] [--keep DIR] [--yan CMD] [--yancore CMD]

`cases` runs every conformance/yir/run case on both. `gen` writes N random Yan-K programs
and runs each on both. A run is compared on stdout, exit status and panic kind. Exit status 2
from either side means "not supported yet" and skips the program. Any other difference is a
bug in one implementation or a gap in the spec; the program is kept for a report.
"""

import argparse
import random
import re
import shlex
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
YAN = str(ROOT / "target/release/yan")
YANCORE = str(ROOT / "formal/.lake/build/bin/yancore")


@dataclass(frozen=True)
class Run:
    exit: int
    stdout: str
    panic: str

    def key(self) -> tuple[int, str, str]:
        return (self.exit, self.stdout, self.panic)


def run(cmd: str, file: Path) -> Run:
    try:
        p = subprocess.run(
            [*shlex.split(cmd), "run", str(file)], capture_output=True, text=True, timeout=120
        )
    except subprocess.TimeoutExpired:
        return Run(2, "", "timeout")
    panic = ""
    for ln in p.stderr.splitlines():
        # `panic(kind): message` since 8.3; the interpreter used to print `panic: kind`
        if m := re.match(r"panic(?:\((\w+)\))?: ?(\w*)", ln):
            panic = m.group(1) or m.group(2)
            break
    return Run(p.returncode, p.stdout, panic)


def compare(files: list[Path], yan: str, yancore: str, keep: Path | None) -> int:
    agree = skipped = 0
    outcomes: dict[str, int] = {}
    differ: list[tuple[Path, Run, Run]] = []
    for f in files:
        a, b = run(yan, f), run(yancore, f)
        if a.exit == 2 or b.exit == 2:
            skipped += 1
        elif a.key() == b.key():
            agree += 1
            o = f"panic {a.panic}" if a.panic else f"exit {a.exit}"
            outcomes[o] = outcomes.get(o, 0) + 1
        else:
            differ.append((f, a, b))
            if keep:
                keep.mkdir(parents=True, exist_ok=True)
                (keep / f.name).write_text(f.read_text())
    for f, a, b in differ:
        print(
            f"DIFFER {f.name}: yan exit={a.exit} panic={a.panic or '-'}"
            f" | yancore exit={b.exit} panic={b.panic or '-'}"
        )
        if a.stdout != b.stdout:
            print(f"  yan stdout:  {a.stdout[:200]!r}")
            print(f"  yancore stdout: {b.stdout[:200]!r}")
    print(f"{agree} agree, {len(differ)} differ, {skipped} skipped (unsupported on one side)")
    print("agreed outcomes: " + ", ".join(f"{k} {v}" for k, v in sorted(outcomes.items())))
    return 1 if differ else 0


SMALL = [0, 1, 2, 3, -1, -2, 5, 7, 10, -7, 100]
LARGE = [2**31, 2**62, -(2**62), 2**63 - 1, -(2**63), 3037000500]
OPS = ["add_checked_i64", "sub_checked_i64", "mul_checked_i64", "floordiv_i64", "mod_i64"]

EMIT = """\
fn emit(cap io: Io, let x: i64) -> Unit {
    var y: i64 = mod_i64(let x, let 1000003)
    loop 'd {
        let z: bool = eq_i64(let y, let 0)
        if z { break 'd }
        let g: i64 = mod_i64(let y, let 10)
        match g {
            0 => { std.io.print(cap io, let "0") }
            1 => { std.io.print(cap io, let "1") }
            2 => { std.io.print(cap io, let "2") }
            3 => { std.io.print(cap io, let "3") }
            4 => { std.io.print(cap io, let "4") }
            5 => { std.io.print(cap io, let "5") }
            6 => { std.io.print(cap io, let "6") }
            7 => { std.io.print(cap io, let "7") }
            8 => { std.io.print(cap io, let "8") }
            _ => { std.io.print(cap io, let "9") }
        }
        y = floordiv_i64(let y, let 10)
    }
    std.io.print(cap io, let ".")
    ret ()
}
"""


class Gen:
    """Random well-typed Yan Core-K programs over i64, a sum, a failure record and `inout`."""

    def __init__(self, rng: random.Random) -> None:
        self.rng = rng
        self.n = 0
        self.fns: list[str] = []

    def fresh(self, p: str) -> str:
        self.n += 1
        return f"{p}{self.n}"

    def lit(self) -> str:
        return str(self.rng.choice(LARGE if self.rng.random() < 0.08 else SMALL))

    def atom(self, ints: list[str]) -> str:
        if ints and self.rng.random() < 0.7:
            return self.rng.choice(ints)
        return self.lit()

    def op(self, ints: list[str]) -> str:
        o = self.rng.choice(OPS)
        return f"{o}(let {self.atom(ints)}, let {self.atom(ints)})"

    def mark(self, ind: str) -> str:
        return f'{ind}std.io.print(cap io, let "{self.fresh("m")}")'

    def block(
        self, ind: str, ints: list[str], vs: list[str], depth: int, can_fail: bool, n: int
    ) -> list[str]:
        out: list[str] = []
        ints = list(ints)
        for _ in range(n):
            out += self.stmt(ind, ints, vs, depth, can_fail)
        return out

    def stmt(
        self, ind: str, ints: list[str], vs: list[str], depth: int, can_fail: bool
    ) -> list[str]:
        r = self.rng
        k = r.choice(
            [
                "let",
                "let",
                "assign",
                "if",
                "match",
                "intmatch",
                "loop",
                "call",
                "fail",
                "par",
                "jp",
                "emit",
                "assert",
            ]
        )
        sub = ind + "    "
        if depth <= 0 and k in ("if", "match", "intmatch", "loop", "par", "jp"):
            k = "let"
        if k == "let":
            x, e = self.fresh("x"), self.op(ints)
            ints.append(x)
            return [f"{ind}let {x}: i64 = {e}"]
        if k == "assign" and vs:
            return [f"{ind}{r.choice(vs)} = {self.op(ints)}"]
        if k == "if":
            c = self.fresh("c")
            a, b = self.atom(ints), self.atom(ints)
            cmp = r.choice(["lt_i64", "eq_i64", "ge_i64"])
            return (
                [f"{ind}let {c}: bool = {cmp}(let {a}, let {b})", f"{ind}if {c} {{", self.mark(sub)]
                + self.block(sub, ints, vs, depth - 1, can_fail, r.randint(0, 2))
                + [f"{ind}}} else {{", self.mark(sub)]
                + self.block(sub, ints, vs, depth - 1, can_fail, r.randint(0, 2))
                + [f"{ind}}}"]
            )
        if k == "match":
            s = self.fresh("s")
            ctor = r.choice(
                [
                    f"A {{ x: {self.atom(ints)} }}",
                    f"B {{ x: {self.atom(ints)}, y: {self.atom(ints)} }}",
                    "C",
                ]
            )
            p, q, w = self.fresh("p"), self.fresh("q"), self.fresh("w")
            return (
                [
                    f"{ind}let {s}: Sh = {ctor}",
                    f"{ind}match {s} {{",
                    f"{sub}A {{ x: {p} }} => {{",
                    self.mark(sub + "    "),
                ]
                + self.block(sub + "    ", [*ints, p], vs, depth - 1, can_fail, 1)
                + [f"{sub}}}", f"{sub}B {{ x: {q}, y: {w} }} => {{", self.mark(sub + "    ")]
                + self.block(sub + "    ", [*ints, q, w], vs, depth - 1, can_fail, 1)
                + [f"{sub}}}", f"{sub}C => {{", self.mark(sub + "    "), f"{sub}}}", f"{ind}}}"]
            )
        if k == "intmatch" and ints:
            v = r.choice(ints)
            return (
                [
                    f"{ind}match {v} {{",
                    f"{sub}0 => {{",
                    self.mark(sub + "    "),
                    f"{sub}}}",
                    f"{sub}1 => {{",
                    self.mark(sub + "    "),
                    f"{sub}}}",
                    f"{sub}_ => {{",
                    self.mark(sub + "    "),
                ]
                + self.block(sub + "    ", ints, vs, depth - 1, can_fail, 1)
                + [f"{sub}}}", f"{ind}}}"]
            )
        if k == "loop":
            i, c, lab = self.fresh("i"), self.fresh("c"), self.fresh("l")
            return (
                [
                    f"{ind}var {i}: i64 = 0",
                    f"{ind}loop '{lab} {{",
                    f"{sub}let {c}: bool = ge_i64(let {i}, let {r.randint(0, 4)})",
                    f"{sub}if {c} {{ break '{lab} }}",
                    self.mark(sub),
                ]
                + self.block(sub, [*ints, i], vs, depth - 1, can_fail, r.randint(1, 2))
                + [f"{sub}{i} = add_checked_i64(let {i}, let 1)", f"{ind}}}"]
            )
        if k == "call" and self.fns and vs:
            f = r.choice(self.fns)
            v = r.choice(vs)
            arg = self.atom([x for x in ints if x != v])
            if can_fail and r.random() < 0.5:
                x = self.fresh("x")
                ints.append(x)
                return [f"{ind}let {x}: i64 = {f}(cap io, let {arg}, inout {v})?"]
            t, ok, e = self.fresh("t"), self.fresh("o"), self.fresh("e")
            return [
                f"{ind}let {t}: Result[i64, Bad] = try {f}(cap io, let {arg}, inout {v})",
                f"{ind}match {t} {{",
                f"{sub}Ok {{ value: {ok} }} => {{ emit(cap io, let {ok}) }}",
                f"{sub}Err {{ error: {e} }} => {{",
                f"{sub}    let {e}c: i64 = {e}.code",
                f"{sub}    emit(cap io, let {e}c)",
                f"{sub}}}",
                f"{ind}}}",
                f"{ind}emit(cap io, let {v})",
            ]
        if k == "fail" and can_fail:
            c, e = self.fresh("c"), self.fresh("e")
            return [
                f"{ind}let {c}: bool = lt_i64(let {self.atom(ints)}, let {self.atom(ints)})",
                f"{ind}if {c} {{",
                f"{sub}let {e}: Bad = Bad {{ code: {self.atom(ints)} }}",
                f"{sub}fail {e}",
                f"{ind}}}",
            ]
        if k == "par":
            a, b = self.fresh("x"), self.fresh("x")
            lines = [
                f"{ind}par {{",
                f"{sub}{{ let {a}: i64 = {self.op(ints)} }}",
                f"{sub}{{ let {b}: i64 = {self.op(ints)} }}",
                f"{ind}}}",
            ]
            ints += [a, b]
            return lines
        if k == "jp" and vs:
            j, rr, c = self.fresh("k"), self.fresh("r"), self.fresh("c")
            w = r.choice(vs)
            return [
                f"{ind}jp {j}(let {rr}: i64) {{",
                f"{sub}{w} = {rr}",
                self.mark(sub),
                f"{ind}}}",
                f"{ind}let {c}: bool = lt_i64(let {self.atom(ints)}, let {self.atom(ints)})",
                f"{ind}if {c} {{ jump {j}({self.atom(ints)}) }} else {{"
                f" jump {j}({self.atom(ints)}) }}",
            ]
        if k == "assert" and r.random() < 0.3:
            return [f"{ind}assert le_i64(let {self.atom(ints)}, let {self.atom(ints)})"]
        if k == "emit" and ints:
            return [f"{ind}emit(cap io, let {r.choice(ints)})"]
        x, e = self.fresh("x"), self.atom(ints)
        ints.append(x)
        return [f"{ind}let {x}: i64 = {e}"]

    def function(self, name: str) -> list[str]:
        w = self.fresh("w")
        body = [f"    var {w}: i64 = a", "    acc = add_checked_i64(let acc, let 1)"]
        body += self.block("    ", ["a", "acc", w], [w, "acc"], 2, True, self.rng.randint(2, 6))
        body.append(f"    ret {self.rng.choice(['a', 'acc', w])}")
        self.fns.append(name)
        return [
            f"fn {name}(cap io: Io, let a: i64, inout acc: i64) -> i64 fail Bad {{",
            *body,
            "}",
            "",
        ]

    def program(self, name: str) -> str:
        lines = [
            f"module drt.{name}",
            "",
            "sum Sh { A { x: i64 }, B { x: i64, y: i64 }, C }",
            "",
            "record Bad { code: i64 }",
            "",
            EMIT,
        ]
        for i in range(self.rng.randint(1, 4)):
            lines += self.function(f"f{i}")
        v = self.fresh("v")
        body = [f"    var {v}: i64 = {self.lit()}"]
        body += self.block("    ", [v], [v], 3, False, self.rng.randint(3, 8))
        body += [f"    emit(cap io, let {v})", "    ret ()"]
        lines += ["fn main(cap io: Io) -> Unit {", *body, "}", ""]
        return "\n".join(lines)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("mode", choices=["cases", "gen"])
    ap.add_argument("--yan", default=YAN)
    ap.add_argument("--yancore", default=YANCORE)
    ap.add_argument("--count", type=int, default=200)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--keep", type=Path)
    a = ap.parse_args()
    if a.mode == "cases":
        files = sorted((ROOT / "conformance/yir/run").glob("*.yir"))
        return compare(files, a.yan, a.yancore, a.keep)
    with tempfile.TemporaryDirectory() as d:
        files = []
        for i in range(a.count):
            p = Path(d) / f"g{a.seed}_{i}.yir"
            p.write_text(Gen(random.Random(a.seed * 1_000_003 + i)).program(f"g{i}"))
            files.append(p)
        return compare(files, a.yan, a.yancore, a.keep)


if __name__ == "__main__":
    sys.exit(main())
