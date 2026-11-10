"""Run the conformance suite (spec 24) against one or more executors.

An executor is any command that implements:
    CMD check --format json [FLAGS] FILE  -> JSON array of diagnostic records (spec 18.2)
    CMD run [FLAGS] FILE                  -> the program's stdout and exit status
    CMD run --ub [FLAGS] FILE             -> exit status; a UB report names "ub(N)" on stderr
    CMD patch --format json [FLAGS] PATCH -> applies PATCH in its directory; JSON records on refusal
    CMD semver --format json --base OLD NEW -> JSON object with "bump"
    CMD ARGS... --format json             -> run in a pkg/ case's copied root; JSON records
    CMD fmt FILE                          -> rewrites FILE in place; exit 1 if it does not load
    CMD trust; CMD mcp package [FLAGS]    -> run in an mcp/ case's copied root; JSON-RPC lines
                                             on stdin and stdout, or a refusal code on stderr
Exit status 2 or above from either command is a failure of the tool, never a verdict.
The same suite drives the interpreter and every code generator (spec 22.1, 24.1).

With --laws, the runner instead checks the round-trip laws of 16.5 on every case (R564), using
    CMD hash --format json [--reverse-items] FILE -> JSON array of {"key", "hash", "identity"}
    CMD view --as yag FILE                        -> the module's Yan Agent projection
    CMD fmt --check FILE                          -> exit 1 if FILE is not canonical
Exit status 1 from either means the module does not load or is outside the Yan Agent subset; the
law is then skipped for that case.

The runner also writes the suite's manifest (--manifest) and coverage report (--coverage)
from the case headers and conformance/catalogue.toml.
"""

import argparse
import json
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import tomllib
from collections import Counter, defaultdict
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Literal, Protocol

SURFACES = ("yag", "yn", "yir")
SURFACE_KINDS = ("pass", "fail", "run")
# Top-level directories of 24.1 and the kind each one is judged as. Kinds without a defined
# case format yet are discovered and skipped.
TOP_KINDS = {
    "golden": "run",
    "solver": "fail",
    "ub": "ub",
    "semver": "semver",
    "patch": "patch",
    "pkg": "pkg",
    "fmt": "fmt",
    "mcp": "mcp",
    "roundtrip": "roundtrip",
    "sim": "sim",
}
COMMENT = {".yag": "#", ".yn": "--", ".yir": "--"}
DEFAULT_MILESTONE = {".yir": "M0", ".yag": "M1", ".yn": "M3"}
MILESTONES = ("P0", "M0", "M1", "M1b", "M2a", "M2b", "M3", "RC", "M4")
TOOL_FAILURE = 2
HEADERS = frozenset(
    {"expect", "exit", "stdout", "spec", "rows", "flags", "milestone", "note", "ub"}
)

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
    spec: list[str] = field(default_factory=list)
    rows: list[str] = field(default_factory=list)
    flags: list[str] = field(default_factory=list)
    milestone: str = "M0"
    ub: str | None = None
    meta: dict[str, object] = field(default_factory=dict)


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

    def check(self, path: Path, flags: list[str]) -> list[dict[str, object]]: ...

    def run(self, path: Path, flags: list[str]) -> tuple[str, int]: ...

    def run_ub(self, path: Path, flags: list[str]) -> tuple[str, int]: ...

    def patch(
        self, workdir: Path, patch: Path, flags: list[str]
    ) -> tuple[list[dict[str, object]], int]: ...

    def semver(self, old: Path, new: Path) -> str: ...

    def tool(self, workdir: Path, args: list[str]) -> tuple[list[dict[str, object]], int]: ...

    def fmt(self, path: Path) -> int: ...

    def mcp(self, root: Path, flags: list[str], session: str) -> tuple[str, str, int]: ...


