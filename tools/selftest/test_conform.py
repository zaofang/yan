import tomllib
from dataclasses import dataclass, field
from pathlib import Path

from tools.conform import (
    Case,
    Diag,
    ToolFailure,
    coverage,
    discover,
    judge,
    manifest,
    parse_case,
    selected,
)


@dataclass
class Fake:
    name: str = "fake"
    diags: list[dict[str, object]] = field(default_factory=list)
    output: tuple[str, int] = ("", 0)
    seen_flags: list[list[str]] = field(default_factory=list)
    ub_output: tuple[str, int] = ("", 0)
    patched: str | None = None
    patch_records: list[dict[str, object]] = field(default_factory=list)
    bump: str = "patch"
    tool_result: tuple[list[dict[str, object]], int] = ([], 0)
    seen_args: list[list[str]] = field(default_factory=list)
    formatted: str | None = None
    fmt_exit: int = 0

    def check(self, path: Path, flags: list[str]) -> list[dict[str, object]]:
        self.seen_flags.append(flags)
        return self.diags

    def run(self, path: Path, flags: list[str]) -> tuple[str, int]:
        self.seen_flags.append(flags)
        return self.output

    def run_ub(self, path: Path, flags: list[str]) -> tuple[str, int]:
        return self.ub_output

    def patch(
        self, workdir: Path, patch: Path, flags: list[str]
    ) -> tuple[list[dict[str, object]], int]:
        if self.patched is not None:
            (workdir / "m.yag").write_text(self.patched)
        return self.patch_records, 1 if self.patch_records else 0

    def semver(self, old: Path, new: Path) -> str:
        return self.bump

    def fmt(self, path: Path) -> int:
        if self.formatted is not None:
            path.write_text(self.formatted)
        return self.fmt_exit

    def tool(self, workdir: Path, args: list[str]) -> tuple[list[dict[str, object]], int]:
        assert (workdir / "package.yag").is_file()
        self.seen_args.append(args)
        return self.tool_result


def diag(code: str, line: int, col: int, severity: str = "error") -> dict[str, object]:
    return {"code": code, "severity": severity, "span": {"line": line, "col": col}}


def test_parse_case_headers() -> None:
    text = (
        "# spec: 8.6, 4.3\n# rows: R345\n# milestone: M2a\n# flags: --strict\n# note: why\n"
        "# expect: E501 at 14:9\n# stdout:\n# hello\n#\n# world\n# exit: 101\nmain()\n# x\n"
    )
    case = parse_case("run", Path("r.yag"), text)
    assert case.expect == [Diag("E501", 14, 9)]
    assert case.stdout == "hello\n\nworld\n"
    assert case.exit == 101
    assert case.spec == ["8.6", "4.3"]
    assert case.rows == ["R345"]
    assert case.milestone == "M2a"
    assert case.flags == ["--strict"]


def test_defaults_by_surface() -> None:
    case = parse_case("pass", Path("p.yn"), "x = 1\n")
    assert (case.stdout, case.exit, case.milestone, case.flags) == (None, 0, "M3", [])
    assert parse_case("pass", Path("p.yir"), "").milestone == "M0"


def test_dash_comment_headers() -> None:
    case = parse_case("run", Path("r.yir"), "-- exit: 1\n-- stdout:\n-- hi\n--\nmodule m\n")
    assert (case.stdout, case.exit) == ("hi\n\n", 1)


def test_fail_case_needs_exact_codes() -> None:
    case = Case("fail", Path("f.yag"), [Diag("E501", 1, 2)])
    assert judge(case, Fake(diags=[diag("E501", 1, 2)])).status == "pass"
    assert judge(case, Fake(diags=[diag("E501", 1, 2), diag("E502", 3, 1)])).status == "fail"
    assert judge(case, Fake(diags=[diag("W820", 1, 2, "warning")])).status == "fail"
    assert judge(case, Fake(diags=[])).status == "fail"


def test_pass_case_ignores_warnings() -> None:
    case = Case("pass", Path("p.yag"))
    assert judge(case, Fake(diags=[diag("W100", 1, 1, "warning")])).status == "pass"
    assert judge(case, Fake(diags=[diag("E100", 1, 1)])).status == "fail"


def test_run_case_compares_stdout_and_exit() -> None:
    case = Case("run", Path("r.yag"), stdout="hi\n", exit=101)
    assert judge(case, Fake(output=("hi\n", 101))).status == "pass"
    assert judge(case, Fake(output=("hi\n", 0))).status == "fail"


