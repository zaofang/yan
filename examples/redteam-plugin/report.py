"""Run every attack in attacks.toml, regenerate audit.txt and the results table in README.md.

uv run python examples/redteam-plugin/report.py [--yan target/debug/yan]
"""

import argparse
import datetime
import shutil
import subprocess
import tempfile
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
BEGIN, END = "<!-- results:begin -->", "<!-- results:end -->"
AUDITED = ["attacks/03-ffi-in-plugin", "attacks/08-link-name"]


def run(yan: Path, src: Path, args: list[str]) -> subprocess.CompletedProcess[str]:
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp) / "w"
        shutil.copytree(src, work, symlinks=True)
        return subprocess.run([yan, *args], cwd=work, capture_output=True, text=True)


def first_line(p: subprocess.CompletedProcess[str]) -> str:
    text = (p.stdout + p.stderr).strip()
    line = text.splitlines()[0] if text else ""
    return line.replace("`", "'").replace("|", "\\|")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--yan", type=Path, default=HERE.parent.parent / "target/debug/yan")
    yan = ap.parse_args().yan.resolve()
    attacks = tomllib.loads((HERE / "attacks.toml").read_text())["attack"]

    rows = ["| # | Attack | Stage | Result | First line of output |", "|---|---|---|---|---|"]
    for a in attacks:
        if not a["dir"]:
            rows.append(
                f"| {a['id']} | {a['name']} | {a['stage']} | **gap** #{a['issue']} | no fixture |"
            )
            continue
        p = run(yan, HERE / a["dir"], a["args"])
        result = "refused" if a["status"] == "refused" else f"**gap** #{a['issue']}"
        rows.append(f"| {a['id']} | {a['name']} | {a['stage']} | {result} | `{first_line(p)}` |")

    audit = []
    for d in AUDITED:
        p = run(yan, HERE / d, ["audit"])
        audit.append(f"$ cd {d} && yan audit\n{p.stdout}")
    (HERE / "audit.txt").write_text("\n".join(audit))

    commit = subprocess.run(
        ["git", "rev-parse", "--short", "HEAD"], cwd=HERE, capture_output=True, text=True
    ).stdout.strip()
    stamp = f"Generated at {commit} on {datetime.date.today()}."
    table = "\n".join([BEGIN, *rows, "", stamp, END])
    readme = (HERE / "README.md").read_text()
    head, rest = readme.split(BEGIN)
    tail = rest.split(END)[1]
    (HERE / "README.md").write_text(head + table + tail)


if __name__ == "__main__":
    main()
