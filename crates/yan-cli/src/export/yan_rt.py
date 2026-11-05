# yan runtime shim (17.8): what Python lacks for exported Yan Agent programs.
import math as _m
import sys
from bisect import bisect_left, bisect_right, insort
from collections.abc import Callable, Iterable, Iterator
from dataclasses import dataclass
from itertools import chain as _chain, count as _count, islice as _islice, repeat as _repeat
from itertools import dropwhile as _dropwhile, takewhile as _takewhile
from types import SimpleNamespace
from typing import NoReturn


class YanFail(Exception):
    pass


@dataclass
class FsError(YanFail):
    path: str
    reason: str


@dataclass
class NotFound(YanFail):
    value: object


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


def byte_len(s: str) -> int:
    return len(s.encode())


def chars(s: str) -> list[str]:
    return list(s)


def index_of[T](xs: list[T], x: T) -> int:
    try:
        return xs.index(x)
    except ValueError:
        raise NotFound(x) from None


def slice_exact[T](xs: list[T], lo: int, hi: int) -> list[T]:
    if not 0 <= lo <= hi <= len(xs):
        raise IndexError(f"slice {lo}..{hi} out of range for length {len(xs)}")
    return xs[lo:hi]


class Iter[T]:
    def __init__(self, it: Iterable[T]) -> None:
        self._it: Iterator[T] = _iter(it)

    def __iter__(self) -> Iterator[T]:
        return self._it

    def next(self) -> T | None:
        return next(self._it, None)

    def collect(self) -> list[T]:
        return list(self._it)

    def count(self) -> int:
        return sum(1 for _ in self._it)

    def sum(self) -> T:
        return sum(self._it)

    def fold[A](self, init: A, f: Callable[[A, T], A]) -> A:
        for x in self._it:
            init = f(init, x)
        return init

    def any(self, f: Callable[[T], bool]) -> bool:
        return any(f(x) for x in self._it)

    def all(self, f: Callable[[T], bool]) -> bool:
        return all(f(x) for x in self._it)

    def find(self, f: Callable[[T], bool]) -> T | None:
        return next((x for x in self._it if f(x)), None)

    def min(self, key: Callable[[T], object] | None = None) -> T | None:
        xs = list(self._it)
        return min(xs, key=key) if xs else None

    def max(self, key: Callable[[T], object] | None = None) -> T | None:
        xs = list(self._it)
        return max(xs, key=key) if xs else None

    def map[U](self, f: Callable[[T], U]) -> Iter[U]:
        return Iter(map(f, self._it))

    def filter(self, f: Callable[[T], bool]) -> Iter[T]:
        return Iter(filter(f, self._it))

    def take(self, n: int) -> Iter[T]:
        return Iter(_islice(self._it, n))

    def skip(self, n: int) -> Iter[T]:
        return Iter(_islice(self._it, n, None))

    def step_by(self, n: int) -> Iter[T]:
        return Iter(_islice(self._it, 0, None, n))

    def take_while(self, f: Callable[[T], bool]) -> Iter[T]:
        return Iter(_takewhile(f, self._it))

    def drop_while(self, f: Callable[[T], bool]) -> Iter[T]:
        return Iter(_dropwhile(f, self._it))

    def enumerate(self) -> Iter[tuple[int, T]]:
        return Iter(enumerate(self._it))

    def zip[U](self, other: Iter[U]) -> Iter[tuple[T, U]]:
        return Iter(zip(self._it, other._it))

    def chain(self, other: Iter[T]) -> Iter[T]:
        return Iter(_chain(self._it, other._it))


_iter = iter


def iter_of[T](xs: Iterable[T]) -> Iter[T]:
    return Iter(xs)


iter = SimpleNamespace(
    count=lambda start=0, step=1: Iter(_count(start, step)),
    repeat=lambda x, n=None: Iter(_repeat(x) if n is None else _repeat(x, n)),
    once=lambda x: Iter([x]),
    empty=lambda: Iter([]),
)


def _total(f: Callable[..., float]) -> Callable[..., float]:
    # IEEE results instead of Python's domain and overflow exceptions.
    def g(*a: float) -> float:
        try:
            return f(*a)
        except ValueError:
            return _m.nan
        except OverflowError:
            return _m.inf

    return g


def _integral(f: Callable[[float], int]) -> Callable[[float], float]:
    return lambda x: float(f(x)) if _m.isfinite(x) else x


def _log(x: float, base: float | None = None) -> float:
    if x == 0:
        return -_m.inf
    return _total(_m.log)(*((x,) if base is None else (x, base)))


def _tgamma(x: float) -> float:
    return _m.copysign(_m.inf, x) if x == 0 else _total(_m.gamma)(x)


def _powr(x: float, y: float) -> float:
    if x < 0 or (x == 1 and _m.isinf(y)):
        return _m.nan
    return _total(_m.pow)(x, y)


math = SimpleNamespace(
    pi=_m.pi,
    e=_m.e,
    inf=_m.inf,
    nan=_m.nan,
    is_inf=_m.isinf,
    is_finite=_m.isfinite,
    is_nan=_m.isnan,
    sqrt=_total(_m.sqrt),
    log=_log,
    log2=_total(_m.log2),
    log10=_total(_m.log10),
    exp=_total(_m.exp),
    sin=_total(_m.sin),
    cos=_total(_m.cos),
    tan=_total(_m.tan),
    acos=_total(_m.acos),
    atan2=_m.atan2,
    hypot=_m.hypot,
    pow=_total(_m.pow),
    powr=_powr,
    tgamma=_tgamma,
    abs=abs,
    floor=_integral(_m.floor),
    ceil=_integral(_m.ceil),
    trunc=_integral(_m.trunc),
    fmod=_m.fmod,
    remainder=_m.remainder,
    copysign=_m.copysign,
    frexp=_m.frexp,
    ldexp=_m.ldexp,
    next_up=lambda x: _m.nextafter(x, _m.inf),
    next_down=lambda x: _m.nextafter(x, -_m.inf),
)


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