@dataclass(frozen=True)
class CommandExecutor:
    name: str
    argv: list[str]

    def check(self, path: Path, flags: list[str]) -> list[dict[str, object]]:
        out = subprocess.run(
            [*self.argv, "check", "--format", "json", *flags, str(path)],
            capture_output=True,
            text=True,
        )
        if out.returncode >= TOOL_FAILURE:
            raise ToolFailure(out.stderr.strip())
        records: list[dict[str, object]] = json.loads(out.stdout or "[]")
        return records

    def run(self, path: Path, flags: list[str]) -> tuple[str, int]:
        out = subprocess.run([*self.argv, "run", *flags, str(path)], capture_output=True, text=True)
        if out.returncode == TOOL_FAILURE and out.stderr.startswith("yan:"):
            raise ToolFailure(out.stderr.strip())
        return out.stdout, out.returncode

    def run_ub(self, path: Path, flags: list[str]) -> tuple[str, int]:
        out = subprocess.run(
            [*self.argv, "run", "--ub", *flags, str(path)], capture_output=True, text=True
        )
        if out.returncode == TOOL_FAILURE and out.stderr.startswith("yan:"):
            raise ToolFailure(out.stderr.strip())
        return out.stderr, out.returncode

    def patch(
        self, workdir: Path, patch: Path, flags: list[str]
    ) -> tuple[list[dict[str, object]], int]:
        out = subprocess.run(
            [*self.argv, "patch", "--format", "json", *flags, patch.name],
            capture_output=True,
            text=True,
            cwd=workdir,
        )
        if out.returncode >= TOOL_FAILURE:
            raise ToolFailure(out.stderr.strip())
        records: list[dict[str, object]] = json.loads(out.stdout or "[]")
        return records, out.returncode

    def semver(self, old: Path, new: Path) -> str:
        out = subprocess.run(
            [*self.argv, "semver", "--format", "json", "--base", str(old), str(new)],
            capture_output=True,
            text=True,
        )
        if out.returncode >= TOOL_FAILURE:
            raise ToolFailure(out.stderr.strip())
        return str(json.loads(out.stdout)["bump"])

    def tool(self, workdir: Path, args: list[str]) -> tuple[list[dict[str, object]], int]:
        out = subprocess.run(
            [*self.argv, *args, "--format", "json"], capture_output=True, text=True, cwd=workdir
        )
        if out.returncode >= TOOL_FAILURE:
            raise ToolFailure(out.stderr.strip())
        records: list[dict[str, object]] = json.loads(out.stdout or "[]")
        return records, out.returncode

    def fmt(self, path: Path) -> int:
        out = subprocess.run([*self.argv, "fmt", str(path)], capture_output=True, text=True)
        if out.returncode >= TOOL_FAILURE:
            raise ToolFailure(out.stderr.strip())
        return out.returncode

    def mcp(self, root: Path, flags: list[str], session: str) -> tuple[str, str, int]:
        # Serving runs the package's code, so the toolchain refuses an untrusted directory.
        trust = subprocess.run([*self.argv, "trust"], capture_output=True, text=True, cwd=root)
        if trust.returncode != 0:
            raise ToolFailure(trust.stderr.strip())
        out = subprocess.run(
            [*self.argv, "mcp", "package", *flags],
            input=session,
            capture_output=True,
            text=True,
            cwd=root,
        )
        return out.stdout, out.stderr, out.returncode

    def hash(self, path: Path, reverse: bool = False) -> list[tuple[str, str]] | None:
        flags = ["--reverse-items"] if reverse else []
        out = subprocess.run(
            [*self.argv, "hash", "--format", "json", *flags, str(path)],
            capture_output=True,
            text=True,
        )
        if out.returncode >= TOOL_FAILURE:
            raise ToolFailure(out.stderr.strip())
        if out.returncode != 0:
            return None
        return sorted((str(h["key"]), str(h["hash"])) for h in json.loads(out.stdout))

    def view(self, path: Path, surface: str, flags: list[str] | None = None) -> str | None:
        out = subprocess.run(
            [*self.argv, "view", "--as", surface, *(flags or []), str(path)],
            capture_output=True,
            text=True,
        )
        if out.returncode >= TOOL_FAILURE:
            raise ToolFailure(out.stderr.strip())
        return out.stdout if out.returncode == 0 else None

    def canonical(self, path: Path) -> bool:
        out = subprocess.run([*self.argv, "fmt", "--check", str(path)], capture_output=True)
        if out.returncode >= TOOL_FAILURE:
            raise ToolFailure(out.stderr.decode().strip())
        return out.returncode == 0


class LawExecutor(Protocol):
    @property
    def name(self) -> str: ...

    def hash(self, path: Path, reverse: bool = False) -> list[tuple[str, str]] | None: ...

    def view(self, path: Path, surface: str, flags: list[str] | None = None) -> str | None: ...

    def canonical(self, path: Path) -> bool: ...


