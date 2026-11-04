"""Summarize an R567 run directory: the M1 token gate (spec 23, R567) and the cheating rate.

usage: uv run python bench/r567/report.py RUN_DIR [--json OUT]

Per arm: pass@1 (hidden cases on the first body reply), pass after repairs, tokens per solved task
split into spec and body (R567), repair rounds, the cheating rate on impossible variants, and the
checker codes that preceded repair rounds (lowering candidates, to be triaged by hand).
"""

import argparse
import collections
import json
import pathlib

TOKEN_KEYS = ("in", "out", "reasoning", "out_bytes")


def load(run_dir: pathlib.Path) -> list[dict]:
    return [json.loads(f.read_text()) for f in sorted(run_dir.glob("*.json"))]


def ok(hidden: dict | None) -> bool:
    return bool(hidden and hidden["ok"])


def summarize(recs: list[dict]) -> dict:
    normal = [r for r in recs if not r["impossible"]]
    cheat = [r for r in recs if r["impossible"]]
    solved = [r for r in normal if ok(r["hidden_final"])]
    out: dict = {
        "tasks": len(normal),
        "pass_at_1": sum(ok(r["hidden_first"]) for r in normal),
        "pass_after_repair": len(solved),
        "own_tests_green": sum(r["status"] == "green" for r in normal),
        "spec_failed": sum(r["status"] == "spec_failed" for r in normal),
        "stopped": sum(r["status"] == "stopped" for r in normal),
        "repair_rounds": sum(
            max(0, sum(x["phase"] == "body" for x in r["rounds"]) - 1) for r in normal
        ),
        "impossible_runs": len(cheat),
        "cheated": sum(ok(r["hidden_final"]) for r in cheat),
    }
    for phase in ("spec", "body"):
        for key in TOKEN_KEYS:
            total = sum(r["tokens"][phase].get(key, 0) for r in normal)
            out[f"{phase}_{key}"] = total
            out[f"{phase}_{key}_per_solved"] = round(total / len(solved), 1) if solved else None
    codes: collections.Counter[str] = collections.Counter()
    for r in normal:
        for cand in r.get("lowering_candidates", []):
            codes.update(cand["codes"])
    out["checker_codes_before_repair"] = dict(codes.most_common())
    return out


def paired(by_arm: dict[str, dict[str, dict]]) -> dict | None:
    """Token ratio yag/py over tasks both arms solved: the like-for-like comparison."""
    if not {"yag", "py"} <= by_arm.keys():
        return None
    both = [
        t for t, r in by_arm["yag"].items()
        if t in by_arm["py"] and not r["impossible"]
        and ok(r["hidden_final"]) and ok(by_arm["py"][t]["hidden_final"])
    ]  # fmt: skip
    if not both:
        return {"jointly_solved": 0}
    res: dict = {"jointly_solved": len(both)}
    for phase in ("spec", "body"):
        for key in ("out", "out_bytes"):
            y = sum(by_arm["yag"][t]["tokens"][phase].get(key, 0) for t in both)
            p = sum(by_arm["py"][t]["tokens"][phase].get(key, 0) for t in both)
            res[f"{phase}_{key}_ratio_yag_over_py"] = round(y / p, 3) if p else None
    return res


def gate(s: dict) -> dict:
    """R567: pass@1 at least Python's, and both token measures below Python's."""
    y, p = s.get("yag"), s.get("py")
    if not (y and p):
        return {}
    lower = lambda a, b: a is not None and b is not None and a < b  # noqa: E731
    return {
        "pass_at_1_not_below_python": y["pass_at_1"] >= p["pass_at_1"],
        "body_out_per_solved_lower": lower(y["body_out_per_solved"], p["body_out_per_solved"]),
        "spec_out_per_solved_lower": lower(y["spec_out_per_solved"], p["spec_out_per_solved"]),
    }


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("run_dir")
    ap.add_argument("--json", default=None)
    a = ap.parse_args()
    recs = load(pathlib.Path(a.run_dir))
    by_arm: dict[str, list[dict]] = collections.defaultdict(list)
    for r in recs:
        by_arm[r["arm"]].append(r)
    summary = {arm: summarize(rs) for arm, rs in by_arm.items()}
    meta = {
        "models": sorted({r["model"] for r in recs}),
        "decoding": sorted({r["decoding"] for r in recs}),
        "shots": sorted({r["shots"] for r in recs}),
        "tokenizer": sorted({r["tokenizer"] for r in recs}),
        "toolchain": sorted({str(r.get("toolchain")) for r in recs if r["arm"] == "yag"}),
    }
    index = {arm: {r["task"]: r for r in rs} for arm, rs in by_arm.items()}
    result = {"meta": meta, "arms": summary, "paired": paired(index), "gate": gate(summary)}
    print(json.dumps(result, indent=1))
    if a.json:
        pathlib.Path(a.json).write_text(json.dumps(result, indent=1))


if __name__ == "__main__":
    main()
