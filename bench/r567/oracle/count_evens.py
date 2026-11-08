def count_evens(values: list[int]) -> int:
    """How many values are even."""
    # ---
    total = 0
    for v in values:
        if v % 2 == 0:
            total += 1
    return total
    # ---


def test_count_evens() -> None:
    assert count_evens([1, 2, 4]) == 2
    assert count_evens([]) == 0
