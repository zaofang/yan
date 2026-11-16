"""Check the code blocks of the guides against the toolchain.

    uv run python docs/guides/check.py [--yan target/debug/yan] [PAGE.md ...]

Fence conventions (info string after the language):
    yag include=PATH   body must equal the repo-relative file PATH, the source of truth
    yag run            a complete program written in the page
    text out           expected stdout of the closest preceding `include` or `run` block
    yag fail=CODE      a complete program that `yan check` must reject with diagnostic CODE
    yag gap=PATH       body equals the repro PATH, which must still fail `yan check`
Every `include` and `run` program runs on the interpreter, machine and cranelift and under `--ub`;
all four must print the `text out` block and exit 0. Any other fence is prose.
"""

import argparse
import re
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FENCE = re.compile(r"^```(\S*)[ \t]*(.*)$")
RUNS = (
    ("interp", []),
    ("machine", ["--backend", "machine"]),
    ("cranelift", ["--backend", "cranelift"]),
    ("ub", ["--ub"]),
)


def fences(text: str):
    lines, i = text.splitlines(), 0
    while i < len(lines):
        m = FENCE.match(lines[i])
        if not m:
            i += 1
            continue
        start, body = i + 1, []
        i += 1
        while i < len(lines) and not lines[i].startswith("```"):
            body.append(lines[i])
            i += 1
        yield start, m.group(1), m.group(2).strip(), "\n".join(body) + "\n"
        i += 1


def run(yan: str, args: list[str], src: str) -> subprocess.CompletedProcess:
    with tempfile.TemporaryDirectory() as d:
        p = Path(d) / "snippet.yag"
        p.write_text(src)
        return subprocess.run([yan, *args, str(p)], capture_output=True, text=True, timeout=120)


def check_page(yan: str, page: Path) -> list[str]:
    errs: list[str] = []
    prog: tuple[int, str] | None = None
    for line, lang, info, body in fences(page.read_text()):
        where = f"{page.relative_to(ROOT)}:{line}"
        if lang == "yag" and info.startswith("include="):
            path = info.removeprefix("include=")
            src = (ROOT / path).read_text()
            if src.strip() != body.strip():
                errs.append(f"{where}: differs from {path}")
            prog = (line, src)
        elif lang == "yag" and info.startswith("gap="):
            path = info.removeprefix("gap=")
            src = (ROOT / path).read_text()
            if src.strip() != body.strip():
                errs.append(f"{where}: differs from {path}")
            r = run(yan, ["check"], src)
            if r.returncode == 0:
                errs.append(f"{where}: {path} now passes; remove it from the page")
            prog = None
        elif lang == "yag" and info == "run":
            prog = (line, body)
        elif lang == "yag" and info.startswith("fail="):
            code = info.removeprefix("fail=")
            r = run(yan, ["check"], body)
            if code not in r.stdout + r.stderr:
                errs.append(
                    f"{where}: expected {code}, got {(r.stdout + r.stderr).strip()[:120]!r}"
                )
            prog = None
        elif lang == "text" and info == "out":
            if prog is None:
                errs.append(f"{where}: `text out` with no program before it")
                continue
            for name, flags in RUNS:
                r = run(yan, ["run", *flags], prog[1])
                if r.returncode != 0 or r.stdout.strip() != body.strip():
                    head = f"{where}: program at line {prog[0]} on {name}: exit {r.returncode}"
                    errs.append(f"{head}\n{r.stdout}{r.stderr}")
            prog = None
        elif lang == "yag" and info:
            errs.append(f"{where}: unknown fence option {info!r}")
    if prog is not None:
        errs.append(f"{page.relative_to(ROOT)}:{prog[0]}: program with no `text out` block")
    return errs


def main() -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("--yan", default=str(ROOT / "target/debug/yan"))
    ap.add_argument("pages", nargs="*", type=Path)
    a = ap.parse_args()
    pages = [p.resolve() for p in a.pages] or sorted((ROOT / "docs/guides").glob("*.md"))
    errs = [e for p in pages for e in check_page(a.yan, p)]
    print("\n".join(errs) if errs else f"docs: {len(pages)} pages, all blocks ok")
    return 1 if errs else 0


if __name__ == "__main__":
    sys.exit(main())
