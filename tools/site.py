"""Builds the static website (home, tour, playground, status, blog, the terminal page) and checks
its programs.

uv run python -m tools.site build OUT [--url URL] [--no-wasm]
uv run python -m tools.site check --yan target/release/yan
"""

import argparse
import base64
import html
import json
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
import zlib
from collections import Counter
from dataclasses import dataclass, field
from datetime import date
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent.parent
SITE = ROOT / "site"
PLAYGROUND = ROOT / "playground"
PLAYGROUND_FILES = ("index.html", "worker.js", "examples.js")
BACKENDS = ("interp", "cranelift")
SURFACES = {"yag": "Yan Agent", "yn": "Yan", "yir": "Yan Core"}
KEYWORDS = frozenset(
    re.findall(
        r"\S+",
        "def fn pub let var if else elif for in while return match case with uses fail record "
        "data type trait impl loop break continue module cap where and or not true false True "
        "False None import from as par class pass assert pre post ex take unreachable extern "
        "yield lambda is handle resume do then end none",
    )
)
NAV = (("tour/", "Tour"), ("play/", "Playground"), ("status/", "Status"), ("blog/", "Blog"))


@dataclass
class Program:
    lang: str
    source: str
    page: str
    expect: str | None = None
    exit: int = 0
    check_only: bool = False
    stdout: str | None = None


@dataclass
class Page:
    path: str
    title: str
    body: str
    programs: list[Program] = field(default_factory=list)


def share_hash(lang: str, text: str) -> str:
    """The playground's share fragment: surface, then the source as base64url raw deflate."""
    c = zlib.compressobj(9, zlib.DEFLATED, -15)
    data = c.compress(text.encode()) + c.flush()
    return f"{lang}/" + base64.urlsafe_b64encode(data).rstrip(b"=").decode()


def highlight(text: str, lang: str) -> str:
    if lang not in SURFACES and lang != "sh":
        return html.escape(text, quote=False)
    comment = "--" if lang == "yn" else "#"
    token = re.compile(
        rf"({re.escape(comment)}.*$)|(\"(?:[^\"\\\n]|\\.)*\")|(\b\d[\d_]*(?:\.\d+)?\b)|([A-Za-z_]\w*)",
        re.M,
    )
    out, at = [], 0
    for m in token.finditer(text):
        out.append(html.escape(text[at : m.start()], quote=False))
        word = m.group(0)
        if m.group(1):
            cls = "c"
        elif m.group(2):
            cls = "s"
        elif m.group(3):
            cls = "n"
        elif lang != "sh" and word in KEYWORDS:
            cls = "k"
        elif lang != "sh" and word[0].isupper():
            cls = "t"
        else:
            cls = ""
        w = html.escape(word, quote=False)
        out.append(f'<span class="{cls}">{w}</span>' if cls else w)
        at = m.end()
    return "".join(out) + html.escape(text[at:], quote=False)


def inline(text: str) -> str:
    parts = re.split(r"(`[^`]+`)", text)
    out = []
    for i, part in enumerate(parts):
        if i % 2:
            out.append(f"<code>{html.escape(part[1:-1], quote=False)}</code>")
            continue
        s = html.escape(part, quote=False)
        s = re.sub(r"\[([^\]]+)\]\(([^)\s]+)\)", r'<a href="\2">\1</a>', s)
        s = re.sub(r"\*\*(.+?)\*\*", r"<strong>\1</strong>", s)
        out.append(s)
    return "".join(out)


def slug(text: str) -> str:
    return re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")


def program_block(p: Program, root: str) -> str:
    if p.expect:
        note = f'<span class="label rejected">rejected: {p.expect}</span>'
    elif p.exit:
        note = f'<span class="label rejected">exit {p.exit}</span>'
    else:
        note = ""
    link = f"{root}play/#{share_hash(p.lang, p.source)}"
    return (
        f'<figure class="program" data-s="{p.lang}"><div class="bar">'
        f'<span class="name">{SURFACES[p.lang]}</span><span class="ext">.{p.lang}</span>{note}'
        f'<a class="btn" href="{link}">Open in playground</a></div>'
        f'<pre class="code">{highlight(p.source, p.lang)}</pre></figure>'
    )


