def diffs(xs: list[int]) -> list[int]:
    """Differences between consecutive elements."""
    out: list[int] = []
    for i in range(1, len(xs)):
        out.append(xs[i] - xs[i - 1])
    return out


def test_diffs() -> None:
    assert diffs([1, 4, 9]) == [3, 5]
