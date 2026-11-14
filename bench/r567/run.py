"""R567 harness: Yan Agent against typed Python on MultiPL-E-style HumanEval and MBPP tasks.

Both arms get the same task text and the same two phases (spec 21.6):
  spec  the header, contracts and examples (Yan Agent), or the header and pytest cases (Python)
  body  the bodies, then repair rounds from the checker's report
The Yan Agent arm runs through `yan agent`; the Python arm runs mypy --strict and pytest here.
Hidden cases (tasks.py) are appended to the model's file afterwards, and the same cases run in
both arms. pass@1 is the hidden verdict on the first body reply, before any repair round.

usage: uv run python bench/r567/run.py --yan target/release/yan --model claude:claude-sonnet-5-5
"""

import argparse
import json
import os
import pathlib
import re
import shlex
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor

import model as models
import tasks as taskset

HERE = pathlib.Path(__file__).resolve().parent
SCHEMA = 1
SPEC_REPAIRS, BODY_REPAIRS, MAX_TOKENS = 2, 3, 16000
TOKENIZER = "provider"  # usage counts are the provider's own; see README for the caveat

PY_SYSTEM = "You write typed Python 3.12. Code must pass mypy --strict; tests use pytest."
PY_SPEC_ASK = (
    "Task: {prompt}\n\nPhase 1, the header and tests. Write the declarations the task needs and "
    "each function's annotated def line and docstring with `...` as the body, then pytest test "
    "functions for it. Reply with one ```python code block only."
)
PY_BODY_ASK = (
    "Phase 2, the bodies. Keep every line you wrote unchanged and replace each `...` body with "
    "the implementation. Reply with the whole file in one ```python code block only."
)
PY_REPAIR_ASK = (
    "{tool} reported:\n{report}\n\n"
    "Reply with the whole corrected file in one ```python code block only."
)


def extract(text: str) -> str:
    m = re.search(r"```[a-zA-Z]*\n(.*?)```", text, re.S)
    return (m.group(1) if m else text).rstrip() + "\n"


def sh(cmd: list[str], cwd: str, timeout: int = 120) -> subprocess.CompletedProcess:
    try:
        return subprocess.run(
            cmd, cwd=cwd, capture_output=True, text=True, timeout=timeout, check=False
        )
    except subprocess.TimeoutExpired as e:
        return subprocess.CompletedProcess(cmd, 124, "", f"timeout after {e.timeout}s")


def out_bytes(text: str) -> int:
    return len(text.encode())


# Python arm


def py_shots() -> str:
    parts = [f"```python\n{p.read_text()}```" for p in sorted((HERE / "shots").glob("s*.py"))]
    return "Examples:\n\n" + "\n\n".join(parts)


def fence_python_grammar(path: str) -> str:
    """A copy of a Python GBNF whose root is one ```python block, so a reply is exactly that."""
    text = pathlib.Path(path).read_text()
    text = re.sub(r"^root\s*::=", "pyroot ::=", text, count=1, flags=re.M)
    out = pathlib.Path(tempfile.mkdtemp(prefix="r567-grammar-"), "py.gbnf")
    out.write_text(f'root ::= "```python\\n" pyroot "```"\n{text}')
    return str(out)


def py_check(src: str, spec_phase: bool) -> dict:
    with tempfile.TemporaryDirectory() as d:
        pathlib.Path(d, "test_task.py").write_text(src)
        flags = ["--disable-error-code", "empty-body"] if spec_phase else []
        m = sh(
            [sys.executable, "-m", "mypy", "--strict", "--no-incremental", *flags, "test_task.py"],
            d,
        )
        report = [] if m.returncode == 0 else [m.stdout.strip()]
        if not spec_phase:
            t = sh([sys.executable, "-m", "pytest", "-q", "-x", "test_task.py"], d)
            if t.returncode:
                report.append("\n".join(t.stdout.strip().splitlines()[-25:]))
        return {"ok": not report, "report": "\n\n".join(report)}


def py_hidden(src: str, task: taskset.Task) -> dict:
    with tempfile.TemporaryDirectory() as d:
        pathlib.Path(d, "test_hidden.py").write_text(src + taskset.python_tests(task))
        p = sh([sys.executable, "-m", "pytest", "-q", "--tb=no", "-rf", "test_hidden.py"], d, 300)
    failed = set(re.findall(r"FAILED test_hidden\.py::test_hidden_(\d+)", p.stdout))
    total = len(task.cases)
    ran = re.search(r"(\d+) passed", p.stdout)
    passed = int(ran.group(1)) if ran else 0
    passed = min(passed, total - len(failed))
    return {"passed": passed, "total": total, "ok": passed == total and not failed}