def render(md: str, path: str, root: str) -> Page:
    """Renders the Markdown subset the site uses: headings, paragraphs, lists, tables, fences,
    and raw HTML blocks (lines starting with `<`)."""
    lines = md.rstrip("\n").split("\n")
    title, out, programs = "", [], []
    last: Program | None = None
    i = 0

    def starts_block(line: str) -> bool:
        return not line.strip() or line.startswith(("#", "```", "<", "- ", "|"))

    while i < len(lines):
        line = lines[i]
        if line.startswith("```"):
            info = line[3:].split()
            j = i + 1
            while not lines[j].startswith("```"):
                j += 1
            text = "\n".join(lines[i + 1 : j]) + "\n"
            i = j + 1
            lang = info[0] if info else ""
            if lang == "output" and last is not None:
                last.stdout = text
                out.append(f'<pre class="output">{html.escape(text, quote=False)}</pre>')
            elif lang in SURFACES:
                attrs = dict(a.split("=", 1) for a in info[1:] if "=" in a)
                last = Program(
                    lang,
                    text,
                    path,
                    expect=attrs.get("expect"),
                    exit=int(attrs.get("exit", 0)),
                    check_only="check" in info[1:],
                )
                programs.append(last)
                out.append(program_block(last, root))
                continue
            else:
                out.append(f'<pre class="code">{highlight(text, lang)}</pre>')
            last = None
            continue
        if not line.strip():
            i += 1
            continue
        last = None
        if line.startswith("#"):
            level = len(line) - len(line.lstrip("#"))
            text = line[level:].strip()
            title = title or text
            out.append(f'<h{level} id="{slug(text)}">{inline(text)}</h{level}>')
            i += 1
        elif line.startswith("<"):
            j = i
            while j < len(lines) and lines[j].strip():
                j += 1
            out.append("\n".join(lines[i:j]))
            i = j
        elif line.startswith("- "):
            items: list[str] = []
            while i < len(lines) and (lines[i].startswith("- ") or lines[i].startswith("  ")):
                if lines[i].startswith("- "):
                    items.append(lines[i][2:])
                else:
                    items[-1] += " " + lines[i].strip()
                i += 1
            out.append("<ul>" + "".join(f"<li>{inline(t)}</li>" for t in items) + "</ul>")
        elif line.startswith("|"):
            table = []
            while i < len(lines) and lines[i].startswith("|"):
                cells = [c.strip() for c in lines[i].strip("|").split("|")]
                if not all(set(c) <= set("-: ") for c in cells):
                    table.append(cells)
                i += 1
            head = "".join(f"<th>{inline(c)}</th>" for c in table[0])
            body = "".join(
                "<tr>" + "".join(f"<td>{inline(c)}</td>" for c in r) + "</tr>" for r in table[1:]
            )
            out.append(f"<table><thead><tr>{head}</tr></thead><tbody>{body}</tbody></table>")
        else:
            j = i + 1
            while j < len(lines) and not starts_block(lines[j]):
                j += 1
            out.append(f"<p>{inline(' '.join(lines[i:j]))}</p>")
            i = j
    return Page(path, title, "\n".join(out), programs)


@dataclass
class Build:
    commit: str
    day: str
    url: str
    status: dict[str, Any] | None = None


