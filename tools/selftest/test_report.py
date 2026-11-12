from pathlib import Path

from tools import lint_reports, report

STATUS = {
    "commit": "dc53c0c" + "0" * 33,
    "date": "2026-11-12",
    "conformance": {
        "executors": ["interp"],
        "milestones": {
            "M0": {"cases": 10, "pass": 8, "fail": 1, "skip": 1},
            "M1": {"cases": 5, "pass": 5, "fail": 0, "skip": 0},
        },
    },
    "differential": {
        "backends": ["interp", "native"],
        "programs": 20,
        "unexplained": 0,
        "allowlisted": 2,
        "refused": 3,
        "stale": 0,
    },
    "spec_debt": {"rows": 100, "rows_without_case": 40, "codes": 50, "codes_unimplemented": 5},
}


def test_render_figures() -> None:
    text = report.render(STATUS, {"Gated": 2, "None": 1}, {"open": 7, "closed": 3})
    assert "| all | 15 | 13 | 1 | 1 |" in text
    assert "13 of 15 (87%)" in text
    assert "Rows without a case: 40 of 100 (40%)" in text
    assert "Gated 2, None 1" in text
    assert "Open: 7, closed: 3" in text


def test_render_passes_lint(tmp_path: Path) -> None:
    path = tmp_path / "2026-11-12-progress.md"
    text = report.render(STATUS, {"Gated": 1}, None)
    path.write_text(text)
    assert lint_reports.lint_report(path, text) == []


def test_laws_counts_real_table() -> None:
    counts = report.laws(report.LAWS.read_text(encoding="utf-8"))
    assert sum(counts.values()) == 16
