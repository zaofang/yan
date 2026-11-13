import json

from tools.themes import TOKENS, main

ROLES = ["text", "text-muted", "success", "warning", "error", "info"]


def lum(hex_: str) -> float:
    def ch(c: float) -> float:
        return c / 12.92 if c <= 0.03928 else ((c + 0.055) / 1.055) ** 2.4

    r, g, b = (ch(int(hex_[i : i + 2], 16) / 255) for i in (1, 3, 5))
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def contrast(a: str, b: str) -> float:
    hi, lo = sorted((lum(a), lum(b)), reverse=True)
    return (hi + 0.05) / (lo + 0.05)


def test_generated_themes_match_the_tokens() -> None:
    assert main(["--check"]) == 0


def test_every_text_role_meets_4_5_to_1() -> None:
    tokens = json.loads(TOKENS.read_text(encoding="utf-8"))
    for mode in ("dark", "light"):
        t = tokens[mode]
        for role in ROLES:
            assert contrast(t[role], t["bg"]) >= 4.5, (mode, role)
        assert contrast(t["on-accent"], t["accent"]) >= 4.5, mode
