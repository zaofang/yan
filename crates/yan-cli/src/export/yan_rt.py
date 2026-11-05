# yan runtime shim (17.8): what Python lacks for exported Yan Agent programs.
import sys
from collections.abc import Callable
from dataclasses import dataclass
from typing import NoReturn


class YanFail(Exception):
    pass


@dataclass
class FsError(YanFail):
    path: str
    reason: str


@dataclass
class ParseError(YanFail):
    text: str


class _Io:
    def print(self, text: object) -> None:
        print(text)


io = _Io()


class _FakeFs:
    def __init__(self, files: dict[str, str]) -> None:
        self.files = files

    def read(self, path: str) -> str:
        if path not in self.files:
            raise FsError(path=path, reason="not found")
        return self.files[path]


def fake_fs(files: dict[str, str]) -> _FakeFs:
    return _FakeFs(files)


def copy[T](x: T) -> T:
    return x.copy() if hasattr(x, "copy") else x


def parse_int(s: str) -> int:
    try:
        return int(s)
    except ValueError:
        raise ParseError(s) from None


def parse_float(s: str) -> float:
    try:
        return float(s)
    except ValueError:
        raise ParseError(s) from None


def raise_(e: BaseException) -> NoReturn:
    raise e


class _Missing:
    def __init__(self, err: YanFail | None) -> None:
        self.err = err


def attempt[T](f: Callable[[], T | None]) -> T | _Missing:
    try:
        v = f()
    except YanFail as e:
        return _Missing(e)
    return _Missing(None) if v is None else v


def else_[T](f: Callable[[], T | None], g: Callable[[YanFail | None], T]) -> T:
    v = attempt(f)
    return g(v.err) if isinstance(v, _Missing) else v


def run_main(f: Callable[[], object]) -> None:
    try:
        f()
    except YanFail as e:
        print(f"fail: {e}", file=sys.stderr)
        sys.exit(1)
