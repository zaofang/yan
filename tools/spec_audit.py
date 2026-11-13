# ruff: noqa: E501
"""Spec row audit: implementation evidence, conformance coverage and difficulty per row.

    uv run python -m tools.spec_audit --conform-json C.json --difftest-json D.json \\
        --csv research/2026-11-13-spec-difficulty-audit.csv --summary

Inputs are the outputs of
    tools/conform.py --executor interp=target/release/yan --json C.json --report-only
    tools/difftest.py --json D.json --report-only

Evidence per row (no judgement): cases citing the row, their interpreter verdict, their native
(cranelift) verdict from the differential run, and crates whose source cites the row.
Difficulty and the hard-core flag are judgement (CLASSES, one class per rows.toml entry, rows
inherit it). Only the R rows are audited, the same population as tools.status spec_debt.
"""

import argparse
import csv
import json
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

from tools import conform, difftest, rows

ROOT = Path(__file__).resolve().parent.parent
FRONT = {
    "yan-cst",
    "yan-yag",
    "yan-yn",
    "yan-surface",
    "yan-elab",
    "yan-check",
    "yan-flow",
    "yan-names",
    "yan-hir",
    "yan-diag",
    "yan-core",
    "yan-ebnf",
}
NATIVE = {"yan-cg-cranelift", "yan-mir", "yan-opt", "yan-rt"}
DONE = {"native", "interp"}  # a citing case passes
INTERP = {"yan-interp", "yan-std", "yan-sandbox"}

# Hand classification per rows.toml entry index (the row ranges of one entry share a class).
# "idx[-idx],...|difficulty 1-5|hard-core category (empty if none)|one-line reason".
# Judgement, reviewed by the lead; the evidence columns are mechanical. Unlisted entries: 2.
CLASSES = """
0|4|ownership|view exclusivity and copy-out rules across every pass
1|4|concurrency|scheduler and I/O handler semantics
4|3||fallible allocation siblings across std
6|4||generators lower to resumable state machines
8|4|gpu/kernel|GPU target and performance bar
12|3||toolchain capability, query and undo
13,22,23,25,26,37,38,44,55,58,74,78,80,81,83,86,90,97,103,105,119,121,135|3||cross-cutting rule or mid-size subsystem
144,145,149,160,163,169,171,174,177,185,213,214,215,243,244,263,264,281,284-292,314,315,331,332,333|3||cross-cutting rule or mid-size subsystem
20,21,96,115|4||numerics: libm, reductions, autodiff, correctly rounded math
29,32,261,262|4||ABI and foreign-call layout
30,319,321,322|4|concurrency|select, supervision and restart semantics
31|5|verification|solver reasoning on bit operators
34,36,42,48,91,162,164,165,175,268,279,297-300,336|4|concurrency|structured concurrency, mailboxes, cancellation, scheduling
35,94,247,280,282,283|4|staging|compile-time evaluation, reflection and type functions
43,69,114,267,323,324|4|verification|contract, cost and header-fact checking
46,294,312,313|4|gpu/kernel|kernel regions, tiles and launch
49,50,51,52,53,301|5|distributed|location-transparent remote calls, faults, simulated nodes
316,317,318|4|distributed|remote send, undelivered and stop reasons
54,117|4||deferred overflow, check elision, loop versioning
62,63,138,166,167,245,302,303,305|4|ownership|moves, drops, partial moves, flow facts
304|5|ownership|fixed-point maybe-moved analysis
64,65,127,172,173,335|4|effects|capabilities, determinism and relaxed-math blocks
82|4|effects|model calls, durable resume, untrusted values
112,113,133,293,295,296|5|verification|exact solver, invariant inference, affine index proofs
99,102,107,141,337,338|5|research|research track, not normative for 1.0
98,100,116,118,168|4||post-1.0 surface, engine or agent-loop work
120|5|concurrency|session types
128|5|formal|Lean model of the core
129|4|formal|gaps found by the formal model
143|4|concurrency|cancel scopes lowered to deadline statements
307|1||decision record (language choice)
308,309,310|5|selfhost/llvm|self-hosting stages, LLVM backend, compiler in Yan
311|2||sequencing statement for S2
329,330,7,9,10,18,47,84,109,139,140,142,148|1||policy, naming or scope statement; no engineering
"""
RULES: dict[int, tuple[int, str, str]] = {}
for line in CLASSES.strip().splitlines():
    idxs, d, cat, why = line.split("|")
    for part in idxs.split(","):
        lo, _, hi = part.partition("-")
        for i in range(int(lo), int(hi or lo) + 1):
            RULES[i] = (int(d), cat, why)
DEFAULT = (2, "", "front-end rule, std function or tooling surface")


def classify(idx: int) -> tuple[int, str, str]:
    return RULES.get(idx, DEFAULT)


