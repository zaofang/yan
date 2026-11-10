from tools.changelog import prs, render, row_ids

LOG = (
    "Merge pull request #7 from o/fix/drop-order\nFix drop order\n\x00"
    "Merge pull request #3 from o/conform/actors\nconform: actors\n\x00"
    "Merge remote-tracking branch 'origin/main' into x\n\x00"
    "Merge pull request #5 from o/misc\nTidy\n\x00"
)


def test_prs_groups_by_branch_prefix_and_skips_other_merges() -> None:
    assert prs(LOG) == [
        ("Fixes", 7, "Fix drop order"),
        ("Conformance", 3, "conform: actors"),
        ("Other", 5, "Tidy"),
    ]


def test_row_ids_expands_ranges() -> None:
    text = '[[entry]]\nrows = "R1, R450 to R452"\n'
    assert row_ids(text) == {1, 450, 451, 452}


def test_render_orders_sections_and_lists_rows() -> None:
    text = render(prs(LOG), {9, 8})
    assert text.index("## Fixes") < text.index("## Conformance") < text.index("## Other")
    assert "- Fix drop order (#7)" in text
    assert text.rstrip().endswith("R8, R9")
