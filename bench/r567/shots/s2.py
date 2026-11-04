from dataclasses import dataclass

import pytest


@dataclass
class BadLine(Exception):
    line: int
    msg: str


@dataclass
class DupKey(Exception):
    key: str


def parse_config(src: str) -> dict[str, int]:
    out: dict[str, int] = {}
    for n, raw in enumerate(src.splitlines(), start=1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        key, sep, val = line.partition("=")
        if not sep:
            raise BadLine(n, "missing =")
        key = key.strip()
        if key in out:
            raise DupKey(key)
        try:
            out[key] = int(val.strip())
        except ValueError:
            raise BadLine(n, "bad int") from None
    return out


def test_parse_config() -> None:
    assert parse_config("a = 1\n# c\nb=2") == {"a": 1, "b": 2}
    with pytest.raises(DupKey):
        parse_config("a=1\na=2")
