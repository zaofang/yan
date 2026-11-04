def filter_by_substring(strings: list[str], substring: str) -> list[str]:
    """Keep the strings that contain substring."""
    # ---
    return [s for s in strings if substring in s]
    # ---


def test_filter() -> None:
    assert filter_by_substring([], "a") == []
    assert filter_by_substring(["abc", "xyz"], "a") == ["abc"]
