def greatest_common_divisor(a: int, b: int) -> int:
    """Greatest common divisor of a and b."""
    # ---
    x, y = a, b
    while y != 0:
        x, y = y, x % y
    return x
    # ---


def test_gcd() -> None:
    assert greatest_common_divisor(3, 5) == 1
    assert greatest_common_divisor(25, 15) == 5
