"""The tile and the social card, from lockup.svg and tokens.json (Workshop Standard L2, L9, L13).

uv run python -m tools.brand    write site/brand/favicon.svg and site/brand/social-card.svg

The PNGs (favicon-32.png, icon-512.png, social-card-1200x630.png) are these SVGs rendered in
Chromium at 1x. Change this script or the tokens, not the outputs.
"""

import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BRAND = ROOT / "site/brand"
# The tile's seal: heavier strokes than the lockup's, drawn for small sizes.
TILE_SEAL = (
    "M 0.00 0.00 L 100.00 0.00 L 100.00 100.00 L 0.00 100.00 Z  "
    "M 5.31 18.45 L 5.31 94.69 L 15.83 94.69 L 15.83 28.98 L 84.17 28.98 L 84.17 94.69 "
    "L 94.69 94.69 L 94.69 18.45 Z  M 18.45 57.88 L 18.45 94.69 L 81.55 94.69 L 81.55 57.88 Z  "
    "M 5.31 5.31 L 5.31 15.83 L 94.69 15.83 L 94.69 5.31 Z  "
    "M 18.45 31.60 L 18.45 42.12 L 81.55 42.12 L 81.55 31.60 Z  "
    "M 18.45 44.74 L 18.45 55.26 L 81.55 55.26 L 81.55 44.74 Z  "
    "M 71.02 84.17 L 28.98 84.17 L 28.98 68.40 L 71.02 68.40 Z  "
    "M 31.60 71.02 L 31.60 81.55 L 68.40 81.55 L 68.40 71.02 Z "
)
BOX = 0.72


def tile(paper: str, seal: str) -> str:
    inset = 100 * (1 - BOX) / 2
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">'
        f'<rect width="100" height="100" fill="{paper}"/>'
        f'<path fill="{seal}" fill-rule="evenodd" transform="translate({inset:g} {inset:g}) '
        f'scale({BOX})" d="{TILE_SEAL}"/></svg>\n'
    )


def social_card(t: dict[str, str]) -> str:
    lock = (BRAND / "lockup.svg").read_text(encoding="utf-8")
    inner = re.sub(r"^<svg[^>]*>|</svg>\s*$", "", lock.strip())
    inner = re.sub(r"<title>.*?</title>", "", inner)
    inner = inner.replace('class="seal"', f'fill="{t["text"]}"')
    inner = inner.replace('class="word" fill="currentColor"', f'fill="{t["text"]}"')
    inner = inner.replace('class="tone"', f'fill="{t["accent"]}"')
    avatar = (BRAND / "zaofang-avatar.svg").read_text(encoding="utf-8").strip()
    avatar = avatar.replace("<svg ", '<svg x="0" y="0" width="28" height="28" ', 1)
    w = 640
    s = w / 298.12
    x, y = (1200 - w) / 2, (630 - 139.38 * s) / 2 - 24
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1200 630" width="1200" height="630">'
        f'<rect width="1200" height="630" fill="{t["bg"]}"/>'
        f'<g transform="translate({x:.2f} {y + 19.69 * s:.2f}) scale({s:.4f})">{inner}</g>'
        f'<g transform="translate(464 536)">{avatar}'
        f'<text x="40" y="20" fill="{t["text-muted"]}" font-family="DM Mono, monospace" '
        'font-size="18">a zaofang project · 造坊出品</text></g></svg>\n'
    )


def main() -> None:
    tokens = json.loads((BRAND / "tokens.json").read_text(encoding="utf-8"))
    light = tokens["light"]
    (BRAND / "favicon.svg").write_text(tile(light["bg"], light["accent"]), encoding="utf-8")
    (BRAND / "social-card.svg").write_text(social_card(tokens["dark"]), encoding="utf-8")


if __name__ == "__main__":
    main()
