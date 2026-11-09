import base64
import zlib
from pathlib import Path

from tools.site import Build, layout, pages, render, share_hash


def test_share_hash_round_trips_through_raw_deflate() -> None:
    text = 'pub def main() uses io:\n    io.print("hi")\n'
    lang, data = share_hash("yag", text).split("/")
    raw = base64.urlsafe_b64decode(data + "=" * (-len(data) % 4))
    assert lang == "yag"
    assert zlib.decompress(raw, -15).decode() == text


def test_output_fence_attaches_to_the_program_before_it() -> None:
    page = render("# T\n\n```yag exit=101\nx\n```\n\n```output\n2\n```\n", "tour/t.html", "../")
    [p] = page.programs
    assert (p.exit, p.stdout, page.title) == (101, "2\n", "T")
    assert 'href="../play/#yag/' in page.body


def test_every_page_renders_with_relative_links(tmp_path: Path) -> None:
    build = Build("0" * 40, "2026-01-01", "")
    all_pages, feed = pages(build)
    paths = {p.path for p in all_pages}
    assert {"index.html", "tour/index.html", "status/index.html", "blog/index.html"} <= paths
    for p in all_pages:
        assert 'href="/' not in layout(p, build, "")
    assert feed.startswith("<?xml")
