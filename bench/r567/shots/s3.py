def second_largest(xs: list[int]) -> int | None:
    """Second largest distinct value of xs, or None."""
    vals = sorted(set(xs))
    if len(vals) < 2:
        return None
    return vals[-2]


def test_second_largest() -> None:
    assert second_largest([3, 1, 3, 2]) == 2
    assert second_largest([5, 5]) is None
