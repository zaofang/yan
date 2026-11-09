# Functions, examples and contracts

Parameters and results are typed. A public function carries at least one example, an `ex` line the
checker runs, so documentation cannot drift from behaviour. `pre` states what the caller must
guarantee; a call that breaks it panics.

```yag
pub def clamp(x: int, lo: int, hi: int) -> int:
    pre lo <= hi
    ex clamp(5, 0, 3) == 3
    ex clamp(-1, 0, 3) == 0
    if x < lo:
        return lo
    if x > hi:
        return hi
    return x

pub def main() uses io:
    io.print(f"{clamp(7, 0, 5)} {clamp(2, 0, 5)}")
```

```output
5 2
```

A broken precondition stops the program with exit status 101 at the call that broke it:

```yag exit=101
def half(x: int) -> int:
    pre x % 2 == 0
    return x // 2

pub def main() uses io:
    io.print(f"{half(4)}")
    io.print(f"{half(3)}")
```

```output
2
```
