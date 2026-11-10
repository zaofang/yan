import random
from pathlib import Path

from tools import lens
from tools.conform import ToolFailure

S = lens.Surface("Fake", ".fk", "fk")


class Fake:
    """Hashes a module as its sorted lines; projects by upper-casing, except `bad` lines."""

    name = "fake"

    def __init__(self, getput_bug: str = "", unsupported: str = "") -> None:
        self.bug, self.unsupported = getput_bug, unsupported

    def hash(self, path: Path, reverse: bool = False) -> list[tuple[str, str]] | None:
        lines = [ln.removeprefix("def ") for ln in path.read_text().splitlines() if ln.strip()]
        return sorted((ln.split("(")[0], ln) for ln in lines)

    def view(self, path: Path, surface: str, flags: list[str] | None = None) -> str | None:
        text = path.read_text()
        if self.unsupported and self.unsupported in text:
            raise ToolFailure("yan: not supported yet: x")
        if path.suffix == ".fk":
            return text
        return "".join(
            "def x\n" if self.bug and self.bug in ln else f"def {ln}\n"
            for ln in text.splitlines()
            if ln.strip()
        )

    def canonical(self, path: Path) -> bool:
        return True


def test_generator_is_seeded() -> None:
    assert lens.generate(random.Random(3)) == lens.generate(random.Random(3))


def test_shrink_keeps_only_the_failing_item() -> None:
    items = ["a", "b", "c", "d"]
    assert lens.shrink(items, lambda it: "c" in it) == ["c"]


def test_passes_when_laws_hold() -> None:
    assert lens.check(Fake(), S, 0, 5).status == "pass"


def test_failure_prints_minimal_counterexample() -> None:
    r = lens.check(Fake(getput_bug="f0"), S, 0, 20)
    assert r.status == "fail"
    assert r.detail.startswith("GetPut")
    body = r.detail.split("counterexample:\n")[1]
    assert "f0" in body
    assert body.count("-> int") == 1


def test_unsupported_is_not_a_violation() -> None:
    assert lens.check(Fake(unsupported="f"), S, 0, 5).status == "pass"
