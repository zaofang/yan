from tools import fuzz


def test_seed_is_deterministic() -> None:
    assert fuzz.generate(7) == fuzz.generate(7)
    assert fuzz.generate(7) != fuzz.generate(8)


def test_program_has_entry() -> None:
    assert "pub def main() uses io:" in fuzz.generate(0)
