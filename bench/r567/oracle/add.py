def add(x: int, y: int) -> int:
    """Sum of two integers."""
    # ---
    return x + y
    # ---


def test_add() -> None:
    assert add(2, 3) == 5
    assert add(-1, 1) == 0
