"""Native leak gate (#1500): run every difftest program on the Cranelift backend with
`--report-live` and fail when native code ends with runtime blocks still allocated.

A program that exits 0 or 1 must report `live: 0`. A program listed in
tools/nativelive_allow.toml is expected to leak (its issue owns the fix); an entry that no longer
leaks is stale and fails the run.
"""

import argparse
import re
import subprocess
import sys
import tomllib
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from tools import difftest

ROOT = difftest.ROOT
ALLOWLIST = ROOT / "tools" / "nativelive_allow.toml"
BACKEND = "cranelift"
LIVE = re.compile(r"^live: (\d+)$", re.M)
CHECKED_EXITS = (0, 1)


def programs() -> list[tuple[str, list[str]]]:
    """The differential corpus plus the real-world examples."""
    found = dict(difftest.programs())
    for p in sorted(ROOT.glob("examples/real/*.yag")):
        found[p.relative_to(ROOT).as_posix()] = []
    return sorted(found.items())


def live_blocks(yan: Path, program: str, flags: list[str]) -> int | None:
    """Blocks still live at exit, or None when the program did not end normally or was refused."""
    try:
        out = subprocess.run(
            [str(yan), "run", "--backend", BACKEND, "--report-live", *flags, str(ROOT / program)],
            capture_output=True,
            text=True,
            timeout=difftest.TIMEOUT,
            cwd=ROOT,
        )
    except subprocess.TimeoutExpired:
        return None
    m = LIVE.search(out.stderr)
    return int(m.group(1)) if m and out.returncode in CHECKED_EXITS else None


def load_allowlist(path: Path) -> dict[str, str]:
    if not path.exists():
        return {}
    entries = tomllib.loads(path.read_text(encoding="utf-8")).get("allow", [])
    for e in entries:
        if not re.fullmatch(r"#\d+", e.get("issue", "")):
            sys.exit(f'nativelive: allowlist entry {e.get("program")} needs issue = "#N"')
    return {e["program"]: e["issue"] for e in entries}


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--yan", type=Path, default=ROOT / "target" / "release" / "yan")
    ap.add_argument("--jobs", type=int, default=4)
    ap.add_argument("--allowlist", type=Path, default=ALLOWLIST)
    ap.add_argument("--only", help="substring filter on program paths")
    ap.add_argument("--report-only", action="store_true", help="always exit 0")
    args = ap.parse_args(argv)

    progs = [(p, f) for p, f in programs() if args.only is None or args.only in p]
    allow = load_allowlist(args.allowlist)

    with ThreadPoolExecutor(args.jobs) as pool:
        lives = list(pool.map(lambda pf: live_blocks(args.yan, *pf), progs))
    leaks = {p: n for (p, _), n in zip(progs, lives, strict=True) if n}

    unexplained = {p: n for p, n in leaks.items() if p not in allow}
    stale = sorted(p for p in allow if p not in leaks and (args.only is None or args.only in p))
    for p, n in sorted(unexplained.items()):
        print(f"LEAK {p}: live {n}")
    for p in stale:
        print(f"STALE allowlist entry {p} ({allow[p]})")
    print(
        f"nativelive: {len(progs)} programs: {len(unexplained)} leaking, "
        f"{len(leaks) - len(unexplained)} allowlisted, {len(stale)} stale"
    )
    return 0 if args.report_only or not (unexplained or stale) else 1


if __name__ == "__main__":
    sys.exit(main())