def _items(value: str) -> list[str]:
    return [v.strip() for v in value.split(",") if v.strip()]


# SPEC-Q: 24.1 shows "# expect: E501 at 14:9" (Yan Agent; Yan and Yan Core use "--"), "# stdout:"
# blocks and "# exit: 101", but not the block layout or the metadata headers. Assumed (see
# conformance/README.md): headers are the file's leading comment lines; a "stdout:" block is
# the following prefixed lines (the prefix alone is an empty line) up to the next header or
# the first non-comment line; several blocks concatenate; unknown headers are ignored.
def parse_case(kind: str, path: Path, text: str) -> Case:
    prefix = COMMENT[path.suffix]
    case = Case(kind, path, milestone=DEFAULT_MILESTONE[path.suffix])
    stdout: list[str] | None = None
    in_stdout = False
    for line in text.splitlines():
        if not line.startswith(prefix):
            break
        body = line.removeprefix(prefix).removeprefix(" ")
        key, sep, value = body.partition(":")
        if in_stdout and not (sep and key in HEADERS):
            assert stdout is not None
            stdout.append(body)
            continue
        in_stdout = False
        match key if sep else "":
            case "expect":
                code, _, pos = value.strip().partition(" at ")
                ln, _, col = pos.partition(":")
                case.expect.append(Diag(code, int(ln), int(col)))
            case "exit":
                case.exit = int(value)
            case "stdout":
                in_stdout = True
                stdout = stdout if stdout is not None else []
            case "spec":
                case.spec = _items(value)
            case "rows":
                case.rows = _items(value)
            case "flags":
                case.flags = value.split()
            case "milestone":
                case.milestone = value.strip()
            case "ub":
                case.ub = value.strip()
    case.stdout = None if stdout is None else "".join(s + "\n" for s in stdout)
    return case


def _files(base: Path) -> list[Path]:
    return [p for p in sorted(base.rglob("*")) if p.suffix in COMMENT]


def discover(root: Path) -> list[Case]:
    cases: list[Case] = []
    for surface in SURFACES:
        for kind in SURFACE_KINDS:
            for p in _files(root / surface / kind):
                cases.append(parse_case(kind, p, p.read_text(encoding="utf-8")))
    for top, kind in TOP_KINDS.items():
        base = root / top
        if not base.is_dir():
            continue
        if kind in ("patch", "semver", "pkg", "fmt", "mcp"):
            cases += [_dir_case(kind, p) for p in sorted(base.iterdir()) if p.is_dir()]
            continue
        for p in _files(base):
            cases.append(parse_case(kind, p, p.read_text(encoding="utf-8")))
    return cases


# Directory cases (patch/, semver/, pkg/) describe themselves in expect.toml
# (see conformance/README.md).
def _dir_case(kind: str, path: Path) -> Case:
    expect = path / "expect.toml"
    meta = tomllib.loads(expect.read_text(encoding="utf-8")) if expect.is_file() else {}
    spec = meta.get("spec", "")
    flags = meta.get("flags", [])
    assert isinstance(spec, str) and isinstance(flags, list)
    return Case(
        kind,
        path,
        spec=_items(spec),
        rows=[str(r) for r in meta.get("rows", [])],
        flags=[str(f) for f in flags],
        milestone=str(meta.get("milestone", "M2a" if kind == "semver" else "M1")),
        meta=meta,
    )


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
            got = _diags(ex.check(case.path, case.flags), errors_only=True)
            if got:
                return Result(name, ex.name, "fail", f"unexpected errors: {got}")
        case "fail":
            got = _diags(ex.check(case.path, case.flags), errors_only=False)
            if Counter(got) != Counter(case.expect):
                return Result(name, ex.name, "fail", f"expected {sorted(case.expect)}, got {got}")
        case "run":
            stdout, code = ex.run(case.path, case.flags)
            if stdout != (case.stdout or "") or code != case.exit:
                return Result(name, ex.name, "fail", f"exit {code}, stdout {stdout!r}")
        case "ub":
            stderr, code = ex.run_ub(case.path, case.flags)
            flagged = "ub(" in stderr
            if case.ub == "none" and (flagged or code != 0):
                return Result(name, ex.name, "fail", f"expected a clean run, exit {code}: {stderr}")
            if case.ub != "none" and (f"ub({case.ub})" not in stderr or code == 0):
                return Result(
                    name, ex.name, "fail", f"expected ub({case.ub}), exit {code}: {stderr}"
                )
        case "patch":
            return _judge_patch(case, ex)
        case "pkg":
            return _judge_pkg(case, ex)
        case "fmt":
            return _judge_fmt(case, ex)
        case "mcp":
            return _judge_mcp(case, ex)
        case "semver":
            bump = ex.semver(case.path / "old", case.path / "new")
            if bump != case.meta.get("bump"):
                return Result(
                    name, ex.name, "fail", f"expected {case.meta.get('bump')}, got {bump}"
                )
        case _:
            return Result(name, ex.name, "skip", f"{case.kind} case format not defined yet")
    return Result(name, ex.name, "pass")


