def is_sorted(xs: list[int]) -> bool:
    """True when xs is in non-decreasing order."""
    return all(a <= b for a, b in zip(xs, xs[1:], strict=False))


def test_is_sorted() -> None:
    assert is_sorted([1, 2, 2]) is True
    assert is_sorted([2, 1]) is False
