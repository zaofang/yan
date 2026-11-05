from dataclasses import dataclass, field
from pathlib import Path

from tools.conform import Case, ToolFailure, check_laws, law1, law3, law4

type Hashes = list[tuple[str, str]]


@dataclass
class FakeLaws:
    """Hashes a module as one item per non-comment line, keyed by its first word."""

    name: str = "fake"
    order_matters: bool = False
    yag: str | None = "x = 1\n"
    bare: str | None = None
    seen: list[str] = field(default_factory=list)

    def hash(self, path: Path, reverse: bool = False) -> Hashes | None:
        text = path.read_text(encoding="utf-8")
        if "unsupported" in text:
            raise ToolFailure("yan: not supported yet: something")
        lines = [ln for ln in text.splitlines() if ln.strip() and not ln.startswith(("#", "--"))]
        if not lines:
            return None
        if reverse and self.order_matters:
            lines = [f"{ln}!" for ln in lines]
        self.seen.append(path.suffix)
        return sorted((ln.split()[0], ln) for ln in lines)

    def view(self, path: Path, surface: str, flags: list[str] | None = None) -> str | None:
        if "--bare" in (flags or []) and self.bare is not None:
            return self.bare
        return self.yag

    def canonical(self, path: Path) -> bool:
        return True


def case(tmp: Path, name: str, text: str, kind: str = "run") -> Case:
    p = tmp / name
    p.write_text(text, encoding="utf-8")
    return Case(kind, p)


def test_law4_passes_when_hashes_ignore_layout(tmp_path: Path) -> None:
    c = case(tmp_path, "m.yag", "# a comment\nx = 1\n\ny = 2\n")
    assert law4(c, FakeLaws()).status == "pass"


def test_law4_fails_on_item_order(tmp_path: Path) -> None:
    c = case(tmp_path, "m.yag", "x = 1\ny = 2\n")
    r = law4(c, FakeLaws(order_matters=True))
    assert r.status == "fail"
    assert r.detail.startswith("item order")


def test_law4_skips_modules_that_do_not_load(tmp_path: Path) -> None:
    assert law4(case(tmp_path, "m.yag", "# only a comment\n"), FakeLaws()).status == "skip"


def test_law3_compares_yn_with_its_yag_projection(tmp_path: Path) -> None:
    c = case(tmp_path, "m.yn", "x = 1\n")
    ex = FakeLaws()
    assert law3(c, ex).status == "pass"
    assert ex.seen == [".yn", ".yag"]
    assert law3(c, FakeLaws(yag="x = 2\n")).status == "fail"


def test_law3_skips_outside_the_yag_subset_and_other_surfaces(tmp_path: Path) -> None:
    assert law3(case(tmp_path, "m.yn", "x = 1\n"), FakeLaws(yag=None)).status == "skip"
    assert law3(case(tmp_path, "m.yag", "x = 1\n"), FakeLaws()).status == "skip"


def test_unsupported_constructs_skip_and_fail_cases_are_not_checked(tmp_path: Path) -> None:
    cases = [
        case(tmp_path, "a.yag", "unsupported\n"),
        case(tmp_path, "b.yag", "x = 1\n", kind="fail"),
    ]
    out = check_laws(cases, FakeLaws())
    assert [r.status for r in out["4"]] == ["skip"]


def test_law1_compares_without_comments(tmp_path: Path) -> None:
    c = case(tmp_path, "m.yag", "# stdout: x\n\nx = 1\n")
    assert law1(c, FakeLaws()).status == "pass"
    r = law1(c, FakeLaws(yag="x = 2\n"))
    assert r.status == "fail"
    assert "line 1" in r.detail


def test_law1_bare_check_catches_a_mark_covering_for_raise(tmp_path: Path) -> None:
    c = case(tmp_path, "m.yag", "x = 1\n")
    assert law1(c, FakeLaws(bare="x = 1\n")).status == "pass"
    r = law1(c, FakeLaws(bare="x = 2\n"))
    assert r.status == "fail"
    assert r.detail.startswith("bare:")
