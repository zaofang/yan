# ruff: noqa: E501
"""Hierarchical progress view rolled up from GitHub issues (the source of truth).

uv run python -m tools.progress_tree --html progress.html [--json progress.json]

Hierarchy: all -> group:P1..P10 (label) -> subgroup (bold heading in the TRACK-Pn body) -> issue.
Leaves are non-tracking issues carrying a group label; progress is closed/total from issue state,
never from checkboxes. Issues with a group label that no tracker body lists go to "Unlisted".
Uses the REST API only (token from GH_TOKEN or GITHUB_TOKEN).
"""

import argparse
import html
import json
import os
import re
import urllib.request
from datetime import UTC, datetime
from typing import Any

REPO = "zaofang/yan"
API = f"https://api.github.com/repos/{REPO}"
OWN_TRACKER = {"P4": 851, "P5": 752, "P6": 1119}
HEADING = re.compile(r"^\*\*(.+?)\*\*\s*$")
REF = re.compile(r"#(\d+)")


def get(path: str) -> Any:
    req = urllib.request.Request(f"{API}{path}")
    token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
    if token:
        req.add_header("Authorization", f"Bearer {token}")
    with urllib.request.urlopen(req) as resp:
        return json.load(resp)


def fetch_issues(label: str | None = None) -> list[dict[str, Any]]:
    out, page = [], 1
    while True:
        q = f"/issues?state=all&per_page=100&page={page}" + (f"&labels={label}" if label else "")
        batch = get(q)
        out += [i for i in batch if "pull_request" not in i]
        if len(batch) < 100:
            return out
        page += 1


def labels(issue: dict[str, Any]) -> set[str]:
    return {lab["name"] for lab in issue["labels"]}


def subgroups(body: str) -> dict[str, list[int]]:
    groups: dict[str, list[int]] = {}
    current = "General"
    for line in (body or "").splitlines():
        if m := HEADING.match(line.strip()):
            current = m.group(1)
        elif line.lstrip().startswith("- ["):
            groups.setdefault(current, []).extend(int(n) for n in REF.findall(line)[:1])
    return groups


def tally(nums: list[int], issues: dict[int, dict[str, Any]]) -> dict[str, Any]:
    items = [issues[n] for n in nums if n in issues]
    return {
        "done": sum(i["state"] == "closed" for i in items),
        "total": len(items),
        "issues": [
            {"n": i["number"], "title": i["title"], "closed": i["state"] == "closed"} for i in items
        ],
    }


def build() -> dict[str, Any]:
    all_issues = fetch_issues()
    by_num = {i["number"]: i for i in all_issues}
    trackers = {
        g: i
        for i in all_issues
        if re.match(r"TRACK-P\d+ ", i["title"])
        for g in labels(i)
        if re.fullmatch(r"group:P\d+", g)
    }
    trackers |= {f"group:{g}": by_num[n] for g, n in OWN_TRACKER.items()}
    tracker_nums = {t["number"] for t in trackers.values()}
    leaves = {
        n: i
        for n, i in by_num.items()
        if n not in tracker_nums
        and "kind:tracking" not in labels(i)
        and any(lab.startswith("group:P") for lab in labels(i))
    }
    groups = []
    for g in sorted(trackers, key=lambda s: int(s.split("P")[1])):
        tracker = trackers[g]
        members = {n for n, i in leaves.items() if g in labels(i)}
        subs, seen = [], set()
        listed = subgroups(tracker["body"])
        if g.removeprefix("group:") in OWN_TRACKER:
            kids = [
                c["number"] for c in get(f"/issues/{tracker['number']}/sub_issues?per_page=100")
            ]
            listed = {f"Sub-issues of #{tracker['number']}": kids}
        for name, nums in listed.items():
            nums = [n for n in dict.fromkeys(nums) if n in members and n not in seen]
            seen.update(nums)
            if nums:
                subs.append({"name": name, **tally(nums, by_num)})
        if rest := sorted(members - seen):
            subs.append({"name": "Unlisted in tracker", **tally(rest, by_num)})
        done = sum(s["done"] for s in subs)
        total = sum(s["total"] for s in subs)
        groups.append(
            {
                "group": g.removeprefix("group:"),
                "tracker": tracker["number"],
                "title": tracker["title"].removeprefix("TRACK-").split(" ", 1)[-1],
                "done": done,
                "total": total,
                "subgroups": subs,
            }
        )
    return {
        "repo": REPO,
        "generated": datetime.now(UTC).strftime("%Y-%m-%d %H:%MZ"),
        "done": sum(g["done"] for g in groups),
        "total": sum(g["total"] for g in groups),
        "groups": groups,
    }


