def below_zero(operations: list[int]) -> bool:
    """True if the running balance ever drops below zero."""
    # ---
    balance = 0
    for op in operations:
        balance += op
        if balance < 0:
            return True
    return False
    # ---


def test_below_zero() -> None:
    assert below_zero([1, 2, -4, 5]) is True
    assert below_zero([1, 2]) is False
