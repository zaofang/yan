from tools import difftest
from tools.difftest import Outcome


def test_panic_detail_and_frames_are_ignored() -> None:
    a = "panic(overflow): 9 + 1 overflows i64\n  at main x.yir:1:1\n"
    b = "panic(overflow): integer overflow\n"
    assert difftest.diagnostics(a) == difftest.diagnostics(b)


def test_panic_kind_differs() -> None:
    assert difftest.diagnostics("panic(bounds): x\n") != difftest.diagnostics(
        "panic(overflow): x\n"
    )


def test_refusal_is_its_own_field() -> None:
    oracle = Outcome("1\n", 0, "")
    got = Outcome("", 2, "yan: not supported yet: type `X` in native code\n")
    assert [m.field for m in difftest.compare("p", "cranelift", oracle, got)] == ["refused"]


def test_fields_compared() -> None:
    oracle = Outcome("1\n", 0, "")
    got = Outcome("2\n", 1, "")
    assert [m.field for m in difftest.compare("p", "b", oracle, got)] == ["stdout", "exit"]
