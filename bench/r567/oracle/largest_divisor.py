def largest_divisor(n: int) -> int:
    """Largest divisor of n smaller than n."""
    # ---
    k = n - 1
    while k > 1:
        if n % k == 0:
            return k
        k -= 1
    return 1
    # ---


def test_largest_divisor() -> None:
    assert largest_divisor(15) == 5
    assert largest_divisor(7) == 1
