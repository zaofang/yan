"""One-off: rename conformance/ to the Yan names (R774), on top of the agent rename PR.

Agents never edit conformance/, so the owner runs this on a branch of their own:

    python3 tools/rename_yan.py

It moves the case files, rewrites their text, regenerates manifest.json and COVERAGE.md, stages
everything and deletes itself. The same rules renamed the rest of the repository.
"""

import pathlib
import re
import subprocess

HIST = "兰亭集序"
CORE_PHRASES = (
    r"text|IR text|item|items|tree|type|types|checker|form|statements?|productions?|spellings?"
    r"|names|elaboration|signature|module"
)
CORE_BEFORE = (
    r"checked|Checked|elaborated|optimized|Optimized|canonical|valid|accepted|to|as|into|unmarked"
    r"|typed|stable"
)
SURF = {"yong": "yir", "xin": "yag", "wen": "yn"}


def git(*a: str) -> str:
    return subprocess.run(["git", *a], check=True, capture_output=True, text=True).stdout


def line(s: str, code: bool) -> str:
    if HIST in s:
        return s
    # Paths, crates, extensions.
    s = s.replace(".yong/", ".yan/")
    s = re.sub(r"\byong([-_])wen\b", r"yan\1yn", s)
    s = re.sub(r"\byong([-_])xin\b", r"yan\1yag", s)
    s = re.sub(r"\bYongK\b", "YanCore", s)
    s = re.sub(r"\byongk\b", "yancore", s)
    s = re.sub(r"\bYONGK\b", "YANCORE", s)
    s = re.sub(r"\.yong\b", ".yir", s)
    s = re.sub(r"\.wen\b", ".yn", s)
    s = re.sub(r"\.xin\b", ".yag", s)
    s = re.sub(r"\.chang\b", ".ynt", s)
    s = re.sub(r"\bconformance/yong\b", "conformance/yir", s)
    s = re.sub(r"\bconformance/xin\b", "conformance/yag", s)
    s = re.sub(r"\bconformance/wen\b", "conformance/yn", s)
    s = re.sub(
        r"(?<![\w/.-])(yong|xin|wen)/(pass|fail|run)\b", lambda m: SURF[m[1]] + "/" + m[2], s
    )
    s = re.sub(r"\byong-", "yan-", s)
    s = re.sub(r"\byong_", "yan_", s)
    s = re.sub(r"\bYONG_", "YAN_", s)
    s = re.sub(r"@yong/", "@yan/", s)
    # Code identifiers and surface tokens.
    s = re.sub(r"::Yong\b", "::Yir", s)
    s = re.sub(r"::Xin\b", "::Yag", s)
    s = re.sub(r"::Wen\b", "::Yn", s)
    s = re.sub(r'(name"?\s*[:=]\s*)"yong"', r'\1"yan"', s)
    s = re.sub(r'"yong"', '"yir"', s)
    s = re.sub(r'"xin"', '"yag"', s)
    s = re.sub(r'"wen"', '"yn"', s)
    s = re.sub(r"--as (xin|wen|yong)\b", lambda m: "--as " + SURF[m[1]], s)
    s = re.sub(r"\byong(?= (M0|fail|pass|run|ub)\b)", "yir", s)
    if code:
        s = re.sub(r"(?<=_)xin\b|\bxin(?=_)|(?<=_)xin(?=_)", "yag", s)
        s = re.sub(r"(?<=_)wen\b|\bwen(?=_)|(?<=_)wen(?=_)", "yn", s)
        s = re.sub(r"(?<=_)yong\b|(?<=_)yong(?=_)", "yir", s)
        s = re.sub(r"\bxin\b", "yag", s)
        s = re.sub(r"\bwen\b", "yn", s)
    s = s.replace("永", "言")
    # Prose: the core where it stands beside a surface, or in a core phrase.
    s = re.sub(r"\bYong IR\b", "Yan IR", s)
    if re.search(r"\b(Xin|Wen|Chang)\b", s):
        s = re.sub(r"\bYong\b", "Yan Core", s)
    s = re.sub(rf"\bYong (?=({CORE_PHRASES})\b)", "Yan Core ", s)
    s = re.sub(rf"\b({CORE_BEFORE}) Yong\b", r"\1 Yan Core", s)
    s = re.sub(r"\bYong\b", "Yan", s)
    s = re.sub(r"\bYONG\b", "YAN", s)
    s = re.sub(r"\byong\b", "yan", s)
    s = re.sub(r"\bWen\b", "Yan", s)
    s = re.sub(r"\bXin\b", "Yan Agent", s)
    s = re.sub(r"\bChang\b", "Yan Notation", s)
    s = re.sub(r"\bWEN\b", "YN", s)
    s = re.sub(r"\bXIN\b", "YAG", s)
    s = s.replace("Yan Core Core", "Yan Core").replace("Yan Agent Agent", "Yan Agent")
    return s


def new_path(f: str) -> str:
    p = re.sub(r"^conformance/(yong|xin|wen)/", lambda m: f"conformance/{SURF[m[1]]}/", f)
    return re.sub(r"\.(yong|xin|wen)$", lambda m: "." + SURF[m[1]], p)


def main() -> None:
    files = [f for f in git("ls-files", "conformance", "tests").splitlines()]
    for f in files:
        p = pathlib.Path(f)
        try:
            s = p.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        t = "\n".join(line(x, True) for x in s.split("\n"))
        if t != s:
            p.write_text(t, encoding="utf-8")
    for f in files:
        n = new_path(f)
        if n != f:
            pathlib.Path(n).parent.mkdir(parents=True, exist_ok=True)
            git("mv", f, n)
    for d in sorted(pathlib.Path("conformance").rglob("*"), reverse=True):
        if d.is_dir() and not any(d.iterdir()):
            d.rmdir()
    subprocess.run(
        [
            "uv",
            "run",
            "python",
            "tools/conform.py",
            "--manifest",
            "conformance/manifest.json",
            "--coverage",
            "conformance/COVERAGE.md",
        ],
        check=True,
    )
    git("add", "-A", "conformance")
    git("rm", "-q", "tools/rename_yan.py")


if __name__ == "__main__":
    main()