def git(*args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout.strip()


def layout(page: Page, build: Build, current: str) -> str:
    root = "../" * page.path.count("/")
    lockup = (SITE / "brand/lockup.svg").read_text().strip()
    nav = "".join(
        f'<a href="{root}{href}"{" aria-current=page" if href == current else ""}>{name}</a>'
        for href, name in NAV
    )
    motion = ' class="motion"' if page.path == "index.html" else ""
    title = "Yan 言" if page.path == "index.html" else f"{page.title} · Yan 言"
    card = f"{build.url}brand/social-card-1200x630.png" if build.url else ""
    meta = f'<meta property="og:image" content="{card}">' if card else ""
    return f"""<!doctype html>
<html lang="en">
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{html.escape(title)}</title>
<meta property="og:title" content="{html.escape(title)}">
{meta}
<link rel="icon" href="{root}brand/favicon.svg" type="image/svg+xml">
<link rel="icon" href="{root}brand/favicon-32.png" sizes="32x32">
<link rel="apple-touch-icon" href="{root}brand/icon-512.png">
<link rel="alternate" type="application/atom+xml" href="{root}blog/feed.xml" title="Yan blog">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Inter+Tight:wght@600;800&family=Inter:wght@400;500;600&family=JetBrains+Mono:wght@400;500;700&family=DM+Mono:wght@400;500&family=Noto+Serif+SC:wght@600&display=swap">
<link rel="stylesheet" href="{root}style.css">
<header{motion}><a class="home" href="{root}">{lockup}</a><nav>{nav}</nav></header>
<main>
{page.body}
</main>
<footer><p>Built from <code>{build.commit[:7]}</code> on {build.day}.
Text CC-BY-4.0, examples 0BSD.</p>
<p class="stamp"><img src="{root}brand/zaofang-avatar.svg" alt="">
a zaofang project · 造坊出品</p></footer>
</html>
"""


def tour_pages() -> list[Page]:
    files = sorted((SITE / "tour").glob("*.md"))
    pages = [render(f.read_text(), f"tour/{f.stem}.html", "../") for f in files]
    for k, p in enumerate(pages):
        prev = f'<a href="{pages[k - 1].path[5:]}">Back: {pages[k - 1].title}</a>' if k else ""
        nxt = (
            f'<a href="{pages[k + 1].path[5:]}">Next: {pages[k + 1].title}</a>'
            if k + 1 < len(pages)
            else '<a href="../play/">Next: the playground</a>'
        )
        p.body = f'<p class="label">Tour {k + 1} of {len(pages)}</p>\n{p.body}'
        p.body += f'\n<nav class="pager">{prev}{nxt}</nav>'
    items = "".join(f'<li><a href="{p.path[5:]}">{p.title}</a></li>' for p in pages)
    index = Page("tour/index.html", "Tour", f"<h1>Tour</h1>\n<ol class=toc>{items}</ol>")
    return [index, *pages]


def blog_pages(build: Build) -> tuple[list[Page], str]:
    posts = []
    for f in sorted((SITE / "blog").glob("*.md"), reverse=True):
        p = render(f.read_text(), f"blog/{f.stem}.html", "../")
        day = f.stem[:10]
        p.body = p.body.replace("</h1>", f'</h1>\n<p class="label">{day}</p>', 1)
        posts.append((day, p))
    items = "".join(
        f'<li><span class="label">{d}</span> <a href="{p.path[5:]}">{p.title}</a></li>'
        for d, p in posts
    )
    index = Page("blog/index.html", "Blog", f"<h1>Blog</h1>\n<ul class=toc>{items}</ul>")
    entries = "".join(
        f'<entry><title>{html.escape(p.title)}</title><link href="{build.url}{p.path}"/>'
        f"<id>{build.url}{p.path}</id><updated>{d}T00:00:00Z</updated>"
        f'<content type="html">{html.escape(p.body)}</content></entry>'
        for d, p in posts
    )
    updated = posts[0][0] if posts else build.day
    feed = (
        '<?xml version="1.0" encoding="utf-8"?>\n<feed xmlns="http://www.w3.org/2005/Atom">'
        f'<title>Yan</title><id>{build.url}blog/</id><link href="{build.url}blog/"/>'
        f"<updated>{updated}T00:00:00Z</updated>{entries}</feed>\n"
    )
    return [index, *(p for _, p in posts)], feed


def table(head: list[str], body: list[list[object]]) -> str:
    th = "".join(f"<th>{h}</th>" for h in head)
    tr = "".join("<tr>" + "".join(f"<td>{c}</td>" for c in r) + "</tr>" for r in body)
    return f"<table><thead><tr>{th}</tr></thead><tbody>{tr}</tbody></table>"


def stamp(status: dict[str, Any]) -> str:
    c = status["commit"]
    return f'<p class="label">CI run on <code>{c[:7]}</code>, {status["date"]}</p>'


def pct(n: int, d: int) -> str:
    return f"{100 * n / d:.0f}%" if d else "-"


def status_page(build: Build) -> Page:
    cases = json.loads((ROOT / "conformance/manifest.json").read_text())["cases"]
    by_surface = Counter(c["path"].split("/")[0] for c in cases)
    rfcs = [f for f in (ROOT / "rfcs").glob("[0-9]*.md") if not f.name.startswith("0000")]
    body = [
        "<h1>Status</h1>",
        f'<p class="label">Site built from <code>{build.commit[:7]}</code> on {build.day}</p>',
    ]
    s = build.status
    if s is None:
        body.append(
            "<p>This build carries no CI status data (built without <code>--status</code>).</p>"
        )
    else:
        conf = s["conformance"]
        body += [
            '<h2 id="conformance">Conformance by milestone</h2>',
            stamp(s),
            f"<p>Executor: {', '.join(conf['executors'])}.</p>",
            table(
                ["Milestone", "Cases", "✓ Pass", "✗ Fail", "○ Skip", "Pass rate"],
                [
                    [m, v["cases"], v["pass"], v["fail"], v["skip"], pct(v["pass"], v["cases"])]
                    for m, v in conf["milestones"].items()
                ],
            ),
        ]
        d = s["differential"]
        body += [
            '<h2 id="differential">Differential run</h2>',
            stamp(s),
            f"<p>{d['programs']} programs on {', '.join(d['backends'][1:])} against the "
            f"{d['backends'][0]} oracle.</p>",
            table(
                ["Unexplained", "Allowlisted", "Refused by a backend", "Stale allowlist"],
                [[d["unexplained"], d["allowlisted"], d["refused"], d["stale"]]],
            ),
        ]
        debt = s["spec_debt"]
        body += [
            '<h2 id="spec-debt">Spec debt</h2>',
            stamp(s),
            table(
                ["Measure", "Count", "Of"],
                [
                    [
                        "Change rows with no conformance case",
                        debt["rows_without_case"],
                        debt["rows"],
                    ],
                    [
                        "Diagnostic codes not implemented",
                        debt["codes_unimplemented"],
                        debt["codes"],
                    ],
                ],
            ),
        ]
        b = s["bench"]
        langs = {"yan": "Yan", "py": "Python", "rs": "Rust"}
        body += [
            '<h2 id="bench">Benchmarks</h2>',
            stamp(s),
            f"<p>Wall time in ms, one run, startup subtracted; indicative only. "
            f"{b['env']['system']} {b['env']['machine']}.</p>",
            table(
                ["Program", *(f"{name} ms" for name in langs.values())],
                [
                    [r["program"], *(r["ms"].get(lang, "-") for lang in langs)]
                    for r in b["programs"]
                ],
            ),
        ]
    body += [
        '<h2 id="repository">Repository</h2>',
        table(
            ["Measure", "Count"],
            [["Conformance cases", len(cases)], ["RFCs", len(rfcs)]]
            + [[f"Cases in <code>{k}/</code>", v] for k, v in sorted(by_surface.items())],
        ),
    ]
    return Page("status/index.html", "Status", "\n".join(body))


def terminal_page(tour: Page, build: Build) -> str:
    """index.txt, served at / to terminals by site/worker.js."""
    c = tomllib.loads((SITE / "terminal.toml").read_text())
    hello = next(p for p in tour.programs if p.stdout is not None)
    col = max(len(x["id"]) for x in c["links"]) + 3
    lines = ["言 yan", "", *c["lines"], "", f"$ cat {c['file']}", hello.source.rstrip("\n"), ""]
    lines += [f"$ yan run {c['file']}", hello.stdout.rstrip("\n"), ""]
    for x in c["links"]:
        url = x.get("url") or build.url + x["path"]
        lines += [x["id"].ljust(col) + x["summary"], " " * col + url]
    lines += ["", f"built from {build.commit[:7]} on {build.day}", "a zaofang project · 造坊出品"]
    return "\n".join(lines) + "\n"


def pages(build: Build) -> tuple[list[Page], str]:
    home = render((SITE / "index.md").read_text(), "index.html", "")
    blog, feed = blog_pages(build)
    return [home, *tour_pages(), status_page(build), *blog], feed


def build(out: Path, url: str, wasm: bool, status: Path | None) -> None:
    wasm_file = PLAYGROUND / "yan.wasm"
    if wasm and not wasm_file.exists():
        sys.exit("playground/yan.wasm is missing: run playground/build.sh, or pass --no-wasm")
    data = json.loads(status.read_text()) if status else None
    info = Build(git("rev-parse", "HEAD"), git("log", "-1", "--format=%cs"), url, data)
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    shutil.copytree(SITE / "brand", out / "brand")
    shutil.copy(SITE / "style.css", out / "style.css")
    (out / "play").mkdir()
    for name in PLAYGROUND_FILES:
        shutil.copy(PLAYGROUND / name, out / "play" / name)
    if wasm:
        shutil.copy(wasm_file, out / "play/yan.wasm")
    all_pages, feed = pages(info)
    for p in all_pages:
        target = out / p.path
        target.parent.mkdir(parents=True, exist_ok=True)
        current = next((h for h, _ in NAV if p.path.startswith(h)), "")
        target.write_text(layout(p, info, current))
    (out / "blog/feed.xml").write_text(feed)
    tour = next(p for p in all_pages if p.path.startswith("tour/0"))
    (out / "index.txt").write_text(terminal_page(tour, info))


def check_program(yan: str, p: Program) -> str | None:
    with tempfile.TemporaryDirectory() as d:
        f = Path(d) / f"main.{p.lang}"
        f.write_text(p.source)
        if p.expect or p.check_only:
            r = subprocess.run(
                [yan, "check", "--format", "json", str(f)], capture_output=True, text=True
            )
            diags = json.loads(r.stdout or "[]")
            codes = {d["code"] for d in diags if d.get("code")}
            errors = {d["code"] for d in diags if d.get("severity") == "error"}
            if p.expect and p.expect not in codes:
                return f"expected {p.expect}, got {sorted(codes) or 'no diagnostics'}"
            if p.check_only and errors:
                return f"expected no errors, got {sorted(errors)}"
            return None
        for backend in BACKENDS:
            cmd = [yan, "run", "--backend", backend, str(f)]
            r = subprocess.run(cmd, capture_output=True, text=True)
            if r.returncode != p.exit:
                return (
                    f"{backend}: exit {r.returncode}, expected {p.exit}: {r.stderr.strip()[:300]}"
                )
            if p.stdout is not None and r.stdout != p.stdout:
                return f"{backend}: stdout {r.stdout!r}, expected {p.stdout!r}"
        return None


def check(yan: str) -> int:
    info = Build("0" * 40, date.today().isoformat(), "")
    failures = 0
    programs = [p for page in pages(info)[0] for p in page.programs]
    for p in programs:
        error = check_program(yan, p)
        if error:
            failures += 1
            print(f"{p.page}: {p.source.splitlines()[0]}: {error}")
    print(f"{len(programs) - failures} of {len(programs)} site programs pass")
    return 1 if failures else 0


def main() -> int:
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("build")
    b.add_argument("out", type=Path)
    b.add_argument("--url", default="", help="absolute site URL ending in /, for feeds and cards")
    b.add_argument("--no-wasm", action="store_true")
    b.add_argument("--status", type=Path, help="status data from tools.status, rendered on status/")
    c = sub.add_parser("check")
    c.add_argument("--yan", required=True)
    args = ap.parse_args()
    if args.cmd == "build":
        build(args.out, args.url, not args.no_wasm, args.status)
        return 0
    return check(args.yan)


if __name__ == "__main__":
    sys.exit(main())