def test_flags_reach_the_executor() -> None:
    ex = Fake()
    judge(Case("pass", Path("p.yag"), flags=["--level", "release"]), ex)
    assert ex.seen_flags == [["--level", "release"]]


def test_tool_failure_is_a_failed_case() -> None:
    class Broken(Fake):
        def check(self, path: Path, flags: list[str]) -> list[dict[str, object]]:
            raise ToolFailure("no front end")

    result = judge(Case("pass", Path("p.yir")), Broken())
    assert result.status == "fail"
    assert "no front end" in result.detail


def test_discover(tmp_path: Path) -> None:
    (tmp_path / "yag" / "fail").mkdir(parents=True)
    (tmp_path / "yag" / "fail" / "a.yag").write_text("# expect: E1 at 1:1\n")
    (tmp_path / "yir" / "fail").mkdir(parents=True)
    (tmp_path / "yir" / "fail" / "b.yir").write_text("-- expect: E2 at 3:4\nmodule m\n")
    (tmp_path / "yir" / "fail" / "c.txt").write_text("")
    (tmp_path / "solver").mkdir()
    (tmp_path / "solver" / "s.yag").write_text("x = 1\n")
    (tmp_path / "golden").mkdir()
    (tmp_path / "golden" / "g.yag").write_text("# stdout:\n# hi\n")
    (tmp_path / "patch" / "p1").mkdir(parents=True)
    cases = discover(tmp_path)
    assert [(c.kind, c.path.name) for c in cases] == [
        ("fail", "a.yag"),
        ("fail", "b.yir"),
        ("run", "g.yag"),
        ("fail", "s.yag"),
        ("patch", "p1"),
    ]
    assert cases[1].expect == [Diag("E2", 3, 4)]
    assert judge(cases[3], Fake()).status == "pass"
    assert judge(cases[4], Fake()).status == "fail"


def test_missing_suite_is_empty(tmp_path: Path) -> None:
    assert discover(tmp_path / "nope") == []


def test_milestone_selection() -> None:
    early = Case("pass", Path("a.yir"), milestone="M0")
    late = Case("pass", Path("b.yn"), milestone="M3")
    assert selected([early, late], "M1") == [early]
    assert selected([early, late], None) == [early, late]


def test_manifest_paths_are_relative(tmp_path: Path) -> None:
    case = Case("run", tmp_path / "yag" / "run" / "r.yag", stdout="hi\n", spec=["24.3"])
    assert manifest([case], tmp_path)["cases"] == [
        {
            "path": "yag/run/r.yag",
            "kind": "run",
            "milestone": "M0",
            "spec": ["24.3"],
            "rows": [],
            "flags": [],
            "expect": [],
            "stdout": "hi\n",
            "exit": 0,
        }
    ]


def test_coverage_counts_codes_and_sections() -> None:
    catalogue = tomllib.loads(
        """
        [codes.E501]
        section = "8.6"
        status = "active"
        meaning = "index out of bounds"
        [codes.E502]
        section = "7.1"
        status = "active"
        meaning = "non-exhaustive match"
        [codes.E231]
        section = "6.1"
        status = "retired"
        meaning = "old"
        [sections]
        "8.6" = "Solver"
        "7.1" = "Match"
        """
    )
    case = Case("fail", Path("f.yag"), [Diag("E501", 1, 1), Diag("N05", 2, 1)], spec=["8.6"])
    report = coverage([case], catalogue)
    assert "1 of 2 active codes" in report
    assert "| E502 | 7.1 | non-exhaustive match | **none** |" in report
    assert "E231" not in report
    assert "N05" in report


def test_ub_cases() -> None:
    flagged = Case("ub", Path("u.yn"), ub="3")
    assert (
        judge(flagged, Fake(ub_output=("ub(3): offset outside allocation\n", 101))).status == "pass"
    )
    assert judge(flagged, Fake(ub_output=("ub(2): dead allocation\n", 101))).status == "fail"
    assert judge(flagged, Fake()).status == "fail"
    clean = Case("ub", Path("c.yn"), ub="none")
    assert judge(clean, Fake()).status == "pass"
    assert judge(clean, Fake(ub_output=("ub(1): data race\n", 101))).status == "fail"


