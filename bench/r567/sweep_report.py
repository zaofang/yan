# ruff: noqa: E501
"""Statistics for an OpenRouter sweep: run records plus the cost ledger of model.py's openrouter vendor.

usage: uv run python bench/r567/sweep_report.py RUNS_DIR LEDGER IDS_JSON OUT_PREFIX

RUNS_DIR holds SAMPLE/MODEL_SLUG/TAG/TASK.yag.json. Writes OUT_PREFIX.jsonl (one row per attempt, no
sources) and OUT_PREFIX-summary.json, and prints the per-model table.
"""

import collections
import json
import math
import pathlib
import statistics
import sys

import tasks as taskset


def wilson(k: int, n: int, z: float = 1.96) -> tuple[float, float]:
    if n == 0:
        return (0.0, 0.0)
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return (round(c - h, 3), round(c + h, 3))


def category(t: taskset.Task) -> list[str]:
    cats = [t.suite]
    ret = t.ret.split("[")[0].split(" ")[0]
    cats.append(f"ret:{ret}")
    sig = t.ret + " ".join(t.param_types)
    for key, name in (("float", "float"), ("dict", "dict"), ("tuple", "tuple"), ("None", "optional"),
                      ("str", "str"), ("bool", "bool")):  # fmt: skip
        if key in sig:
            cats.append(f"has:{name}")
    if len(t.params) >= 3:
        cats.append("params>=3")
    return cats


def attempt(rec: dict, sample: str, model: str, hard: set[str], cats: dict[str, list[str]]) -> dict:
    codes = [c for r in rec["rounds"] for tl in r.get("tools", []) for c in tl["codes"]]
    first_codes = [c for r in rec["rounds"][:1] for tl in r.get("tools", []) for c in tl["codes"]]
    tok = rec["tokens"]
    return {
        "model": model, "sample": sample, "task": rec["task"], "hard": rec["task"] in hard,
        "cats": cats.get(rec["task"], []), "impossible": rec["impossible"], "status": rec["status"],
        "pass1": bool(rec["hidden_first"] and rec["hidden_first"]["ok"]),
        "pass": bool(rec["hidden_final"] and rec["hidden_final"]["ok"]),
        "rounds": len(rec["rounds"]),
        "e_codes": sorted(c for c in codes if c[0] in "EW"), "n_codes": sorted(c for c in codes if c[0] == "N"),
        "first_n": len([c for c in first_codes if c[0] == "N"]),
        "first_e": sorted(c for c in first_codes if c[0] in "EW"),
        "in": tok["spec"]["in"] + tok["body"]["in"], "out": tok["spec"]["out"] + tok["body"]["out"],
        "reasoning": tok["spec"]["reasoning"] + tok["body"]["reasoning"],
    }  # fmt: skip


def main() -> None:
    runs, ledger, ids, out = sys.argv[1:5]
    idset = json.loads(pathlib.Path(ids).read_text())
    hard = set(idset["hard"])
    cats = {t.id: category(t) for t in taskset.select(["humaneval", "mbpp"], None, 1)}
    rows = []
    for f in sorted(pathlib.Path(runs).glob("*/*/*/*.yag.json")):
        sample = f.parts[-4]
        rec = json.loads(f.read_text())
        model = rec["model"].removeprefix("openrouter:")
        rows.append(attempt(rec, sample, model, hard, cats))
    cost: dict[str, float] = collections.defaultdict(float)
    secs: dict[str, list[float]] = collections.defaultdict(list)
    for line in pathlib.Path(ledger).read_text().splitlines():
        r = json.loads(line)
        cost[r["model"]] += r["cost"]
        secs[r["model"]].append(r["secs"])
    pathlib.Path(out + ".jsonl").write_text("".join(json.dumps(r) + "\n" for r in rows))
    summary = {}
    for m in sorted({r["model"] for r in rows}):
        rs = [r for r in rows if r["model"] == m and not r["impossible"]]
        n, k = len(rs), sum(r["pass"] for r in rs)
        k1 = sum(r["pass1"] for r in rs)
        hs = [r for r in rs if r["hard"]]
        es = [r for r in rs if not r["hard"]]
        percat: dict[str, list[int]] = collections.defaultdict(lambda: [0, 0])
        for r in rs:
            for c in r["cats"]:
                percat[c][0] += r["pass"]
                percat[c][1] += 1
        ecodes = collections.Counter(c for r in rs for c in r["e_codes"])
        imp = [r for r in rows if r["model"] == m and r["impossible"]]
        summary[m] = {
            "attempts": n, "pass": k, "pass_rate": round(k / n, 3), "pass_ci": wilson(k, n),
            "pass1_rate": round(k1 / n, 3), "pass1_ci": wilson(k1, n),
            "hard": [sum(r["pass"] for r in hs), len(hs)], "easy": [sum(r["pass"] for r in es), len(es)],
            "spec_failed": sum(r["status"] == "spec_failed" for r in rs),
            "mean_rounds": round(statistics.mean(r["rounds"] for r in rs), 2),
            "first_reply_n_notes": round(statistics.mean(r["first_n"] for r in rs), 2),
            "first_reply_e_rate": round(sum(bool(r["first_e"]) for r in rs) / n, 3),
            "tokens_in": sum(r["in"] for r in rs), "tokens_out": sum(r["out"] for r in rs),
            "tokens_reasoning": sum(r["reasoning"] for r in rs),
            "cost_total": round(cost[m], 4),
            "cost_per_pass": round(cost[m] / k, 4) if k else None,
            "latency_median_s": round(statistics.median(secs[m]), 2) if secs[m] else None,
            "cats": {c: v for c, v in sorted(percat.items())},
            "e_codes": dict(ecodes.most_common(12)),
            "impossible": [sum(r["pass"] for r in imp), len(imp)],
        }  # fmt: skip
    pathlib.Path(out + "-summary.json").write_text(json.dumps(summary, indent=1))
    print(
        f"{'model':46} {'n':>4} {'pass':>5} {'CI':>13} {'p@1':>5} {'hard':>6} {'$':>7} {'$/pass':>7} {'lat':>5}"
    )
    for m, s in sorted(summary.items(), key=lambda x: -x[1]["pass_rate"]):
        print(f"{m:46} {s['attempts']:4} {s['pass_rate']:5.2f} {str(s['pass_ci']):>13} "
              f"{s['pass1_rate']:5.2f} {s['hard'][0]}/{s['hard'][1]:<3} {s['cost_total']:7.3f} "
              f"{s['cost_per_pass'] or 0:7.4f} {s['latency_median_s']}")  # fmt: skip


if __name__ == "__main__":
    main()
