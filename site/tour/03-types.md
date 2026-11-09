# Sum types and matching

A `type` with alternatives is a sum type. `match` takes it apart, and the checker requires every
alternative to be handled.

```yag
type Shape = Circle(r: int) | Rect(w: int, h: int) | Dot()

def area(s: Shape) -> int:
    match s:
        case Circle(r):
            return 3 * r * r
        case Rect(w, h):
            return w * h
        case Dot():
            return 0

pub def main() uses io:
    for s in [Circle(r=1), Rect(w=2, h=3), Dot()]:
        io.print(f"{area(s)}")
```

```output
3
6
0
```
