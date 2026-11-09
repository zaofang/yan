# Deterministic concurrency

`par` runs its branches concurrently. Output still appears in source order, and the result never
depends on scheduling, so a parallel program prints the same thing on every run.

```yag
def sq(x: int) -> int:
    return x * x

pub def main() uses io:
    par:
        a = sq(2)
        b = sq(3)
        io.print("one")
        io.print("two")
    ys = par [sq(x) for x in range(5)]
    io.print(f"{a} {b} {ys}")
```

```output
one
two
4 9 [0, 1, 4, 9, 16]
```

Branches cannot see each other's results while they run. Reading a sibling's variable is a race in
other languages; in Yan it is a compile error:

```yag expect=E705
def sq(x: int) -> int:
    return x * x

def f() -> int:
    par:
        a = sq(2)
        b = a + 1
    return b
```