def _patch_case(tmp: Path, expect: str) -> Case:
    (tmp / "m.yag").write_text("old\n")
    (tmp / "patch.json").write_text("{}")
    (tmp / "result.yag").write_text("new\n")
    (tmp / "expect.toml").write_text(expect)
    [case] = discover_dirs(tmp)
    return case


def discover_dirs(tmp: Path) -> list[Case]:
    root = tmp.parent / (tmp.name + "-root")
    (root / "patch").mkdir(parents=True)
    tmp.rename(root / "patch" / "p1")
    return [c for c in discover(root) if c.kind == "patch"]


def test_patch_result(tmp_path: Path) -> None:
    (tmp_path / "a").mkdir()
    case = _patch_case(tmp_path / "a", 'spec = "18.3"\nresult = "result.yag"\n')
    assert judge(case, Fake(patched="new\n")).status == "pass"
    assert judge(case, Fake(patched="other\n")).status == "fail"


def test_patch_refusal_leaves_module(tmp_path: Path) -> None:
    (tmp_path / "a").mkdir()
    case = _patch_case(tmp_path / "a", 'spec = "18.3"\ncode = "P002"\n')
    refusal: list[dict[str, object]] = [{"code": "P002"}]
    assert judge(case, Fake(patch_records=refusal)).status == "pass"
    assert judge(case, Fake(patch_records=refusal, patched="half\n")).status == "fail"
    assert judge(case, Fake()).status == "fail"


def test_semver_bump(tmp_path: Path) -> None:
    (tmp_path / "semver" / "s1").mkdir(parents=True)
    (tmp_path / "semver" / "s1" / "expect.toml").write_text('spec = "20.6"\nbump = "major"\n')
    [case] = discover(tmp_path)
    assert case.milestone == "M2a"
    assert judge(case, Fake(bump="major")).status == "pass"
    assert judge(case, Fake(bump="minor")).status == "fail"


def _pkg(tmp_path: Path, expect: str) -> Path:
    case = tmp_path / "pkg" / "p1"
    (case / "root").mkdir(parents=True)
    (case / "root" / "package.yag").write_text("")
    (case / "expect.toml").write_text(expect)
    return case


def test_pkg_code(tmp_path: Path) -> None:
    _pkg(tmp_path, 'spec = "20.6"\ncommand = ["check"]\ncode = "E001"\nat = "src/a.yag:2:18"\n')
    [case] = discover(tmp_path)
    assert case.milestone == "M1"
    hit = {**diag("E001", 2, 18), "file": "src/a.yag"}
    fake = Fake(tool_result=([hit], 1))
    assert judge(case, fake).status == "pass"
    assert fake.seen_args == [["check"]]
    moved = {**diag("E001", 3, 18), "file": "src/a.yag"}
    assert judge(case, Fake(tool_result=([moved], 1))).status == "fail"
    assert judge(case, Fake(tool_result=([hit], 0))).status == "fail"


def test_pkg_ok(tmp_path: Path) -> None:
    _pkg(tmp_path, 'spec = "18.4"\ncommand = ["test", "--gate"]\n')
    [case] = discover(tmp_path)
    assert judge(case, Fake(tool_result=([diag("W815", 1, 1, "warning")], 0))).status == "pass"
    assert judge(case, Fake(tool_result=([diag("E014", 1, 1)], 1))).status == "fail"


def _fmt(tmp_path: Path, out: str = "b\n") -> None:
    case = tmp_path / "fmt" / "f1"
    case.mkdir(parents=True)
    (case / "in.yag").write_text("a\n")
    (case / "out.yag").write_text(out)
    (case / "expect.toml").write_text('spec = "18.5"\n')


def test_fmt_compares_bytes(tmp_path: Path) -> None:
    _fmt(tmp_path)
    [case] = discover(tmp_path)
    assert case.kind == "fmt"
    assert case.milestone == "M1"
    assert judge(case, Fake(formatted="b\n")).status == "pass"
    assert judge(case, Fake(formatted="b \n")).status == "fail"
    assert judge(case, Fake()).status == "fail"
    assert judge(case, Fake(formatted="b\n", fmt_exit=1)).status == "fail"
    assert (tmp_path / "fmt" / "f1" / "in.yag").read_text() == "a\n"


def test_fmt_needs_a_pair(tmp_path: Path) -> None:
    _fmt(tmp_path)
    (tmp_path / "fmt" / "f1" / "out.yag").unlink()
    [case] = discover(tmp_path)
    assert judge(case, Fake(formatted="b\n")).status == "fail"
