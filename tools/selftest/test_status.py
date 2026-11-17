from tools import conform, status


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
