import json
import tarfile
from pathlib import Path

from tools import site_inputs


def test_archive_carries_meta_brand_playground_and_the_compiler(tmp_path: Path) -> None:
    bins = tmp_path / "bin"
    bins.mkdir()
    for name in ("yan", "libyan_rt.a"):
        (bins / name).write_bytes(b"x")
    wasm = tmp_path / "yan.wasm"
    wasm.write_bytes(b"\0asm")
    out = tmp_path / "site-inputs.tar.gz"
    site_inputs.pack(out, bins, wasm, None)
    with tarfile.open(out) as t:
        names = set(t.getnames())
        member = t.extractfile("meta.json")
        assert member is not None
        meta = json.load(member)
    assert {"yan", "libyan_rt.a", "brand/lockup.svg", "playground/yan.wasm"} <= names
    assert {"docs/spec/rows.toml", "docs/conformance/manifest.json"} <= names
    assert len(meta["commit"]) == 40 and meta["rfcs"] > 0 and meta["cases"]
