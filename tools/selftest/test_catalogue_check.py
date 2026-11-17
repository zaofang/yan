from tools import catalogue_check as cc

ALLOW: dict[str, list[str]] = {"used_not_catalogued": [], "catalogued_not_used": []}


def test_range_rows_cover_codes() -> None:
    rows, ranges = cc.catalogued("| N01 to N03 | notes |\n| E100 | x |\n")
    assert rows == {"E100"} and ranges == {"N01", "N02", "N03"}


def test_code_missing_from_catalogue_fails() -> None:
    assert cc.check({"E100"}, set(), {"E100", "E101"}, ALLOW) == [
        "E101: used in crates, missing from the catalogue"
    ]


def test_catalogued_code_unused_fails() -> None:
    assert cc.check({"E100", "E101"}, set(), {"E100"}, ALLOW) == [
        "E101: in the catalogue, not used in crates"
    ]


def test_stale_allow_entry_fails() -> None:
    allow = {"used_not_catalogued": ["E100"], "catalogued_not_used": []}
    assert len(cc.check({"E100"}, set(), {"E100"}, allow)) == 1


def test_repo_is_clean() -> None:
    assert cc.main() == 0
