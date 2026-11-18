from tools.rows import (
    allocations,
    case_with_row_errors,
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


def test_case_with_row_flags_uncased_row_above_freeze_exit() -> None:
    cased = {
        "R1009": 0,
        "R999999": 1,
        "R1017": 2,
        "R1020": 3,
        "R1042": 4,
        "R1071": 5,
        "R1080": 6,
        "R1081": 7,
    }
    errors = case_with_row_errors(cased)
    assert [e.split()[0] for e in errors] == ["R999999"]


def test_case_with_row_flags_stale_allowance() -> None:
    errors = case_with_row_errors({})
    assert all("remove it" in e for e in errors) and len(errors) == 6


def test_added_rows_are_head_rows_missing_from_base(monkeypatch) -> None:  # type: ignore[no-untyped-def]
    from tools import rows

    monkeypatch.setattr(rows, "base_entries", lambda ref: [{"rows": "R1", "topic": "a"}])
    monkeypatch.setattr(
        rows, "load", lambda: [{"rows": "R1", "topic": "a"}, {"rows": "R2 to R3", "topic": "b"}]
    )
    assert rows.added_rows("main") == {"R2", "R3"}
