from dataclasses import dataclass, field
from pathlib import Path

from tools.conform import Case, Diag, ToolFailure, discover, judge, parse_headers


@dataclass
class Fake:
    name: str = "fake"
    diags: list[dict[str, object]] = field(default_factory=list)
    output: tuple[str, int] = ("", 0)

    def check(self, path: Path) -> list[dict[str, object]]:
        return self.diags

    def run(self, path: Path) -> tuple[str, int]:
        return self.output


def diag(code: str, line: int, col: int, severity: str = "error") -> dict[str, object]:
    return {"code": code, "severity": severity, "span": {"line": line, "col": col}}


def test_parse_headers() -> None:
    text = "# expect: E501 at 14:9\n# exit: 101\n# stdout:\n# hello\n#\n# world\nmain()\n# x\n"
    expect, stdout, code = parse_headers(text)
    assert expect == [Diag("E501", 14, 9)]
    assert stdout == "hello\n\nworld\n"
    assert code == 101


def test_no_stdout_header_means_none() -> None:
    assert parse_headers("x = 1\n") == ([], None, 0)


def test_fail_case_needs_exact_codes() -> None:
    case = Case("fail", Path("f.jian"), [Diag("E501", 1, 2)])
    assert judge(case, Fake(diags=[diag("E501", 1, 2)])).status == "pass"
    assert judge(case, Fake(diags=[diag("E501", 1, 2), diag("E502", 3, 1)])).status == "fail"
    assert judge(case, Fake(diags=[])).status == "fail"


def test_pass_case_ignores_warnings() -> None:
    case = Case("pass", Path("p.jian"))
    assert judge(case, Fake(diags=[diag("W100", 1, 1, "warning")])).status == "pass"
    assert judge(case, Fake(diags=[diag("E100", 1, 1)])).status == "fail"


def test_run_case_compares_stdout_and_exit() -> None:
    case = Case("run", Path("r.jian"), stdout="hi\n", exit=101)
    assert judge(case, Fake(output=("hi\n", 101))).status == "pass"
    assert judge(case, Fake(output=("hi\n", 0))).status == "fail"


def test_discover(tmp_path: Path) -> None:
    (tmp_path / "jian" / "fail").mkdir(parents=True)
    (tmp_path / "jian" / "fail" / "a.jian").write_text("# expect: E1 at 1:1\n")
    (tmp_path / "yong" / "fail").mkdir(parents=True)
    (tmp_path / "yong" / "fail" / "b.yong").write_text("-- expect: E2 at 3:4\nmodule m\n")
    (tmp_path / "yong" / "fail" / "stray.jian").write_text("")
    (tmp_path / "patch" / "p1").mkdir(parents=True)
    cases = discover(tmp_path)
    assert [(c.kind, c.path.name) for c in cases] == [
        ("fail", "a.jian"),
        ("fail", "b.yong"),
        ("patch", "p1"),
    ]
    assert cases[1].expect == [Diag("E2", 3, 4)]
    assert judge(cases[2], Fake()).status == "skip"


def test_dash_comment_headers() -> None:
    text = "-- exit: 1\n-- stdout:\n-- hi\n--\nmodule m\n"
    assert parse_headers(text, "--") == ([], "hi\n\n", 1)


def test_tool_failure_is_a_failed_case() -> None:
    class Broken(Fake):
        def check(self, path: Path) -> list[dict[str, object]]:
            raise ToolFailure("no front end")

    result = judge(Case("pass", Path("p.yong")), Broken())
    assert result.status == "fail"
    assert "no front end" in result.detail


def test_missing_suite_is_empty(tmp_path: Path) -> None:
    assert discover(tmp_path / "nope") == []
