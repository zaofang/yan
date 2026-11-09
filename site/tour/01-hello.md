# Hello, capabilities

A Yan program starts at `main`. This one is in Yan Agent (`.yag`), the Python-shaped surface:

```yag
pub def main() uses io:
    io.print("hello, world")
```

```output
hello, world
```

`uses io` is a capability. A function can only touch the outside world (the terminal, files, the
network, the clock) through capabilities it names, and a caller must hold every capability its callees
use. The checker enforces this before anything runs:

```yag expect=E301
def greet() -> None uses io:
    io.print("hi")

def twice() -> None:
    greet()
```

`twice` calls `greet`, which needs `io`, but `twice` does not declare it, so the program is rejected
with E301. A function that names no capability is pure: given the same arguments it returns the same
result, every time, on every machine.
