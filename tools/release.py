"""Release helpers for the tag workflow (#863).

release.py package OUT FILE...          write a reproducible .tar.gz of FILEs (flat)
release.py formula VERSION SHA256SUMS   print the Homebrew formula for the project tap
"""

import argparse
import gzip
import io
import os
import tarfile
from pathlib import Path

REPO = "atbraz/yan-lang"
BOTTLES = {
    ("macos", "arm"): "aarch64-apple-darwin",
    ("macos", "intel"): "x86_64-apple-darwin",
    ("linux", "arm"): "aarch64-unknown-linux-gnu",
    ("linux", "intel"): "x86_64-unknown-linux-gnu",
}


def checksums(text: str) -> dict[str, str]:
    return {
        name: digest
        for digest, name in (line.split() for line in text.splitlines() if line.strip())
    }


def formula(version: str, sums: dict[str, str]) -> str:
    def block(os: str, cpu: str) -> str:
        archive = f"yan-{BOTTLES[os, cpu]}.tar.gz"
        url = f"https://github.com/{REPO}/releases/download/v{version}/{archive}"
        return f'    on_{cpu} do\n      url "{url}"\n      sha256 "{sums[archive]}"\n    end\n'

    platforms = "".join(
        f"  on_{os} do\n" + block(os, "arm") + block(os, "intel") + "  end\n\n"
        for os in ("macos", "linux")
    )
    return (
        "class Yan < Formula\n"
        '  desc "Yan language toolchain"\n'
        f'  homepage "https://github.com/{REPO}"\n'
        f'  version "{version}"\n'
        '  license "Apache-2.0 WITH LLVM-exception"\n\n' + platforms + "  def install\n"
        '    libexec.install "yan", "libyan_rt.a"\n'
        '    bin.install_symlink libexec/"yan"\n'
        "  end\n\n"
        "  test do\n"
        '    (testpath/"hello.yag").write <<~YAN\n'
        "      pub def main() uses io:\n"
        '          io.print("hello")\n'
        "    YAN\n"
        '    assert_equal "hello\\n", shell_output("#{bin}/yan run #{testpath}/hello.yag")\n'
        "  end\n"
        "end\n"
    )


def package(out: Path, files: list[Path], mtime: int) -> None:
    """Same bytes for the same inputs on any host: sorted names, fixed owner, mode and times."""
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w", format=tarfile.PAX_FORMAT) as tar:
        for f in sorted(files, key=lambda f: f.name):
            info = tarfile.TarInfo(f.name)
            data = f.read_bytes()
            info.size, info.mtime, info.mode = (
                len(data),
                mtime,
                0o755 if os.access(f, os.X_OK) else 0o644,
            )
            tar.addfile(info, io.BytesIO(data))
    with (
        out.open("wb") as raw,
        gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=mtime) as gz,
    ):
        gz.write(buf.getvalue())


def main() -> None:
    p = argparse.ArgumentParser()
    sub = p.add_subparsers(dest="cmd", required=True)
    k = sub.add_parser("package")
    k.add_argument("out", type=Path)
    k.add_argument("files", type=Path, nargs="+")
    f = sub.add_parser("formula")
    f.add_argument("version")
    f.add_argument("sums", type=Path)
    a = p.parse_args()
    if a.cmd == "package":
        package(a.out, a.files, int(os.environ.get("SOURCE_DATE_EPOCH", "0")))
        return
    print(formula(a.version.removeprefix("v"), checksums(a.sums.read_text())), end="")


if __name__ == "__main__":
    main()