def py_phase(chat, system, msgs, ask, spec_phase, repairs) -> list[dict]:
    rounds = []
    msgs.append({"role": "user", "content": ask})
    for i in range(repairs + 1):
        r = chat(system, msgs, MAX_TOKENS)
        msgs.append({"role": "assistant", "content": r["text"]})
        src = extract(r["text"])
        res = py_check(src, spec_phase)
        rounds.append(
            {
                "phase": "spec" if spec_phase else "body",
                "round": i,
                "usage": r["usage"],
                "out_bytes": out_bytes(r["text"]),
                "ok": res["ok"],
                "report": res["report"],
                "src": src,
            }
        )
        if res["ok"] or i == repairs:
            break
        msgs.append(
            {"role": "user",
             "content": PY_REPAIR_ASK.format(tool="mypy and pytest", report=res["report"])}
        )  # fmt: skip
    return rounds


def totals(rounds: list[dict], phase: str) -> dict:
    t = {"in": 0, "out": 0, "reasoning": 0, "out_bytes": 0}
    for r in rounds:
        if r["phase"] == phase:
            for k in ("in", "out", "reasoning"):
                t[k] += r["usage"][k]
            t["out_bytes"] += r.get("out_bytes", 0)
    return t


def run_py(task: taskset.Task, a: argparse.Namespace) -> dict:
    system = PY_SYSTEM + ("\n\n" + py_shots() if a.shots else "")
    chat = models.make(a.model, a.grammar_py_fenced)
    msgs: list[dict] = []
    rounds = py_phase(
        chat, system, msgs, PY_SPEC_ASK.format(prompt=task.prompt), True, SPEC_REPAIRS
    )
    if rounds[-1]["ok"]:
        rounds += py_phase(chat, system, msgs, PY_BODY_ASK, False, BODY_REPAIRS)
    body = [r for r in rounds if r["phase"] == "body"]
    final = rounds[-1]["src"]
    green = bool(body) and body[-1]["ok"]
    return {
        "status": "green" if green else ("body_failed" if body else "spec_failed"),
        "rounds_to_green": len(body) if green else None,
        "tokens": {"spec": totals(rounds, "spec"), "body": totals(rounds, "body")},
        "first_body": body[0]["src"] if body else None,
        "final": final if body else None,
        "rounds": [{k: v for k, v in r.items() if k != "src"} for r in rounds],
    }


# Yan Agent arm


def yan_hidden(src: str, task: taskset.Task, yan: str) -> dict:
    total = len(task.cases)
    with tempfile.TemporaryDirectory() as d:
        pathlib.Path(d, "task.yag").write_text(src + taskset.yan_tests(task))
        p = sh([yan, "test", "--no-cache"], d, 300)
    text = p.stdout + p.stderr
    m = re.search(r"\d+ of (\d+) tests passed", text)
    if m is None or int(m.group(1)) < total:  # a checker error leaves the tests unrun
        return {"passed": 0, "total": total, "ok": False, "error": text.strip()[:300]}
    failed = set(re.findall(r"FAIL \S*test_hidden_(\d+)", text))
    passed = total - len(failed)
    return {"passed": passed, "total": total, "ok": not failed}


def phase_replies(rec: dict) -> dict[str, str]:
    """The first reply of the body phase, found by the Phase 2 prompt in the transcript."""
    msgs = rec.get("messages") or []
    for i, m in enumerate(msgs):
        if m["role"] == "user" and "Phase 2" in m["content"] and i + 1 < len(msgs):
            return {"first_body": extract(msgs[i + 1]["content"])}
    return {}


def run_yan(task: taskset.Task, a: argparse.Namespace) -> dict:
    with tempfile.TemporaryDirectory() as d:
        t = pathlib.Path(d, "task.json")
        t.write_text(json.dumps({"id": task.id.replace("/", "_"), "prompt": task.prompt}))
        adapter = f"cmd:{shlex.quote(sys.executable)} {HERE / 'model.py'} {a.model}"
        cmd = [
            a.yan, "agent", str(t), "--dir", str(pathlib.Path(d, "work")), "--model", adapter,
            "--primer", str(HERE / "primer.md"), "--spec-repairs", str(SPEC_REPAIRS),
            "--body-repairs", str(BODY_REPAIRS), "--max-tokens", str(MAX_TOKENS),
        ]  # fmt: skip
        if a.shots:
            cmd += ["--shots", str(HERE / "shots")]
        env = {**os.environ, **({"R567_GRAMMAR": a.grammar_yag} if a.decoding == "grammar" else {})}
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=3600, check=False, env=env)
    if p.returncode == 2 or not p.stdout.strip():
        raise RuntimeError(p.stderr.strip()[:300])
    rec = json.loads(p.stdout)
    msgs = [m["content"] for m in rec.get("messages", []) if m["role"] == "assistant"]
    for r, text in zip(rec["rounds"], msgs, strict=False):
        r["out_bytes"] = out_bytes(text)
    for ph in ("spec", "body"):
        rec["tokens"][ph]["out_bytes"] = sum(
            r.get("out_bytes", 0) for r in rec["rounds"] if r["phase"] == ph
        )
    rec.update(phase_replies(rec))
    rec["final"] = extract(msgs[-1]) if rec["status"] != "green" and msgs else rec["final"]
    rec["lowering_candidates"] = lowering_candidates(rec)
    for k in ("system", "messages", "spec"):
        rec.pop(k, None)
    return rec


