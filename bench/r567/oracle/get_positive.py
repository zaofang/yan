def get_positive(values: list[int]) -> list[int]:
    """Keep the values above zero."""
    # ---
    return [v for v in values if v > 0]
    # ---


def test_get_positive() -> None:
    assert get_positive([-1, 2, -4, 5]) == [2, 5]
    assert get_positive([]) == []
