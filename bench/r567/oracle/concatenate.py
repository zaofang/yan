def concatenate(strings: list[str]) -> str:
    """Join the strings into one."""
    # ---
    out = ""
    for s in strings:
        out += s
    return out
    # ---


def test_concatenate() -> None:
    assert concatenate([]) == ""
    assert concatenate(["a", "b", "c"]) == "abc"
