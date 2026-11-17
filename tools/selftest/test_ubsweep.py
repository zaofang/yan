from tools.ubsweep import FINDING


def first(stderr: str) -> str | None:
    m = FINDING.search(stderr)
    return m.group(1) if m else None


def test_ub_leak_panic_and_fault_are_findings() -> None:
    assert (
        first("ub(2): read through a freed allocation\n")
        == "ub(2): read through a freed allocation"
    )
    assert first("hi\nleak: 3 allocations, 41 bytes, live when main returned\n")
    assert first("thread 'x' panicked at crates/yan-machine/src/rt.rs:294:9:\n")
    assert first("machine fault: bad block\n")


def test_refusals_and_yan_panics_are_not_findings() -> None:
    assert first("yan: not supported yet: call of `std.fs.sub` in native code\n") is None
    assert first("panic(bounds): index 3 out of range\n") is None
