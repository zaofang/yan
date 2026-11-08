def is_prime(n: int) -> bool:
    """True if n is a prime number."""
    # ---
    if n < 2:
        return False
    k = 2
    while k * k <= n:
        if n % k == 0:
            return False
        k += 1
    return True
    # ---


def test_is_prime() -> None:
    assert is_prime(1) is False
    assert is_prime(13) is True
    assert is_prime(15) is False
