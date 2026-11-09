# Failure

A function that can fail says so in its signature with `fail`. There are no unchecked exceptions:
the caller either handles the failure with `else`, or passes it on with `?` and declares it too.

```yag
class Bad:
    code: int

def check(x: int) -> int fail Bad:
    if x < 0:
        fail Bad(code=x)
    return x

pub def main() uses io:
    a = check(-4) else err -> err.code * 10
    b = check(5) else 0
    io.print(f"{a} {b}")
```

```output
-40 5
```

`else err -> ...` binds the failure; `else 0` replaces it with a value.
