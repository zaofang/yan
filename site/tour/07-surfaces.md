# One core, many surfaces

Yan Agent is one surface. Plain Yan (`.yn`) is the syntax written for people: no colons, expression
bodies, clause-form functions. Both lower to the same core, Yan Core, so they share types, checks and
runtime, and the playground prints any module in every surface at once.

The shapes example from earlier, in Yan:

```yn
type Shape = Circle(r: int) | Rect(w: int, h: int) | Dot()

area(s: Shape) -> int uses none =
  Circle(r) -> 3 * r * r
  Rect(w, h) -> w * h
  Dot() -> 0

main() uses io =
  for s in [Circle(r: 1), Rect(w: 2, h: 3), Dot()]
    io.print("\(area(s))")
```

```output
3
6
0
```

Open it in the playground and look at the Yan Agent pane: it is the same module, printed in the
other surface.
