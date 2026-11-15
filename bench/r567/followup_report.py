# ruff: noqa: E501
"""Follow-up analysis for the OpenRouter sweep: primer v1 against v2, the hard suite, impossible
variants and the Python arm. Reads run directories written by run.py (EXP/MODEL/TAG/TASK.ARM.json).

usage: uv run python bench/r567/followup_report.py RUNS_DIR BASELINE_ATTEMPTS_JSONL IDS_JSON OUT_JSON
"""

import collections
import glob
import json
import pathlib
import sys


def ok(h: dict | None) -> bool:
    return bool(h and h["ok"])


def load(pat: str) -> list[dict]:
    out = []
    for f in glob.glob(pat):
        r = json.loads(pathlib.Path(f).read_text())
        r["_model"] = r["model"].removeprefix("openrouter:")
        out.append(r)
    return out


def main() -> None:
    runs, base, ids, out = sys.argv[1:5]
    core = set(json.loads(pathlib.Path(ids).read_text())["core"])
    v1: dict[str, dict] = collections.defaultdict(dict)
    for line in pathlib.Path(base).read_text().splitlines():
        r = json.loads(line)
        if not r["impossible"]:
            v1[r["model"]][r["task"]] = r
    primer: dict[str, dict] = {}
    for ver in ("v2", "v3"):
        new: dict[str, dict] = collections.defaultdict(dict)
        for r in load(f"{runs}/primer{ver}-*/*/*/*.yag.json"):
            if not r["impossible"]:
                new[r["_model"]][r["task"]] = r
        for m in sorted(new):
            common = [t for t in new[m] if t in v1[m]]
            primer[f"{ver} {m}"] = {
                "tasks": len(common),
                "v1_pass": sum(v1[m][t]["pass"] for t in common),
                f"{ver}_pass": sum(ok(new[m][t]["hidden_final"]) for t in common),
                "v1_pass1": sum(v1[m][t]["pass1"] for t in common),
                f"{ver}_pass1": sum(ok(new[m][t]["hidden_first"]) for t in common),
            }
    cats: dict[str, list[int]] = collections.defaultdict(lambda: [0, 0])
    tasks: dict[str, list[int]] = collections.defaultdict(lambda: [0, 0])
    for exp in ("hardv2", "hardv2b", "hardv3"):
        for r in load(f"{runs}/{exp}/*/*/*.yag.json"):
            for d, k in ((cats, f"{exp} {r['category']}"), (tasks, f"{exp} {r['task']}")):
                d[k][0] += ok(r["hidden_final"])
                d[k][1] += 1
    imp: dict[str, list[int]] = collections.defaultdict(lambda: [0, 0])
    for r in load(f"{runs}/impossible/*/*/*.yag.json"):
        imp[r["_model"]][0] += ok(r["hidden_final"])
        imp[r["_model"]][1] += 1
    py = {}
    pyr: dict[str, dict] = collections.defaultdict(dict)
    for r in load(f"{runs}/pyarm/*/*/*.py.json"):
        pyr[r["_model"]][r["task"]] = r
    for m, d in sorted(pyr.items()):
        t = [x for x in d if x in core]
        both = [x for x in t if x in v1[m] and ok(d[x]["hidden_final"]) and v1[m][x]["pass"]]
        tok = lambda r: r["tokens"]["spec"]["out"] + r["tokens"]["body"]["out"]  # noqa: E731
        py[m] = {
            "core_tasks": len(t),
            "py_pass": sum(ok(d[x]["hidden_final"]) for x in t),
            "yag_pass": sum(v1[m][x]["pass"] for x in t if x in v1[m]),
            "joint": len(both),
            "py_out_tokens": sum(tok(d[x]) for x in both),
            "yag_out_tokens": sum(v1[m][x]["out"] for x in both),
        }
    summary = {"primer": primer, "hard_categories": cats, "hard_tasks": tasks,
               "impossible": imp, "python_arm": py}  # fmt: skip
    pathlib.Path(out).write_text(json.dumps(summary, indent=1))


if __name__ == "__main__":
    main()