def case_key(p: Path | str) -> str:
    return str((ROOT / p).resolve().relative_to(ROOT))


def citations() -> dict[str, set[str]]:
    out: dict[str, set[str]] = defaultdict(set)
    pat = re.compile(r"\bR\d{1,4}\b")
    for crate in (ROOT / "crates").iterdir():
        for f in crate.rglob("*"):
            if f.suffix in (".rs", ".yn") and f.is_file():
                for r in pat.findall(f.read_text(errors="ignore")):
                    out[r].add(crate.name)
    return out


def audit(conf_json: Path, diff_json: Path) -> list[dict[str, str]]:
    entries = rows.load()
    owner, _ = rows.allocations(entries)
    cases = conform.discover(ROOT / "conformance")
    verdict = {
        case_key(r["case"]): r["status"] for r in json.loads(conf_json.read_text())["results"]
    }
    diff = json.loads(diff_json.read_text())
    refused = {s.split(" [")[0] for s in diff["refused"]}
    in_diff = {p for p, _ in difftest.programs()}
    cited = citations()
    sec_cases: Counter[str] = Counter(sec for c in cases for sec in set(c.spec))
    by_row: dict[str, list[conform.Case]] = defaultdict(list)
    for c in cases:
        for label in c.rows:
            for r in rows.expand(label):
                by_row[r].append(c)
    out = []
    for rid, idx in sorted(
        (kv for kv in owner.items() if kv[0].startswith("R")), key=lambda kv: int(kv[0][1:])
    ):
        e = entries[idx]
        d, cat, why = classify(idx)
        cs = by_row.get(rid, [])
        states = {verdict.get(case_key(c.path), "skip") for c in cs}
        interp = "none" if not cs else "pass" if states == {"pass"} else "fail"
        progs = [c.path.relative_to(ROOT).as_posix() for c in cs if c.kind == "run"]
        progs = [p for p in progs if p in in_diff]
        native = "n/a" if not progs else "refused" if any(p in refused for p in progs) else "ok"
        crates = cited.get(rid, set())
        secs = {m for m in re.findall(r"\d+(?:\.\d+)*", e["sections"])}
        sec_n = sum(sec_cases[m] for m in secs)
        if native == "ok":
            status = "native"
        elif interp == "pass" or (crates & (FRONT | INTERP)):
            status = "interp" if interp == "pass" else "interp-uncased"
        elif crates & NATIVE:
            status = "native-uncased"
        elif interp == "fail":
            status = "case-failing"
        else:
            status = "none"
        out.append(
            {
                "row": rid,
                "entry": str(idx),
                "topic": e["topic"][:80],
                "sections": e["sections"],
                "cases": str(len(cs)),
                "interp_cases": interp,
                "native_cases": native,
                "cited_in": "+".join(sorted(crates)),
                "section_cases": str(sec_n),
                "status": status,
                "difficulty": str(d),
                "hard": cat,
                "why": why,
            }
        )
    return out


def _share(rows_: list[dict[str, str]]) -> str:
    done = sum(r["status"] in DONE for r in rows_)
    return f"{done}/{len(rows_)}"


def summary(table: list[dict[str, str]]) -> str:
    hard = [r for r in table if r["hard"]]
    core = [r for r in hard if r["hard"] != "research"]
    dark = [r for r in table if r["status"] == "none" and r["section_cases"] == "0"]
    cats = sorted({r["hard"] for r in hard})
    lines = [
        f"rows: {len(table)}; with a case: {sum(r['cases'] != '0' for r in table)}",
        "status: "
        + ", ".join(f"{k} {v}" for k, v in Counter(r["status"] for r in table).most_common()),
        f"hard rows: {_share(hard)} done; excluding research: {_share(core)}",
        f"no evidence, no case on its sections: {len(dark)}; hard {sum(bool(r['hard']) for r in dark)}",
        "hard by category: "
        + ", ".join(f"{c} {_share([r for r in hard if r['hard'] == c])}" for c in cats),
        "by difficulty: "
        + ", ".join(
            f"{d} {_share([r for r in table if r['difficulty'] == str(d)])}" for d in range(1, 6)
        ),
    ]
    return "\n".join(lines)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--conform-json", type=Path, required=True)
    ap.add_argument("--difftest-json", type=Path, required=True)
    ap.add_argument("--csv", type=Path)
    ap.add_argument("--summary", action="store_true")
    a = ap.parse_args()
    table = audit(a.conform_json, a.difftest_json)
    if a.csv:
        with a.csv.open("w", newline="") as f:
            w = csv.DictWriter(f, fieldnames=list(table[0]))
            w.writeheader()
            w.writerows(table)
    if a.summary:
        print(summary(table))
    return 0


if __name__ == "__main__":
    sys.exit(main())
