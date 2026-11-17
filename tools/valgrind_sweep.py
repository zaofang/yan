"""Valgrind sweep (#1583): build every difftest program and example natively and run it under
valgrind. Any invalid access or definitely-lost block fails (exit code 99). A program that does
not build (a refusal) is skipped. Programs listed in tools/valgrind_allow.toml are expected to
fail (their issue owns the fix); an entry that no longer fails is stale and fails the run.

Nightly only: each program is built and run under valgrind.
"""

import argparse
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from tools import difftest, nativelive

ROOT = difftest.ROOT
ALLOWLIST = ROOT / "tools" / "valgrind_allow.toml"
ERROR_EXIT = 99
VALGRIND = [
    "valgrind",
    "-q",
    f"--error-exitcode={ERROR_EXIT}",
    "--leak-check=full",
    # `static_str_block` leaks one block per string literal by design.
    "--errors-for-leak-kinds=definite",
]


def clean(yan: Path, program: str, flags: list[str], tmp: Path) -> str | None:
    """None when `program` runs clean or does not build, else the first valgrind report line."""
    exe = tmp / program.replace("/", "_")
    build = subprocess.run(
        [str(yan), "build", *flags, str(ROOT / program), "-o", str(exe)],
        capture_output=True,
        timeout=difftest.TIMEOUT,
        cwd=ROOT,
    )
    if build.returncode != 0:
        return None
    try:
        out = subprocess.run(
            [*VALGRIND, str(exe)],
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            timeout=difftest.TIMEOUT,
            cwd=ROOT,
        )
    except subprocess.TimeoutExpired:
        return "timeout"
    if out.returncode != ERROR_EXIT:
        return None
    lines = [ln for ln in out.stderr.splitlines() if ln.startswith("==")]
    return lines[0].split("== ", 1)[-1][:120] if lines else "valgrind error"


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

    with tempfile.TemporaryDirectory() as d, ThreadPoolExecutor(args.jobs) as pool:
        found = list(pool.map(lambda pf: clean(args.yan, pf[0], pf[1], Path(d)), progs))
    hits = {p: what for (p, _), what in zip(progs, found, strict=True) if what}

    unexplained = {p: w for p, w in hits.items() if p not in allow}
    stale = sorted(p for p in allow if p not in hits and (args.only is None or args.only in p))
    for p, w in sorted(unexplained.items()):
        print(f"VALGRIND {p}: {w}")
    for p in stale:
        print(f"STALE allowlist entry {p} ({allow[p]})")
    print(
        f"valgrind: {len(progs)} programs: {len(unexplained)} failing, "
        f"{len(hits) - len(unexplained)} allowlisted, {len(stale)} stale"
    )
    return 0 if args.report_only or not (unexplained or stale) else 1


if __name__ == "__main__":
    sys.exit(main())
