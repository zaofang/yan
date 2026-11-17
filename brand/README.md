# Brand assets

All rights reserved by the project lead (spec R357). The logo, seal, icons, lockups, social card and favicons
may be used only as `TRADEMARK.md` allows. They are not under the repository's Apache-2.0 or CC-BY-4.0 licenses.

Provenance: the 言 glyph is drawn from Noto Serif SC (SIL OFL-1.1, which permits glyph outlines in artwork).
Source font version and written confirmation of original or assigned calligraphy (R364) are open items in the
public-release audit (S4), to be settled with the trademark lawyer. The playground loads Noto Serif SC, Inter,
JetBrains Mono and DM Mono (all OFL-1.1) from Google Fonts; self-hosting is tracked as audit item N2.

## Use

Yan follows the Zaofang Workshop Standard: shared laws for the system layer, with Yan's own mark and palette.

- Colour: red is the tone on the á, and actions that run code. In the lockup the seal takes the text colour;
  the red seal appears only as the standalone tile (favicon, app icon, file icons).
- Tokens: `tokens.json` holds both themes in the shared schema (bg, surface, border, text, text-muted,
  accent, on-accent, success, warning, error, info), with syntax and 16-colour ANSI palettes. Every text
  role meets 4.5:1 on its ground.
- Type: Inter Tight for display, Inter for text, JetBrains Mono for code, DM Mono for labels.
- Size: the lockup is never shown narrower than 96 px (24 mm in print); below that, use the tile. Clear
  space is the x-height of `yan` on every side.
- Motion: on the home page the word types out, then the tone arrives. Reduced motion shows it at once.
- Tile: the seal in a 72 percent box on paper, the same in both themes (`favicon.svg`, `icon-512.png`,
  `favicon-32.png`).
- Generated: `favicon.svg` and `social-card.svg` come from `uv run python -m tools.brand`; the PNGs are those
  SVGs rendered in Chromium at 1x. Editor and terminal themes come from `uv run python -m tools.themes`.
- CLI: `yan check` in prose leads each diagnostic with a glyph (✗ error, ▲ warning, ◆ note), opens with
  `言 yan <version>`, ends with a summary line, and colours only on a terminal without `NO_COLOR`.
- Stamp: the site footer and social card carry the Zaofang avatar with "a zaofang project · 造坊出品"
  (`zaofang-avatar.svg`, Zaofang's mark, used only for this stamp).