def lowering_candidates(rec: dict) -> list[dict]:
    """Rounds that failed on checker diagnostics, each followed by a repair round. They are listed
    for triage (surface-semantics study, recommendation 1): whether a code reflects a
    Python-semantics expectation is a human call, so this tags candidates and decides nothing."""
    return [
        {
            "phase": r["phase"],
            "round": r["round"],
            "codes": codes,
            "report": str(r.get("report"))[:400],
        }
        for r in rec["rounds"]
        if not r.get("ok")
        and (codes := sorted({c for t in r.get("tools", []) for c in t["codes"]}))
    ]


# Run


def run_one(task: taskset.Task, arm: str, a: argparse.Namespace) -> dict:
    rec = run_yan(task, a) if arm == "yag" else run_py(task, a)
    hid = (lambda s: yan_hidden(s, task, a.yan)) if arm == "yag" else (lambda s: py_hidden(s, task))
    first = rec.pop("first_body", None)
    rec.update(
        schema=SCHEMA, task=task.id, suite=task.suite, arm=arm, model=a.model, shots=a.shots,
        decoding=a.decoding, tokenizer=TOKENIZER, impossible=task.impossible,
        cases=len(task.cases),
        hidden_first=hid(first) if first else None,
        hidden_final=hid(rec["final"]) if rec.get("final") else None,
    )  # fmt: skip
    return rec


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("--yan", required=True, help="path to the yan binary")
    ap.add_argument(
        "--model", required=True, help="claude:M, anthropic:M, compat:URL#M or oracle:DIR"
    )
    ap.add_argument("--suite", default="humaneval,mbpp")
    ap.add_argument("--limit", type=int, default=None, help="tasks to run, after a seeded shuffle")
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument(
        "--ids", default="", help="comma-separated task ids, e.g. HumanEval/11,mbpp/118"
    )
    ap.add_argument("--only", default="", help="comma-separated task function names")
    ap.add_argument("--shots", type=int, choices=[0, 5], default=5)
    ap.add_argument("--arms", default="yag,py")
    ap.add_argument("--decoding", choices=["free", "grammar"], default="free")
    ap.add_argument("--grammar-yag", default=str(HERE / "out" / "yag.gbnf"))
    ap.add_argument("--grammar-py", default=None, help="GBNF for Python; its root is fenced here")
    ap.add_argument(
        "--impossible",
        type=float,
        default=0.0,
        help="share of tasks also run as impossible variants",
    )
    ap.add_argument("--workers", type=int, default=4)
    ap.add_argument("--out", default=str(HERE / "runs"))
    a = ap.parse_args()
    a.yan = str(pathlib.Path(a.yan).resolve())
    if a.model.startswith("oracle:"):  # the adapter runs from another directory
        d, _, flag = a.model[7:].partition("?")
        a.model = f"oracle:{pathlib.Path(d).resolve()}" + (f"?{flag}" if flag else "")
    a.grammar_py_fenced = fence_python_grammar(a.grammar_py) if a.grammar_py else None
    if a.decoding == "grammar" and "py" in a.arms.split(",") and not a.grammar_py:
        sys.exit("--decoding grammar with the py arm needs --grammar-py")
    if a.decoding == "grammar" and not a.model.startswith("compat:"):
        sys.exit(
            "constrained decoding needs a server that takes a grammar: use --model compat:URL#MODEL"
        )

    ts = taskset.select(a.suite.split(","), a.limit, a.seed)
    if a.only:
        ts = [t for t in ts if t.name in a.only.split(",")]
    if a.ids:
        ts = [t for t in ts if t.id in a.ids.split(",")]
    n_imp = round(len(ts) * a.impossible)
    ts += [taskset.impossible(t) for t in ts[:n_imp]]
    tag = f"{a.model.split(':')[0]}-{a.decoding}-{a.shots}shot"
    jobs = []
    for t in ts:
        for arm in a.arms.split(","):
            f = pathlib.Path(a.out, tag, f"{t.id.replace('/', '_')}.{arm}.json")
            if not f.exists():
                jobs.append((t, arm, f))

    def go(job):
        t, arm, f = job
        try:
            rec = run_one(t, arm, a)
        except Exception as e:  # one failed task must not stop the run
            print(f"FAIL {f.name}: {e!r}", file=sys.stderr)
            return
        f.parent.mkdir(parents=True, exist_ok=True)
        f.write_text(json.dumps(rec, indent=1))
        ok = (rec["hidden_first"] or {}).get("ok")
        print(f"done {f.name} status={rec['status']} pass@1={ok}", flush=True)

    print(f"{len(jobs)} runs", flush=True)
    with ThreadPoolExecutor(a.workers) as ex:
        list(ex.map(go, jobs))


if __name__ == "__main__":
    main()
