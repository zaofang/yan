"""Run the conformance suite (spec 24) against one or more executors.

An executor is any command that implements:
    CMD check --format json [FLAGS] FILE  -> JSON array of diagnostic records (spec 18.2)
    CMD run [FLAGS] FILE                  -> the program's stdout and exit status
    CMD run --ub [FLAGS] FILE             -> exit status; a UB report names "ub(N)" on stderr
    CMD patch --format json [FLAGS] PATCH -> applies PATCH in its directory; JSON records on refusal
    CMD semver --format json --base OLD NEW -> JSON object with "bump"
    CMD ARGS... --format json             -> run in a pkg/ case's copied root; JSON records
Exit status 2 or above from either command is a failure of the tool, never a verdict.
The same suite drives the interpreter and every code generator (spec 22.1, 24.1).

With --laws, the runner instead checks the round-trip laws of 16.5 on every case (R564), using
    CMD hash --format json [--reverse-items] FILE -> JSON array of {"key", "hash", "identity"}
    CMD view --as xin FILE                        -> the module's Xin projection
    CMD fmt --check FILE                          -> exit 1 if FILE is not canonical
Exit status 1 from either means the module does not load or is outside the Xin subset; the
law is then skipped for that case.

The runner also writes the suite's manifest (--manifest) and coverage report (--coverage)
from the case headers and conformance/catalogue.toml.
"""

import argparse
import json
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

SURFACES = ("xin", "wen", "yong")
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
    "roundtrip": "roundtrip",
    "sim": "sim",
}
COMMENT = {".xin": "#", ".wen": "--", ".yong": "--"}
DEFAULT_MILESTONE = {".yong": "M0", ".xin": "M1", ".wen": "M3"}
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
        if out.returncode == TOOL_FAILURE and out.stderr.startswith("yong:"):
            raise ToolFailure(out.stderr.strip())
        return out.stdout, out.returncode

    def run_ub(self, path: Path, flags: list[str]) -> tuple[str, int]:
        out = subprocess.run(
            [*self.argv, "run", "--ub", *flags, str(path)], capture_output=True, text=True
        )
        if out.returncode == TOOL_FAILURE and out.stderr.startswith("yong:"):
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

    def view(self, path: Path, surface: str) -> str | None:
        out = subprocess.run(
            [*self.argv, "view", "--as", surface, str(path)], capture_output=True, text=True
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

    def view(self, path: Path, surface: str) -> str | None: ...

    def canonical(self, path: Path) -> bool: ...


def _items(value: str) -> list[str]:
    return [v.strip() for v in value.split(",") if v.strip()]


# SPEC-Q: 24.1 shows "# expect: E501 at 14:9" (Xin; Wen and Yong use "--"), "# stdout:"
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
        if kind in ("patch", "semver", "pkg"):
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
    found = [p for p in (case.path / "patch.json", case.path / "patch.xin") if p.is_file()]
    if not found:
        return Result(name, ex.name, "fail", "no patch.json or patch.xin")
    patch = found[0]
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp)
        for f in case.path.iterdir():
            shutil.copy(f, work / f.name)
        records, code = ex.patch(work, work / patch.name, case.flags)
        after = (work / "m.xin").read_text(encoding="utf-8")
    before = (case.path / "m.xin").read_text(encoding="utf-8")
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
    """A Wen module within the Xin subset elaborates as its Xin projection does."""
    name = str(case.path)
    if case.path.suffix != ".wen":
        return Result(name, ex.name, "skip", "not Wen")
    base = ex.hash(case.path)
    if base is None:
        return Result(name, ex.name, "skip", "does not load")
    xin = ex.view(case.path, "xin")
    if xin is None:
        return Result(name, ex.name, "skip", "outside the Xin subset")
    with tempfile.TemporaryDirectory() as tmp:
        p = Path(tmp) / case.path.with_suffix(".xin").name
        p.write_text(xin, encoding="utf-8")
        got = ex.hash(p)
    if got is None:
        return Result(name, ex.name, "fail", "the Xin projection does not load")
    if got != base:
        return Result(name, ex.name, "fail", _hash_diff(base, got))
    return Result(name, ex.name, "pass")


def _first_difference(a: str, b: str) -> str:
    for n, (x, y) in enumerate(zip(a.splitlines(), b.splitlines(), strict=False), 1):
        if x != y:
            return f"line {n}: {x!r} vs {y!r}"
    return f"{len(a.splitlines())} vs {len(b.splitlines())} lines"


def law1(case: Case, ex: LawExecutor) -> Result:
    """A canonical Xin module prints back from its Yong byte for byte.

    Comments do not reach Yong yet, so both sides are compared without comment lines.
    """
    name = str(case.path)
    if case.path.suffix not in (".xin", ".jian"):
        return Result(name, ex.name, "skip", "not Xin")
    if not ex.canonical(case.path):
        return Result(name, ex.name, "skip", "not canonical")
    got = ex.view(case.path, "xin")
    if got is None:
        return Result(name, ex.name, "skip", "does not load or is outside the Xin subset")
    want = _without_comments(case.path.read_text(encoding="utf-8"), "#").lstrip("\n")
    if got != want:
        return Result(name, ex.name, "fail", _first_difference(want, got))
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
    # patch/, semver/ and pkg/ directories hold Xin modules.
    return {".xin": "Xin", ".wen": "Wen", ".yong": "Yong"}.get(case.path.suffix, "Xin")


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
    sections = catalogue["sections"]
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
    c = Counter(f"{p.path.suffix[1:] or 'xin'} {p.kind}" for p in cases)
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
    return 1 if counts["fail"] else 0


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