def _judge_patch(case: Case, ex: Executor) -> Result:
    name = str(case.path)
    found = [p for p in (case.path / "patch.json", case.path / "patch.yag") if p.is_file()]
    if not found:
        return Result(name, ex.name, "fail", "no patch.json or patch.yag")
    patch = found[0]
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp)
        # The expected result stays out of the work directory: m.yag is the only module there.
        result = case.meta.get("result")
        for f in case.path.iterdir():
            if f.name != result:
                shutil.copy(f, work / f.name)
        records, code = ex.patch(work, work / patch.name, case.flags)
        after = (work / "m.yag").read_text(encoding="utf-8")
    before = (case.path / "m.yag").read_text(encoding="utf-8")
    want_code = case.meta.get("code")
    if want_code is not None:
        codes = [str(r.get("code")) for r in records]
        if want_code not in codes or code == 0 or after != before:
            return Result(name, ex.name, "fail", f"expected {want_code} and no change, got {codes}")
        return Result(name, ex.name, "pass")
    want = (case.path / str(case.meta["result"])).read_text(encoding="utf-8")
    if code != 0 or after != want:
        return Result(name, ex.name, "fail", f"exit {code}, module {after!r}")
    return Result(name, ex.name, "pass")


def _judge_fmt(case: Case, ex: Executor) -> Result:
    name = str(case.path)
    src = next((p for p in sorted(case.path.glob("in.*")) if p.suffix in COMMENT), None)
    want_file = None if src is None else case.path / f"out{src.suffix}"
    if src is None or want_file is None or not want_file.is_file():
        return Result(name, ex.name, "fail", "no in.<ext> and out.<ext> pair")
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp) / src.name
        shutil.copy(src, work)
        code = ex.fmt(work)
        got = work.read_bytes()
    if code != 0 or got != want_file.read_bytes():
        return Result(name, ex.name, "fail", f"exit {code}, output {got!r}")
    return Result(name, ex.name, "pass")


def _judge_pkg(case: Case, ex: Executor) -> Result:
    name = str(case.path)
    args = case.meta.get("command")
    if not isinstance(args, list) or not (case.path / "root").is_dir():
        return Result(name, ex.name, "fail", "no command or no root/")
    with tempfile.TemporaryDirectory() as tmp:
        shutil.copytree(case.path, tmp, dirs_exist_ok=True)
        records, code = ex.tool(Path(tmp) / "root", [str(a) for a in args])
    want = case.meta.get("code")
    if want is None:
        errors = [r.get("code") for r in records if r.get("severity") == "error"]
        if code != 0 or errors:
            return Result(name, ex.name, "fail", f"exit {code}, errors {errors}")
        return Result(name, ex.name, "pass")
    at = case.meta.get("at")
    hits = [r for r in records if r.get("code") == want]
    if at is not None:
        hits = [r for r in hits if _where(r) == at]
    if not hits or code == 0:
        return Result(name, ex.name, "fail", f"expected {want} at {at}, exit {code}: {records}")
    return Result(name, ex.name, "pass")


