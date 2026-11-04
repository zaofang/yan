def top_words(text: str, k: int) -> list[tuple[str, int]]:
    counts: dict[str, int] = {}
    for w in text.lower().split():
        counts[w] = counts.get(w, 0) + 1
    return sorted(counts.items(), key=lambda p: (-p[1], p[0]))[:k]


def test_top_words() -> None:
    assert top_words("b a b", 1) == [("b", 2)]
