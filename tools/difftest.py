"""Differential harness (DL5): run every conformance run case, bench program and example on each
backend and compare stdout, exit status and stderr diagnostics against the interpreter, the oracle.

Backends are the values of `yan run --backend`; `vm` joins automatically once the CLI lists it.
A mismatch is unexplained unless tools/difftest_allow.toml lists the program and backend with an
issue link. An allowlist entry that no longer mismatches is stale and fails the run.
"""

import argparse
import json
import re
import subprocess
import sys
import tomllib
from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor
from dataclasses import asdict, dataclass
from pathlib import Path

from tools import conform

ROOT = Path(__file__).resolve().parent.parent
ORACLE = "interp"
ALLOWLIST = ROOT / "tools" / "difftest_allow.toml"
TIMEOUT = 120
TOOL_FAILURE = 2
PANIC = re.compile(r"^(panic\([a-z_]+\)):.*$")
EXAMPLES = ("examples/tally/g5_tally.yag", "examples/tally/g5_tally.yn")


@dataclass(frozen=True)
class Outcome:
    stdout: str
    exit: int
    stderr: str


@dataclass
class Mismatch:
    program: str
    backend: str
    field: str
    oracle: str
    got: str


def backends(yan: Path) -> list[str]:
    out = subprocess.run([str(yan), "run", "--help"], capture_output=True, text=True).stdout
    m = re.search(r"--backend <BACKEND>.*?possible values: ([^\]]+)\]", out, re.S)
    if m is None:
        sys.exit("difftest: cannot read the backend list from `yan run --help`")
    names = [b.strip() for b in m.group(1).split(",")]
    return [ORACLE, *(b for b in names if b != ORACLE)]


def programs() -> list[tuple[str, list[str]]]:
    """(repo-relative path, flags) of every program the law covers."""
    found: dict[str, list[str]] = {}
    for case in conform.discover(ROOT / "conformance"):
        if case.kind == "run":
            found[case.path.relative_to(ROOT).as_posix()] = case.flags
    for p in sorted((ROOT / "bench" / "programs").glob("*/main.yir")):
        found[p.relative_to(ROOT).as_posix()] = []
    for e in EXAMPLES:
        found[e] = []
    return sorted(found.items())


def run_one(yan: Path, backend: str, program: str, flags: list[str]) -> Outcome:
    try:
        out = subprocess.run(
            [str(yan), "run", "--backend", backend, *flags, str(ROOT / program)],
            capture_output=True,
            text=True,
            timeout=TIMEOUT,
            cwd=ROOT,
        )
    except subprocess.TimeoutExpired:
        return Outcome("", -1, f"timeout after {TIMEOUT}s")
    return Outcome(out.stdout, out.returncode, diagnostics(out.stderr))


def diagnostics(stderr: str) -> str:
    """Stderr as the law compares it: no backtrace frames, panics reduced to their kind.

    Panic detail text (operands, line numbers) is not part of the contract and native code
    does not carry it; the kind in `panic(kind)` and the exit status are.
    """
    lines = [
        ln for ln in stderr.replace(str(ROOT) + "/", "").splitlines() if not ln.startswith("  at ")
    ]
    return "".join(PANIC.sub(r"\1", ln) + "\n" for ln in lines)


def refused(o: Outcome) -> bool:
    return o.exit == TOOL_FAILURE and o.stderr.startswith("yan: not supported yet")


def compare(program: str, backend: str, oracle: Outcome, got: Outcome) -> list[Mismatch]:
    if refused(got):
        return [Mismatch(program, backend, "refused", "", got.stderr.strip())]
    return [
        Mismatch(program, backend, f, str(getattr(oracle, f)), str(getattr(got, f)))
        for f in ("stdout", "exit", "stderr")
        if getattr(oracle, f) != getattr(got, f)
    ]


def load_allowlist(path: Path) -> dict[tuple[str, str], str]:
    if not path.exists():
        return {}
    entries = tomllib.loads(path.read_text(encoding="utf-8")).get("allow", [])
    for e in entries:
        if not re.fullmatch(r"#\d+", e.get("issue", "")):
            sys.exit(f'difftest: allowlist entry {e.get("program")} needs issue = "#N"')
    return {(e["program"], e["backend"]): e["issue"] for e in entries}


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--yan", type=Path, default=ROOT / "target" / "release" / "yan")
    ap.add_argument("--json", type=Path, help="write the report here")
    ap.add_argument("--jobs", type=int, default=4)
    ap.add_argument("--allowlist", type=Path, default=ALLOWLIST)
    ap.add_argument("--only", help="substring filter on program paths")
    ap.add_argument(
        "--strict-refusals",
        action="store_true",
        help="count `not supported yet` as a mismatch (#758)",
    )
    ap.add_argument("--report-only", action="store_true", help="always exit 0")
    args = ap.parse_args(argv)

    names = backends(args.yan)
    progs = [(p, f) for p, f in programs() if args.only is None or args.only in p]
    allow = load_allowlist(args.allowlist)

    def work(item: tuple[str, list[str]]) -> list[Mismatch]:
        program, flags = item
        oracle = run_one(args.yan, ORACLE, program, flags)
        found: list[Mismatch] = []
        for b in names[1:]:
            found += compare(program, b, oracle, run_one(args.yan, b, program, flags))
        return found

    with ThreadPoolExecutor(args.jobs) as pool:
        found = [m for ms in pool.map(work, progs) for m in ms]
    refusals = [m for m in found if m.field == "refused"]
    mismatches = found if args.strict_refusals else [m for m in found if m.field != "refused"]

    seen = {(m.program, m.backend) for m in mismatches}
    unexplained = [m for m in mismatches if (m.program, m.backend) not in allow]
    stale = sorted(k for k in allow if k not in seen and (args.only is None or args.only in k[0]))

    by_pair: dict[tuple[str, str], list[Mismatch]] = defaultdict(list)
    for m in unexplained:
        by_pair[(m.program, m.backend)].append(m)
    for (program, backend), ms in sorted(by_pair.items()):
        print(f"MISMATCH {program} [{backend} vs {ORACLE}]: {', '.join(m.field for m in ms)}")
        for m in ms:
            print(f"  {m.field}: oracle={m.oracle[:200]!r} got={m.got[:200]!r}")
    for program, backend in stale:
        print(f"STALE allowlist entry {program} [{backend}] ({allow[(program, backend)]})")
    explained = len(seen) - len(by_pair)
    print(
        f"difftest: {len(progs)} programs x {names[1:]} vs {ORACLE}: "
        f"{len(by_pair)} unexplained, {explained} allowlisted, {len(stale)} stale, "
        f"{len(refusals)} refused by a backend"
    )

    if args.json:
        args.json.write_text(
            json.dumps(
                {
                    "backends": names,
                    "programs": len(progs),
                    "unexplained": [asdict(m) for m in unexplained],
                    "allowlisted": sorted(
                        f"{p} [{b}] {allow[(p, b)]}" for p, b in seen & allow.keys()
                    ),
                    "refused": [f"{m.program} [{m.backend}] {m.got}" for m in refusals],
                    "stale": [f"{p} [{b}]" for p, b in stale],
                },
                indent=2,
            )
            + "\n",
            encoding="utf-8",
        )
    failed = bool(by_pair or stale)
    return 0 if args.report_only or not failed else 1


if __name__ == "__main__":
    sys.exit(main())