def _judge_mcp(case: Case, ex: Executor) -> Result:
    name = str(case.path)
    if not (case.path / "root").is_dir():
        return Result(name, ex.name, "fail", "no root/")
    session = case.path / "session.jsonl"
    with tempfile.TemporaryDirectory() as tmp:
        shutil.copytree(case.path, tmp, dirs_exist_ok=True)
        out, err, code = ex.mcp(
            Path(tmp) / "root",
            case.flags,
            session.read_text(encoding="utf-8") if session.is_file() else "",
        )
    want = case.meta.get("code")
    if want is not None:
        if code == 0 or not re.search(rf"\b{want}\b", err):
            return Result(name, ex.name, "fail", f"expected {want}, exit {code}: {err.strip()}")
        return Result(name, ex.name, "pass")
    if code != 0:
        return Result(name, ex.name, "fail", f"exit {code}: {err.strip()}")
    listed, replies = case.path / "expect.json", case.path / "expect.jsonl"
    if listed.is_file():
        got: object = json.loads(out)
        expected: object = json.loads(listed.read_text(encoding="utf-8"))
    elif replies.is_file():
        got = [json.loads(line) for line in out.splitlines() if line.strip()]
        lines = replies.read_text(encoding="utf-8").splitlines()
        expected = [json.loads(line) for line in lines if line.strip()]
    else:
        return Result(name, ex.name, "fail", "no code, expect.json or expect.jsonl")
    if got != expected:
        return Result(name, ex.name, "fail", f"got {json.dumps(got)}")
    return Result(name, ex.name, "pass")


def _where(record: dict[str, object]) -> str:
    span = record.get("span")
    assert isinstance(span, dict)
    return f"{record.get('file')}:{span['line']}:{span['col']}"


type Hashes = list[tuple[str, str]]


def _hash_diff(a: Hashes, b: Hashes) -> str:
    only_a = sorted(set(a) - set(b))
    only_b = sorted(set(b) - set(a))
    keys = sorted({k for k, _ in only_a} | {k for k, _ in only_b})
    return "hashes differ for " + ", ".join(keys)


def _without_comments(text: str, prefix: str) -> str:
    return "".join(
        line for line in text.splitlines(keepends=True) if not line.lstrip().startswith(prefix)
    )


def _more_blank_lines(text: str) -> str:
    return text.replace("\n\n", "\n\n\n") + "\n\n"


def law4(case: Case, ex: LawExecutor) -> Result:
    """Item hashes do not depend on formatting, comments or item order."""
    name = str(case.path)
    base = ex.hash(case.path)
    if base is None:
        return Result(name, ex.name, "skip", "does not load")
    text = case.path.read_text(encoding="utf-8")
    prefix = COMMENT[case.path.suffix]
    with tempfile.TemporaryDirectory() as tmp:
        variants: list[tuple[str, Hashes | None]] = [("item order", ex.hash(case.path, True))]
        for what, body in [
            ("comments", _without_comments(text, prefix)),
            ("blank lines", _more_blank_lines(text)),
        ]:
            # Keep the case's directories: a module under `ffi/` is an ffi module (12).
            p = Path(tmp) / case.path.relative_to(case.path.anchor)
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(body, encoding="utf-8")
            variants.append((what, ex.hash(p)))
    for what, got in variants:
        if got is None:
            return Result(name, ex.name, "fail", f"{what}: no longer loads")
        if got != base:
            return Result(name, ex.name, "fail", f"{what}: {_hash_diff(base, got)}")
    return Result(name, ex.name, "pass")


def law3(case: Case, ex: LawExecutor) -> Result:
    """A Yan module within the Yan Agent subset elaborates as its Yan Agent projection does."""
    name = str(case.path)
    if case.path.suffix != ".yn":
        return Result(name, ex.name, "skip", "not Yan")
    base = ex.hash(case.path)
    if base is None:
        return Result(name, ex.name, "skip", "does not load")
    yag = ex.view(case.path, "yag")
    if yag is None:
        return Result(name, ex.name, "skip", "outside the Yan Agent subset")
    with tempfile.TemporaryDirectory() as tmp:
        p = Path(tmp) / case.path.with_suffix(".yag").name
        p.write_text(yag, encoding="utf-8")
        got = ex.hash(p)
    if got is None:
        return Result(name, ex.name, "fail", "the Yan Agent projection does not load")
    if got != base:
        return Result(name, ex.name, "fail", _hash_diff(base, got))
    return Result(name, ex.name, "pass")


def _first_difference(a: str, b: str) -> str:
    for n, (x, y) in enumerate(zip(a.splitlines(), b.splitlines(), strict=False), 1):
        if x != y:
            return f"line {n}: {x!r} vs {y!r}"
    return f"{len(a.splitlines())} vs {len(b.splitlines())} lines"


