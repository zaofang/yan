"""Undefined-behavior sweep (#1583): run every difftest program and the real-world examples on
the abstract-byte machine with `--ub` and fail on any UB report, leak report, Rust panic or
machine fault. A program the machine cannot run yet ("not supported yet") is skipped, not failed.

A program listed in tools/ubsweep_allow.toml is expected to fail (its issue owns the fix); an
entry that no longer fails is stale and fails the run.
"""

import argparse
import re
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from tools import difftest, nativelive

ROOT = difftest.ROOT
ALLOWLIST = ROOT / "tools" / "ubsweep_allow.toml"
FINDING = re.compile(r"^(ub\(\d+\):.*|leak: .*|machine fault: .*|.*panicked at .*)$", re.M)


def finding(yan: Path, program: str, flags: list[str]) -> str | None:
    """The first UB, leak, panic or fault the machine reports for `program`, if any."""
    try:
        out = subprocess.run(
            [str(yan), "run", "--ub", *flags, str(ROOT / program)],
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            timeout=difftest.TIMEOUT,
            cwd=ROOT,
        )
    except subprocess.TimeoutExpired:
        return "timeout"
    m = FINDING.search(out.stderr)
    return m.group(1)[:120] if m else None


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--yan", type=Path, default=ROOT / "target" / "release" / "yan")
    ap.add_argument("--jobs", type=int, default=4)
    ap.add_argument("--allowlist", type=Path, default=ALLOWLIST)
    ap.add_argument("--only", help="substring filter on program paths")
    ap.add_argument("--report-only", action="store_true", help="always exit 0")
    args = ap.parse_args(argv)

    progs = [(p, f) for p, f in nativelive.programs() if args.only is None or args.only in p]
    allow = nativelive.load_allowlist(args.allowlist)

    with ThreadPoolExecutor(args.jobs) as pool:
        found = list(pool.map(lambda pf: finding(args.yan, *pf), progs))
    hits = {p: what for (p, _), what in zip(progs, found, strict=True) if what}

    unexplained = {p: w for p, w in hits.items() if p not in allow}
    stale = sorted(p for p in allow if p not in hits and (args.only is None or args.only in p))
    for p, w in sorted(unexplained.items()):
        print(f"UB {p}: {w}")
    for p in stale:
        print(f"STALE allowlist entry {p} ({allow[p]})")
    print(
        f"ubsweep: {len(progs)} programs: {len(unexplained)} failing, "
        f"{len(hits) - len(unexplained)} allowlisted, {len(stale)} stale"
    )
    return 0 if args.report_only or not (unexplained or stale) else 1


if __name__ == "__main__":
    sys.exit(main())
