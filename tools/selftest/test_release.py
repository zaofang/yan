import os
import tarfile
from pathlib import Path

from tools.release import BOTTLES, checksums, formula, package

SUMS = "".join(f"{i:064x}  yan-{t}.tar.gz\n" for i, t in enumerate(BOTTLES.values()))


def test_checksums_maps_file_to_digest() -> None:
    assert checksums("ab  x.tar.gz\n\n")["x.tar.gz"] == "ab"


def test_formula_pins_every_target_to_its_digest() -> None:
    text = formula("1.0.0", checksums(SUMS))
    for i, target in enumerate(BOTTLES.values()):
        url = f"releases/download/v1.0.0/yan-{target}.tar.gz"
        assert f'{url}"\n      sha256 "{i:064x}"' in text
    assert 'version "1.0.0"' in text


def test_package_is_byte_identical_across_input_times(tmp_path: Path) -> None:
    a, b = tmp_path / "yan", tmp_path / "libyan_rt.a"
    a.write_bytes(b"exe")
    b.write_bytes(b"lib")
    a.chmod(0o755)
    package(tmp_path / "1.tar.gz", [a, b], 0)
    os.utime(a, (1, 1))
    package(tmp_path / "2.tar.gz", [b, a], 0)
    assert (tmp_path / "1.tar.gz").read_bytes() == (tmp_path / "2.tar.gz").read_bytes()
    with tarfile.open(tmp_path / "1.tar.gz") as t:
        assert [(m.name, m.mode) for m in t.getmembers()] == [
            ("libyan_rt.a", 0o644),
            ("yan", 0o755),
        ]
