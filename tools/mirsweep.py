"""MIR validator sweep (#1513 S3, RFC 0097 D7): run `yan view --as mir --check` on every difftest
program and real-world example and fail on any validator violation. A program MIR cannot lower
yet ("not supported yet") is skipped, not failed.

A program listed in tools/mirsweep_allow.toml is expected to fail (its issue owns the fix); an
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
ALLOWLIST = ROOT / "tools" / "mirsweep_allow.toml"
VIOLATION = re.compile(r"^(I1\d\d): ", re.M)


def violations(yan: Path, program: str) -> list[str]:
    """The distinct I-codes `yan view --as mir --check` reports for `program`."""
    try:
        out = subprocess.run(
            [str(yan), "view", "--as", "mir", "--check", str(ROOT / program)],
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            timeout=difftest.TIMEOUT,
            cwd=ROOT,
        )
    except subprocess.TimeoutExpired:
        return ["timeout"]
    return sorted(set(VIOLATION.findall(out.stderr)))


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--yan", type=Path, default=ROOT / "target" / "release" / "yan")
    ap.add_argument("--jobs", type=int, default=4)
    ap.add_argument("--allowlist", type=Path, default=ALLOWLIST)
    ap.add_argument("--only", help="substring filter on program paths")
    ap.add_argument("--report-only", action="store_true", help="always exit 0")
    args = ap.parse_args(argv)

    progs = [p for p, _ in nativelive.programs() if args.only is None or args.only in p]
    allow = nativelive.load_allowlist(args.allowlist)

    with ThreadPoolExecutor(args.jobs) as pool:
        found = list(pool.map(lambda p: violations(args.yan, p), progs))
    hits = {p: codes for p, codes in zip(progs, found, strict=True) if codes}

    unexplained = {p: c for p, c in hits.items() if p not in allow}
    stale = sorted(p for p in allow if p not in hits and (args.only is None or args.only in p))
    for p, c in sorted(unexplained.items()):
        print(f"MIR {p}: {' '.join(c)}")
    for p in stale:
        print(f"STALE allowlist entry {p} ({allow[p]})")
    print(
        f"mirsweep: {len(progs)} programs: {len(unexplained)} failing, "
        f"{len(hits) - len(unexplained)} allowlisted, {len(stale)} stale"
    )
    return 0 if args.report_only or not (unexplained or stale) else 1


if __name__ == "__main__":
    sys.exit(main())
