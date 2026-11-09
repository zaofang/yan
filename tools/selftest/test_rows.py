from tools.rows import (
    allocations,
    check,
    code_map_errors,
    expand,
    missing_sections,
    next_code,
    renumbering,
)


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


def test_next_code_follows_the_family_maximum() -> None:
    code = next_code("E3")
    assert code.startswith("E3") and len(code) == 4


def test_unknown_section_is_an_error() -> None:
    entry = {"rows": "R1", "sections": "3.1, 99.9 (note 4.4)"}
    assert missing_sections([entry]) == ["R1: section 99.9 does not exist"]


def test_renumbering_moves_only_colliding_new_entries() -> None:
    base = [{"rows": "R1", "topic": "a"}, {"rows": "R2, R3", "topic": "b"}]
    assert renumbering(base, [*base, {"rows": "R3 to R4", "topic": "c"}]) == {
        "R3": "R5",
        "R4": "R6",
    }
    assert renumbering(base, [*base, {"rows": "R4", "topic": "c"}]) == {}


def test_code_map_agrees_with_spec_and_catalogue() -> None:
    assert code_map_errors() == []
