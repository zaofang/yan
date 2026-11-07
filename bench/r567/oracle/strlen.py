def strlen(s: str) -> int:
    """Length of the string."""
    # ---
    return len(s)
    # ---


def test_strlen() -> None:
    assert strlen("") == 0
    assert strlen("abc") == 3