def bar(done: int, total: int, cls: str = "") -> str:
    pct = 100 * done // total if total else 0
    return (
        f'<span class="bar {cls}"><i style="width:{pct}%"></i></span>'
        f'<span class="num">{done}/{total} · {pct}%</span>'
    )


STYLE = """
:root{--bg:#fafaf7;--fg:#1c1b19;--mut:#76736b;--track:#e4e1d8;--fill:#3b7a57;--line:#d8d4c8}
@media (prefers-color-scheme:dark){:root:not([data-theme=light]){--bg:#171715;--fg:#ece9e0;--mut:#99958b;--track:#33312c;--fill:#6dbf8f;--line:#3a3833}}
:root[data-theme=dark]{--bg:#171715;--fg:#ece9e0;--mut:#99958b;--track:#33312c;--fill:#6dbf8f;--line:#3a3833}
body{background:var(--bg);color:var(--fg);font:15px/1.45 system-ui,sans-serif;padding-block:24px;padding-left:max(16px,env(safe-area-inset-left,0px));padding-right:max(16px,env(safe-area-inset-right,0px))}
main{max-width:860px;margin:0 auto}h1{font-size:22px;margin:0 0 4px}.meta{color:var(--mut);font-size:13px;margin-bottom:20px}
.row{display:flex;align-items:center;gap:10px;flex-wrap:wrap}.row .name{flex:1 1 220px;min-width:0}
.bar{flex:0 0 180px;height:8px;background:var(--track);border-radius:4px;overflow:hidden;display:inline-block}
.bar i{display:block;height:100%;background:var(--fill)}.num{flex:0 0 96px;font-variant-numeric:tabular-nums;font-size:13px;color:var(--mut);text-align:right}
.total{padding:12px 0;border-bottom:2px solid var(--line);font-weight:600}
details{border-bottom:1px solid var(--line);padding:8px 0}summary{cursor:pointer;list-style:none}
.subs{margin:8px 0 4px 18px}.sub{padding:4px 0;font-size:14px}.sub .bar{height:6px;flex-basis:140px}
ul{margin:4px 0 8px 18px;padding:0;font-size:13px;color:var(--mut)}li.c{text-decoration:line-through}
a{color:inherit}
"""


def render(data: dict[str, Any]) -> str:
    rows_html = []
    for g in data["groups"]:
        subs = "".join(
            f'<details class="sub"><summary class="row"><span class="name">{html.escape(s["name"])}</span>'
            f"{bar(s['done'], s['total'])}</summary><ul>"
            + "".join(
                f'<li class="{"c" if i["closed"] else ""}"><a href="https://github.com/{data["repo"]}/issues/{i["n"]}">'
                f"#{i['n']}</a> {html.escape(i['title'])}</li>"
                for i in s["issues"]
            )
            + "</ul></details>"
            for s in g["subgroups"]
        )
        rows_html.append(
            f'<details><summary class="row"><span class="name"><b>{g["group"]}</b> {html.escape(g["title"])} '
            f'<a href="https://github.com/{data["repo"]}/issues/{g["tracker"]}">#{g["tracker"]}</a></span>'
            f'{bar(g["done"], g["total"])}</summary><div class="subs">{subs}</div></details>'
        )
    return (
        '<meta name="claude:viewport-fit-sides" content="cover">'
        f"<title>Yan Progress Tree</title><style>{STYLE}</style><main>"
        f'<h1>Yan progress tree</h1><div class="meta">{data["repo"]} issues, closed/total per group. '
        f"Generated {data['generated']}. Regenerate: uv run python -m tools.progress_tree --html progress.html</div>"
        f'<div class="total row"><span class="name">All groups (#449)</span>{bar(data["done"], data["total"])}</div>'
        + "".join(rows_html)
        + "</main>"
    )


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--html", required=True)
    ap.add_argument("--json")
    args = ap.parse_args()
    data = build()
    with open(args.html, "w") as f:
        f.write(render(data))
    if args.json:
        with open(args.json, "w") as f:
            json.dump(data, f, indent=1)
    print(f"{data['done']}/{data['total']} across {len(data['groups'])} groups")


if __name__ == "__main__":
    main()
