"""Run the conformance suite (spec 20.1) against one or more executors.

An executor is any command that implements:
    CMD check --format json FILE  -> JSON array of diagnostic records (spec 16.1)
    CMD run FILE                  -> the program's stdout and exit status
Exit status 2 or above from either command is a failure of the tool, never a verdict.
The same suite drives the interpreter and every code generator (spec 22.1, 24.1).
"""

import argparse
import json
import shlex
import subprocess
import sys
from collections import Counter
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Literal, Protocol

SURFACES = {"jian": ".jian", "wen": ".wen", "yong": ".yong"}
KINDS = ("pass", "fail", "run")
COMMENT = {".jian": "#", ".wen": "--", ".yong": "--"}
TOOL_FAILURE = 2

type Status = Literal["pass", "fail", "skip"]


@dataclass(frozen=True, order=True)
class Diag:
    code: str
    line: int
    col: int


@dataclass
class Case:
    kind: str
    path: Path
    expect: list[Diag] = field(default_factory=list)
    stdout: str | None = None
    exit: int = 0


@dataclass(frozen=True)
class Result:
    case: str
    executor: str
    status: Status
    detail: str = ""


class ToolFailure(Exception):
    pass


class Executor(Protocol):
    @property
    def name(self) -> str: ...

    def check(self, path: Path) -> list[dict[str, object]]: ...

    def run(self, path: Path) -> tuple[str, int]: ...


@dataclass(frozen=True)
class CommandExecutor:
    name: str
    argv: list[str]

    def check(self, path: Path) -> list[dict[str, object]]:
        out = subprocess.run(
            [*self.argv, "check", "--format", "json", str(path)], capture_output=True, text=True
        )
        if out.returncode >= TOOL_FAILURE:
            raise ToolFailure(out.stderr.strip())
        records: list[dict[str, object]] = json.loads(out.stdout or "[]")
        return records

    def run(self, path: Path) -> tuple[str, int]:
        out = subprocess.run([*self.argv, "run", str(path)], capture_output=True, text=True)
        if out.returncode == TOOL_FAILURE and out.stderr.startswith("yong:"):
            raise ToolFailure(out.stderr.strip())
        return out.stdout, out.returncode


# SPEC-Q: spec 24.1 does not fix the stdout block layout. Assumed: headers are the leading
# comment lines ("#" in Jian, "--" in Wen and Yong); a "stdout:" block is the following
# comment lines with one space stripped (a bare marker is an empty line) up to the next
# header or non-comment line; several blocks concatenate.
def parse_headers(text: str, mark: str = "#") -> tuple[list[Diag], str | None, int]:
    expect: list[Diag] = []
    stdout: list[str] | None = None
    exit_code = 0
    in_stdout = False
    for line in text.splitlines():
        if not line.startswith(mark):
            break
        body = line[len(mark) :].removeprefix(" ")
        if body.startswith("expect:"):
            in_stdout = False
            code, _, pos = body.removeprefix("expect:").strip().partition(" at ")
            ln, _, col = pos.partition(":")
            expect.append(Diag(code, int(ln), int(col)))
        elif body.startswith("exit:"):
            in_stdout = False
            exit_code = int(body.removeprefix("exit:"))
        elif body.rstrip() == "stdout:":
            in_stdout = True
            stdout = stdout if stdout is not None else []
        elif in_stdout and stdout is not None:
            stdout.append(body)
    return expect, None if stdout is None else "".join(s + "\n" for s in stdout), exit_code


def discover(root: Path) -> list[Case]:
    cases: list[Case] = []
    for surface, suffix in SURFACES.items():
        for kind in KINDS:
            for p in sorted((root / surface / kind).rglob(f"*{suffix}")):
                text = p.read_text(encoding="utf-8")
                expect, stdout, exit_code = parse_headers(text, COMMENT[suffix])
                cases.append(Case(kind, p, expect, stdout, exit_code))
    patch = root / "patch"
    if patch.is_dir():
        cases += [Case("patch", p) for p in sorted(patch.iterdir()) if p.is_dir()]
    return cases


def _diags(records: list[dict[str, object]], errors_only: bool) -> list[Diag]:
    out = []
    for r in records:
        if errors_only and r.get("severity") != "error":
            continue
        span = r["span"]
        assert isinstance(span, dict)
        out.append(Diag(str(r["code"]), int(span["line"]), int(span["col"])))
    return sorted(out)


def judge(case: Case, ex: Executor) -> Result:
    try:
        return _judge(case, ex)
    except ToolFailure as e:
        return Result(str(case.path), ex.name, "fail", f"tool failure: {e}")


def _judge(case: Case, ex: Executor) -> Result:
    name = str(case.path)
    match case.kind:
        case "pass":
            got = _diags(ex.check(case.path), errors_only=True)
            if got:
                return Result(name, ex.name, "fail", f"unexpected errors: {got}")
        case "fail":
            got = _diags(ex.check(case.path), errors_only=False)
            if Counter(got) != Counter(case.expect):
                return Result(name, ex.name, "fail", f"expected {sorted(case.expect)}, got {got}")
        case "run":
            stdout, code = ex.run(case.path)
            if stdout != (case.stdout or "") or code != case.exit:
                return Result(name, ex.name, "fail", f"exit {code}, stdout {stdout!r}")
        case _:
            return Result(name, ex.name, "skip", "patch case format not defined yet")
    return Result(name, ex.name, "pass")


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("--root", type=Path, default=Path("conformance"))
    ap.add_argument("--executor", action="append", default=[], metavar="NAME=CMD")
    ap.add_argument("--json", type=Path, help="write a JSON summary here")
    args = ap.parse_args(argv)

    executors = []
    for spec in args.executor:
        name, _, cmd = spec.partition("=")
        executors.append(CommandExecutor(name, shlex.split(cmd)))

    cases = discover(args.root)
    results = [judge(c, ex) for c in cases for ex in executors]
    counts = Counter(r.status for r in results)
    for r in results:
        if r.status == "fail":
            print(f"FAIL {r.executor} {r.case}: {r.detail}")
    print(f"{len(cases)} cases, {len(executors)} executors: {dict(counts)}")
    if args.json:
        args.json.write_text(
            json.dumps(
                {
                    "cases": len(cases),
                    "executors": [ex.name for ex in executors],
                    "counts": dict(counts),
                    "results": [asdict(r) for r in results],
                },
                indent=2,
            )
        )
    return 1 if counts["fail"] else 0


if __name__ == "__main__":
    sys.exit(main())
