# Values and ownership

Values are values: assigning or passing a list never shares it behind your back. A function that
consumes its argument says `take`, and the caller hands over a copy when it wants to keep its own.

```yag
def grow(take xs: list[int]) -> list[int]:
    xs.append(9)
    return xs

pub def main() uses io:
    xs = [1, 2, 3]
    ys = grow(xs.copy())
    io.print(f"{len(xs)} {len(ys)}")
```

```output
3 4
```

To let a callee change a variable in place, declare the parameter `var` and pass it with `&`:

```yag
def bump(var n: int) -> None:
    n += 1

pub def main() uses io:
    a = 1
    bump(&a)
    bump(&a)
    io.print(f"{a}")
```

```output
3
```
