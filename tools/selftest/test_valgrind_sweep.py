from pathlib import Path

from tools import valgrind_sweep


def test_a_refused_build_is_not_a_finding(tmp_path: Path) -> None:
    yan = tmp_path / "yan"
    yan.write_text("#!/bin/sh\nexit 2\n")
    yan.chmod(0o755)
    assert valgrind_sweep.clean(yan, "examples/real/wordfreq.yag", [], tmp_path) is None
