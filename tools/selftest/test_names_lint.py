from pathlib import Path

from tools import line_ratchet, names_lint

CFG = {"retired": ["Yong", "Fan"], "allow": ["archive/**"]}


def test_flags_retired_name(tmp_path: Path) -> None:
    (tmp_path / "a.md").write_text("The Yong core.\n")
    assert names_lint.scan(["a.md"], tmp_path, CFG) == ["a.md:1: retired name Yong"]


def test_hyphenated_and_allowlisted_pass(tmp_path: Path) -> None:
    (tmp_path / "archive").mkdir()
    (tmp_path / "archive" / "x.md").write_text("Yong\n")
    (tmp_path / "b.md").write_text("Fan-out and Hyndman-Fan\n")
    assert names_lint.scan(["archive/x.md", "b.md"], tmp_path, CFG) == []


def test_main_is_clean() -> None:
    assert names_lint.main() == 0


def test_ratchet_flags_growth(tmp_path: Path) -> None:
    (tmp_path / "f.rs").write_text("x\n" * 5)
    assert line_ratchet.over(tmp_path, ["f.rs"], cap=4) == ["f.rs: 5 lines exceeds the cap of 4"]
    assert line_ratchet.over(tmp_path, ["f.rs"], cap=5) == []


def test_ratchet_main_is_clean() -> None:
    assert line_ratchet.main() == 0
