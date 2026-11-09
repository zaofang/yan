from tools import conform, status
from tools.site import Build, status_page


def test_conformance_groups_results_by_case_milestone() -> None:
    cases = {
        c.milestone: str(c.path.relative_to(status.ROOT))
        for c in conform.discover(status.CONFORMANCE)
    }
    summary = {
        "executors": ["interp"],
        "results": [
            {"case": cases["M0"], "status": "pass"},
            {"case": cases["M3"], "status": "fail"},
            {"case": cases["M1"], "status": "pass"},
        ],
    }
    got = status.conformance(summary)["milestones"]
    assert list(got) == ["M0", "M1", "M3"]
    assert got["M3"] == {"cases": 1, "pass": 0, "fail": 1, "skip": 0}


def test_bench_subtracts_startup_in_ms() -> None:
    data = {
        "env": {},
        "startup": {"yan": 0.01, "py": 0.02},
        "rows": [{"program": "fib", "times": {"yan": {"median": 0.035}, "py": {"median": 0.01}}}],
    }
    assert status.bench(data)["programs"] == [{"program": "fib", "ms": {"yan": 25.0, "py": 0.0}}]


def test_spec_debt_counts_are_bounded() -> None:
    debt = status.spec_debt()
    assert 0 <= debt["rows_without_case"] < debt["rows"]
    assert 0 <= debt["codes_unimplemented"] < debt["codes"]


def test_status_page_renders_every_section_with_its_commit() -> None:
    data = {
        "commit": "abcdef0123",
        "date": "2026-01-02",
        "conformance": {
            "executors": ["interp"],
            "milestones": {"M0": {"cases": 4, "pass": 3, "fail": 1, "skip": 0}},
        },
        "differential": {
            "backends": ["interp", "cranelift"],
            "programs": 9,
            "unexplained": 0,
            "allowlisted": 2,
            "refused": 1,
            "stale": 0,
        },
        "spec_debt": {"rows": 10, "rows_without_case": 4, "codes": 5, "codes_unimplemented": 1},
        "bench": {"env": {"system": "Linux", "machine": "x86_64"}, "programs": []},
    }
    body = status_page(Build("0" * 40, "2026-01-03", "", data)).body
    for anchor in ("conformance", "differential", "spec-debt", "bench"):
        assert f'id="{anchor}"' in body
    assert body.count("<code>abcdef0</code>, 2026-01-02") == 4
    assert "<td>75%</td>" in body


def test_status_page_says_when_it_has_no_data() -> None:
    assert "no CI status data" in status_page(Build("0" * 40, "2026-01-03", "")).body
