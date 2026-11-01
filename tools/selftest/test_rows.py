from tools.rows import allocations, check, expand


def test_expand_lists_and_ranges() -> None:
    assert expand("R1, R2") == ["R1", "R2"]
    assert expand("R450 to R452; SQ30 to SQ31") == ["R450", "R451", "R452", "SQ30", "SQ31"]


def test_row_allocated_twice_is_an_error() -> None:
    _, errors = allocations([{"rows": "R1 to R3"}, {"rows": "R3"}])
    assert errors == ["R3 allocated by entries 0 and 1 (R3)"]


def test_also_maps_without_allocating() -> None:
    assert allocations([{"rows": "R1"}, {"rows": "R1, R2", "also": ["R1"]}])[1] == []
    assert allocations([{"rows": "R2", "also": ["R9"]}])[1] == [
        "entry 0 (R2) lists R9 in also, but no entry allocates it"
    ]


def test_repository_registry_is_consistent() -> None:
    assert check() == []
