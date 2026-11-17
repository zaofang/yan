from pathlib import Path

import pytest

from tools import nativelive


def test_allowlist_entry_needs_an_issue(tmp_path: Path) -> None:
    f = tmp_path / "a.toml"
    f.write_text('[[allow]]\nprogram = "x.yag"\n', encoding="utf-8")
    with pytest.raises(SystemExit):
        nativelive.load_allowlist(f)


def test_allowlist_maps_program_to_issue(tmp_path: Path) -> None:
    f = tmp_path / "a.toml"
    f.write_text('[[allow]]\nprogram = "x.yag"\nissue = "#1"\n', encoding="utf-8")
    assert nativelive.load_allowlist(f) == {"x.yag": "#1"}


def test_live_line_is_parsed() -> None:
    m = nativelive.LIVE.search("noise\nlive: 3\n")
    assert m is not None and m.group(1) == "3"
