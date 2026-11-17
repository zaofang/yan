"""Thread-count matrix (RFC 0086 D6, #1584): run every program at each thread count and compare
stdout, exit status and diagnostics byte for byte against the first count. No allowlist: a
divergence is a bug in the pool.

The count reaches the runtime as YAN_THREADS. Until the pool honours it every count runs the same
sequential code, so the matrix passes trivially; it fails the day a count changes a result.
"""

import argparse
import os
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from tools import difftest

ROOT = difftest.ROOT
COUNTS = (1, 2, 4, 8, 16)


def run(yan: Path, backend: str, program: str, flags: list[str], threads: int) -> difftest.Outcome:
    try:
        out = subprocess.run(
            [str(yan), "run", "--backend", backend, *flags, str(ROOT / program)],
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            timeout=difftest.TIMEOUT,
            cwd=ROOT,
            env={**os.environ, "YAN_THREADS": str(threads)},
        )
    except subprocess.TimeoutExpired:
        return difftest.Outcome("", -1, f"timeout after {difftest.TIMEOUT}s")
    return difftest.Outcome(out.stdout, out.returncode, difftest.diagnostics(out.stderr))


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--yan", type=Path, default=ROOT / "target" / "release" / "yan")
    ap.add_argument("--backend", default="cranelift")
    ap.add_argument("--counts", type=int, nargs="+", default=list(COUNTS))
    ap.add_argument("--repeats", type=int, default=2, help="runs per count")
    ap.add_argument("--jobs", type=int, default=4)
    ap.add_argument("--only", help="substring filter on program paths")
    args = ap.parse_args(argv)

    progs = [(p, f) for p, f in difftest.programs() if args.only is None or args.only in p]
    runs = [n for n in args.counts for _ in range(args.repeats)]

    def work(item: tuple[str, list[str]]) -> list[str]:
        program, flags = item
        base = run(args.yan, args.backend, program, flags, runs[0])
        bad = []
        for n in runs[1:]:
            got = run(args.yan, args.backend, program, flags, n)
            fields = [
                f for f in ("stdout", "exit", "stderr") if getattr(base, f) != getattr(got, f)
            ]
            if fields:
                bad.append(f"DIVERGENCE {program} threads={n} vs {runs[0]}: {', '.join(fields)}")
        return bad

    with ThreadPoolExecutor(args.jobs) as pool:
        bad = [line for lines in pool.map(work, progs) for line in lines]
    print("\n".join(bad))
    print(
        f"threadmatrix: {len(progs)} programs on {args.backend} at counts {args.counts} "
        f"x{args.repeats}: {len(bad)} divergences"
    )
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
