from datetime import date
from pathlib import Path

from tools.lint_reports import lint_readme, lint_report, stale


def test_report_needs_commit_and_date() -> None:
    p = Path("2026-11-04-x.md")
    assert lint_report(p, "# T\n\nMain at `5bfd39c`.\n") == []
    assert len(lint_report(Path("x.md"), "# T\n\nno facts\n")) == 2
    assert lint_report(Path("x.md"), "# T\n\nMain at `5bfd39c`, 2026-11-04.\n") == []


def test_plain_words_are_not_commits() -> None:
    assert len(lint_report(Path("2026-11-04-x.md"), "# T\n\ndecade defaced 12345678\n")) == 1


def test_readme_figure_needs_commit_and_date_on_the_line() -> None:
    assert lint_readme(Path("R.md"), "492 of 544 pass\n") != []
    assert lint_readme(Path("R.md"), "492 of 544 pass at `5bfd39c` on 2026-11-04\n") == []
    assert lint_readme(Path("R.md"), "| a | -27% |\n") == []


def test_stale_after_seven_days() -> None:
    p = Path("2026-11-04-x.md")
    assert not stale(p, "", date(2026, 11, 11))
    assert stale(p, "", date(2026, 11, 12))
