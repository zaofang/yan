"""Run the conformance suite (spec 20.1) against one or more executors.

An executor is any command that implements:
    CMD check --format json FILE  -> JSON array of diagnostic records (spec 16.1)
    CMD run FILE                  -> the program's stdout and exit status
so the same suite drives the Python reference and, later, the Rust port.
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

KINDS = ("pass", "fail", "run", "patch")
SUFFIXES = (".fan", ".wen", ".ben")

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
        records: list[dict[str, object]] = json.loads(out.stdout or "[]")
        return records

    def run(self, path: Path) -> tuple[str, int]:
        out = subprocess.run([*self.argv, "run", str(path)], capture_output=True, text=True)
        return out.stdout, out.returncode


# SPEC-Q: spec 20.1 does not fix the header layout, and .ben has no comment syntax yet
# (proposed SQ2). Assumed: headers are leading "#" lines; a "# stdout:" block is the
# following "# "-prefixed lines ("#" alone is an empty line) up to the next header or
# non-comment line; several blocks concatenate.
def parse_headers(text: str) -> tuple[list[Diag], str | None, int]:
    expect: list[Diag] = []
    stdout: list[str] | None = None
    exit_code = 0
    in_stdout = False
    for line in text.splitlines():
        if not line.startswith("#"):
            break
        body = line[1:].removeprefix(" ")
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
    for kind in KINDS:
        base = root / kind
        if not base.is_dir():
            continue
        if kind == "patch":
            cases += [Case(kind, p) for p in sorted(base.iterdir()) if p.is_dir()]
            continue
        for p in sorted(base.rglob("*")):
            if p.suffix in SUFFIXES:
                expect, stdout, exit_code = parse_headers(p.read_text(encoding="utf-8"))
                cases.append(Case(kind, p, expect, stdout, exit_code))
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