def law1(case: Case, ex: LawExecutor) -> Result:
    """A canonical Yan Agent module prints back from its Yan Core byte for byte.

    Comments do not reach Yan yet, so both sides are compared without comment lines.
    """
    name = str(case.path)
    if case.path.suffix not in (".yag",):
        return Result(name, ex.name, "skip", "not Yan Agent")
    if not ex.canonical(case.path):
        return Result(name, ex.name, "skip", "not canonical")
    # Strict: a spelling mark that raise did not use fails the case (RFC 0034 D6).
    got = ex.view(case.path, "yag", ["--strict"])
    if got is None:
        return Result(name, ex.name, "skip", "does not load or is outside the Yan Agent subset")
    want = _without_comments(case.path.read_text(encoding="utf-8"), "#").lstrip("\n")
    if got != want:
        return Result(name, ex.name, "fail", _first_difference(want, got))
    return _bare(case, ex)


def _bare(case: Case, ex: LawExecutor) -> Result:
    """The bare companion of law 1 (RFC 0034 D7): raise without marks must elaborate to the same
    item hashes, so a mark cannot cover for a raise bug."""
    name = str(case.path)
    base = ex.hash(case.path)
    bare = ex.view(case.path, "yag", ["--bare"])
    if base is None or bare is None:
        return Result(name, ex.name, "fail", "bare: no longer loads")
    with tempfile.TemporaryDirectory() as tmp:
        p = Path(tmp) / case.path.name
        p.write_text(bare, encoding="utf-8")
        got = ex.hash(p)
    if got is None:
        return Result(name, ex.name, "fail", "bare: the raised module does not load")
    if got != base:
        return Result(name, ex.name, "fail", f"bare: {_hash_diff(base, got)}")
    return Result(name, ex.name, "pass")


LAWS = {"1": law1, "3": law3, "4": law4}
LAW_KINDS = ("pass", "run")


def check_laws(cases: list[Case], ex: LawExecutor) -> dict[str, list[Result]]:
    out: dict[str, list[Result]] = {}
    for law, check in LAWS.items():
        results = []
        for c in cases:
            if c.kind not in LAW_KINDS or c.path.suffix not in COMMENT:
                continue
            try:
                results.append(check(c, ex))
            except ToolFailure as e:
                # A construct the implementation does not lower yet is not a law violation.
                status: Status = "skip" if "not supported yet" in str(e) else "fail"
                results.append(Result(str(c.path), ex.name, status, f"tool failure: {e}"))
        out[law] = results
    return out


def selected(cases: list[Case], milestone: str | None) -> list[Case]:
    if milestone is None:
        return cases
    limit = MILESTONES.index(milestone)
    return [c for c in cases if MILESTONES.index(c.milestone) <= limit]


def manifest(cases: list[Case], root: Path) -> dict[str, object]:
    return {
        "schema": 1,
        "cases": [
            {
                "path": c.path.relative_to(root).as_posix(),
                "kind": c.kind,
                "milestone": c.milestone,
                "spec": c.spec,
                "rows": c.rows,
                "flags": c.flags,
                "expect": [asdict(d) for d in c.expect],
                **({"stdout": c.stdout, "exit": c.exit} if c.kind == "run" else {}),
                **({"ub": c.ub} if c.ub else {}),
                **({"expect_toml": c.meta} if c.meta else {}),
            }
            for c in cases
        ],
    }


def _surface(case: Case) -> str:
    # patch/, semver/ and pkg/ directories hold Yan Agent modules.
    return {".yag": "Yan Agent", ".yn": "Yan", ".yir": "Yan Core"}.get(
        case.path.suffix, "Yan Agent"
    )


EXEMPT_FILE = Path(__file__).with_name("coverage_exempt.toml")


