"""Collects CI results into the status data the website renders (law 14: every figure carries its
commit and date, and none is typed by hand).

uv run python -m tools.status --conformance C.json --difftest D.json --bench B.json \
    --out status.json

C.json is `tools/conform.py --json` over every case (no --milestone), D.json is
`tools.difftest --json`, B.json is `bench/run.py --json`. Spec debt is computed here from
spec/rows.toml, the conformance case headers and spec/code-map.toml.
"""

import argparse
import json
import subprocess
import tomllib
from collections import Counter
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from tools import conform, rows

ROOT = Path(__file__).resolve().parent.parent
CONFORMANCE = ROOT / "conformance"
CODE_MAP = ROOT / "spec" / "code-map.toml"


def git(*args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout.strip()


def case_key(path: str | Path) -> str:
    return str((ROOT / path).resolve().relative_to(ROOT))


def conformance(summary: dict[str, Any]) -> dict[str, Any]:
    milestone = {case_key(c.path): c.milestone for c in conform.discover(CONFORMANCE)}
    by: dict[str, Counter[str]] = {}
    for r in summary["results"]:
        by.setdefault(milestone[case_key(r["case"])], Counter())[r["status"]] += 1
    order = [m for m in conform.MILESTONES if m in by]
    return {
        "executors": summary["executors"],
        "milestones": {
            m: {"cases": sum(by[m].values()), **{s: by[m][s] for s in ("pass", "fail", "skip")}}
            for m in order
        },
    }


def differential(report: dict[str, Any]) -> dict[str, Any]:
    return {
        "backends": report["backends"],
        "programs": report["programs"],
        **{k: len(report[k]) for k in ("unexplained", "allowlisted", "refused", "stale")},
    }


def spec_debt() -> dict[str, int]:
    owner, _ = rows.allocations(rows.load())
    allocated = {r for r in owner if r.startswith("R")}
    cased = {
        r for c in conform.discover(CONFORMANCE) for label in c.rows for r in rows.expand(label)
    }
    codes = tomllib.loads(CODE_MAP.read_text())["codes"]
    return {
        "rows": len(allocated),
        "rows_without_case": len(allocated - cased),
        "codes": len(codes),
        "codes_unimplemented": sum(isinstance(e, dict) for e in codes.values()),
    }


def bench(data: dict[str, Any]) -> dict[str, Any]:
    startup = data["startup"]
    return {
        "env": data["env"],
        "programs": [
            {
                "program": r["program"],
                "ms": {
                    lang: round(max(t["median"] - startup[lang], 0.0) * 1000, 1)
                    for lang, t in r["times"].items()
                },
            }
            for r in data["rows"]
        ],
    }


def collect(conf: Path, diff: Path, bench_file: Path) -> dict[str, Any]:
    return {
        "schema": 1,
        "commit": git("rev-parse", "HEAD"),
        "date": datetime.now(UTC).date().isoformat(),
        "conformance": conformance(json.loads(conf.read_text())),
        "differential": differential(json.loads(diff.read_text())),
        "spec_debt": spec_debt(),
        "bench": bench(json.loads(bench_file.read_text())),
    }


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--conformance", type=Path, required=True)
    ap.add_argument("--difftest", type=Path, required=True)
    ap.add_argument("--bench", type=Path, required=True)
    ap.add_argument("--out", type=Path, required=True)
    args = ap.parse_args()
    data = collect(args.conformance, args.difftest, args.bench)
    args.out.write_text(json.dumps(data, indent=2) + "\n")


if __name__ == "__main__":
    main()
