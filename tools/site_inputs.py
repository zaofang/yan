"""Packs what zaofang/yanlang.dev builds the website from: the `site-inputs` release (law 14: every
figure carries its commit and date).

uv run python -m tools.site_inputs OUT.tar.gz --bin-dir target/release [--status status.json]

The archive holds meta.json, brand/, playground/ (with yan.wasm), and `yan` with libyan_rt.a beside
it, which the site uses to check its tour programs.
"""

import argparse
import io
import json
import subprocess
import tarfile
from collections import Counter
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
PLAYGROUND = ("index.html", "worker.js", "examples.js")


def git(*args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout.strip()


def meta(status: Path | None) -> dict[str, Any]:
    cases = json.loads((ROOT / "conformance/manifest.json").read_text())["cases"]
    rfcs = [f for f in (ROOT / "rfcs").glob("[0-9]*.md") if not f.name.startswith("0000")]
    return {
        "commit": git("rev-parse", "HEAD"),
        "day": git("log", "-1", "--format=%cs"),
        "status": json.loads(status.read_text()) if status else None,
        "cases": dict(Counter(c["path"].split("/")[0] for c in cases)),
        "rfcs": len(rfcs),
    }


def pack(out: Path, bin_dir: Path, wasm: Path, status: Path | None) -> None:
    data = json.dumps(meta(status), indent=2).encode()
    with tarfile.open(out, "w:gz") as t:
        info = tarfile.TarInfo("meta.json")
        info.size = len(data)
        t.addfile(info, io.BytesIO(data))
        t.add(ROOT / "brand", "brand")
        for name in PLAYGROUND:
            t.add(ROOT / "playground" / name, f"playground/{name}")
        t.add(wasm, "playground/yan.wasm")
        t.add(bin_dir / "yan", "yan")
        t.add(bin_dir / "libyan_rt.a", "libyan_rt.a")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("out", type=Path)
    ap.add_argument("--bin-dir", type=Path, required=True)
    ap.add_argument("--wasm", type=Path, default=ROOT / "playground/yan.wasm")
    ap.add_argument("--status", type=Path)
    args = ap.parse_args()
    pack(args.out, args.bin_dir, args.wasm, args.status)


if __name__ == "__main__":
    main()
