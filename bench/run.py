"""Time each program in bench/programs under the Yan interpreter, typed Python and Rust.

Every program directory holds main.yir, main.py and main.rs, which compute the same result with the
same algorithm and assert it, so a wrong answer fails the run. Each time is the median wall time of
--runs runs after one warmup, minus the median time of the same language's bench/startup program.
Rust is built with overflow checks on, the setting the M2a gate uses (R389).
"""

import argparse
import json
import platform
import statistics
import subprocess
import sys
import time
from dataclasses import asdict, dataclass
from pathlib import Path

BENCH = Path(__file__).resolve().parent
ROOT = BENCH.parent
LANGS = ("yan", "py", "rs")
FLOOR = 0.001
RUSTC_FLAGS = ("-C", "opt-level=3", "-C", "overflow-checks=on", "-C", "codegen-units=1")


@dataclass
class Timing:
    median: float
    low: float
    high: float


@dataclass
class Row:
    program: str
    times: dict[str, Timing]

    def net(self, lang: str, startup: dict[str, float]) -> float:
        return max(self.times[lang].median - startup[lang], 0.0)


def build_rust(src: Path, out_dir: Path) -> Path:
    exe = out_dir / src.parent.name
    cmd = ["rustc", "--edition", "2024", *RUSTC_FLAGS, "-o", str(exe), str(src)]
    subprocess.run(cmd, check=True, cwd=ROOT)
    return exe


def commands(prog: Path, yan: Path, python: str, rust_dir: Path) -> dict[str, list[str]]:
    return {
        "yan": [str(yan), "run", str(prog / "main.yir")],
        "py": [python, str(prog / "main.py")],
        "rs": [str(build_rust(prog / "main.rs", rust_dir))],
    }


def measure(cmd: list[str], runs: int) -> Timing:
    samples = []
    for i in range(runs + 1):
        start = time.perf_counter()
        done = subprocess.run(cmd, capture_output=True, text=True)
        elapsed = time.perf_counter() - start
        if done.returncode != 0:
            sys.exit(f"{' '.join(cmd)} exited {done.returncode}\n{done.stderr}")
        if i > 0:
            samples.append(elapsed)
    return Timing(statistics.median(samples), min(samples), max(samples))


def ms(seconds: float) -> str:
    return f"{seconds * 1000:.1f}"


def ratio(num: float, den: float, digits: int) -> str:
    # Below FLOOR a time is startup noise, so the ratio is only a lower bound.
    bound = "> " if den < FLOOR else ""
    return f"{bound}{num / max(den, FLOOR):.{digits}f}x"


def report(rows: list[Row], startup: dict[str, float]) -> str:
    lines = [
        "| program | yan ms | python ms | rust ms | yan/python | yan/rust |",
        "|---|---:|---:|---:|---:|---:|",
    ]
    for r in rows:
        net = {lang: r.net(lang, startup) for lang in LANGS}
        lines.append(
            f"| {r.program} | {ms(net['yan'])} | {ms(net['py'])} | {ms(net['rs'])} "
            f"| {ratio(net['yan'], net['py'], 1)} | {ratio(net['yan'], net['rs'], 0)} |"
        )
    yp = statistics.geometric_mean(
        r.net("yan", startup) / max(r.net("py", startup), FLOOR) for r in rows
    )
    lines.append(f"\nGeometric mean, yan/python: {yp:.1f}x.")
    lines.append(
        "Startup subtracted (ms): "
        + ", ".join(f"{lang} {ms(startup[lang])}" for lang in LANGS)
        + "."
    )
    return "\n".join(lines)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--yan", type=Path, default=ROOT / "target/release/yan")
    ap.add_argument("--python", default=sys.executable)
    ap.add_argument("--runs", type=int, default=5)
    ap.add_argument("--json", type=Path, help="also write the raw timings here")
    ap.add_argument("programs", nargs="*", help="program names (default: all)")
    args = ap.parse_args()

    rust_dir = ROOT / "target/bench"
    rust_dir.mkdir(parents=True, exist_ok=True)
    progs = sorted(p for p in (BENCH / "programs").iterdir() if p.is_dir())
    if args.programs:
        progs = [p for p in progs if p.name in args.programs]

    base = commands(BENCH / "startup", args.yan, args.python, rust_dir)
    startup = {lang: measure(cmd, args.runs).median for lang, cmd in base.items()}
    rows = []
    for prog in progs:
        cmds = commands(prog, args.yan, args.python, rust_dir)
        rows.append(Row(prog.name, {lang: measure(cmd, args.runs) for lang, cmd in cmds.items()}))
        print(f"{prog.name} done", file=sys.stderr)

    print(report(rows, startup))
    if args.json:
        env = {"machine": platform.machine(), "system": platform.system(), "python": args.python}
        data = {"env": env, "startup": startup, "rows": [asdict(r) for r in rows]}
        args.json.write_text(json.dumps(data, indent=2) + "\n")


if __name__ == "__main__":
    main()