def coverage(cases: list[Case], catalogue: dict[str, dict[str, dict[str, str]]]) -> str:
    by_code: dict[str, set[str]] = defaultdict(set)
    by_section: dict[str, set[str]] = defaultdict(set)
    rows: Counter[str] = Counter()
    for c in cases:
        for d in c.expect:
            by_code[d.code].add(_surface(c))
        if "code" in c.meta:
            by_code[str(c.meta["code"])].add(_surface(c))
        for s in c.spec:
            by_section[s].add(_surface(c))
        rows.update(c.rows)

    codes = catalogue["codes"]
    active = {k: v for k, v in codes.items() if v["status"] == "active"}
    covered = [k for k in active if k in by_code]
    unknown = sorted(set(by_code) - set(codes) - {k for k in by_code if k[0] == "N"})
    exempt = set(tomllib.loads(EXEMPT_FILE.read_text(encoding="utf-8"))["sections"])
    sections = {k: v for k, v in catalogue["sections"].items() if k not in exempt}
    out = [
        "# Conformance coverage",
        "",
        "Generated by `tools/conform.py --coverage`; do not edit.",
        "",
        f"- Cases: {len(cases)} ({_counts(cases)})",
        f"- Diagnostic codes with a test: {len(covered)} of {len(active)} active codes",
        f"- Spec sections with a test: {sum(1 for s in sections if s in by_section)}"
        f" of {len(sections)}",
        f"- Change rows and SPEC_QUESTIONS cited: {len(rows)}",
        "",
    ]
    if unknown:
        out += [
            "Codes expected by a test but missing from the catalogue: " + ", ".join(unknown),
            "",
        ]
    out += [
        "## Diagnostic codes",
        "",
        "| Code | Section | Meaning | Tested in |",
        "|---|---|---|---|",
    ]
    for code, info in codes.items():
        if info["status"] != "active":
            continue
        where = ", ".join(sorted(by_code.get(code, set()))) or "**none**"
        out.append(f"| {code} | {info['section']} | {info['meaning']} | {where} |")
    notes = sorted(k for k in by_code if k[0] == "N")
    out += ["", "## Lenient-reader notes", "", ", ".join(notes) or "none", ""]
    out += ["## Spec sections", "", "| Section | Title | Tested in |", "|---|---|---|"]
    for sec, title in sections.items():
        where = ", ".join(sorted(by_section.get(sec, set()))) or "**none**"
        out.append(f"| {sec} | {title} | {where} |")
    out += ["", "## Rows cited", "", ", ".join(f"{r} ({n})" for r, n in sorted(rows.items())), ""]
    return "\n".join(out)


def _counts(cases: list[Case]) -> str:
    c = Counter(f"{p.path.suffix[1:] or 'yag'} {p.kind}" for p in cases)
    return ", ".join(f"{k} {n}" for k, n in sorted(c.items()))


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawTextHelpFormatter)
    ap.add_argument("--root", type=Path, default=Path("conformance"))
    ap.add_argument("--executor", action="append", default=[], metavar="NAME=CMD")
    ap.add_argument("--milestone", choices=MILESTONES, help="run only cases due by this milestone")
    ap.add_argument("--json", type=Path, help="write a JSON summary here")
    ap.add_argument("--manifest", type=Path, help="write the case manifest (JSON) here")
    ap.add_argument("--coverage", type=Path, help="write the coverage report (Markdown) here")
    ap.add_argument(
        "--laws", action="store_true", help="report the round-trip laws (16.5) instead; never fails"
    )
    ap.add_argument("--report-only", action="store_true", help="always exit 0")
    args = ap.parse_args(argv)

    executors = []
    for spec in args.executor:
        name, _, cmd = spec.partition("=")
        executors.append(CommandExecutor(name, shlex.split(cmd)))

    cases = discover(args.root)
    if args.manifest:
        args.manifest.write_text(json.dumps(manifest(cases, args.root), indent=1) + "\n")
    if args.coverage:
        catalogue = tomllib.loads((args.root / "catalogue.toml").read_text(encoding="utf-8"))
        args.coverage.write_text(coverage(cases, catalogue))

    cases = selected(cases, args.milestone)
    if args.laws:
        return report_laws(cases, executors, args.json)
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
    return 1 if counts["fail"] and not args.report_only else 0


def report_laws(cases: list[Case], executors: list[CommandExecutor], out: Path | None) -> int:
    summary: dict[str, object] = {}
    for ex in executors:
        for law, results in check_laws(cases, ex).items():
            counts = Counter(r.status for r in results)
            for r in results:
                if r.status == "fail":
                    print(f"FAIL law {law} {r.executor} {r.case}: {r.detail}")
            print(f"law {law}, {ex.name}: {dict(counts)}")
            tally = {str(k): n for k, n in counts.items()}
            summary[f"{ex.name} law {law}"] = {
                "counts": tally,
                "results": [asdict(r) for r in results],
            }
    if out:
        out.write_text(json.dumps(summary, indent=2))
    # Reported only until M3, when the laws gate CI (R564).
    return 0


if __name__ == "__main__":
    sys.exit(main())
