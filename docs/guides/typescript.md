# Yan for TypeScript developers

Learning-set guide (spec 18.9, R319, R664). Examples are plain Yan (`.yn`). Rules live in the spec; section numbers are given where a rule is cited.

## At a glance

| TypeScript | Yan |
|---|---|
| `const x = 1` | `let x = 1` |
| `let x = 1` (reassigned) | `var x = 1` |
| `` `a ${x} b` `` | `"a \(x) b"` |
| `(a) => a + 1` | `a -> a + 1` |
| `x: number` | `x: int` or `x: float`; no implicit conversion |
| `string`, `boolean` | `str`, `bool`; literals `True`, `False` |
| `null`, `undefined`, `T \| undefined` | `Option[T]`; there is no null |
| `throw`, `try`/`catch` | `fail E` in the signature, `?` to propagate, `try Call` to reify |
| `interface P { x: number }` | `type P = { x: int }` |
| discriminated union | `type S = A(x: int) \| B()` |
| `switch`, `x.kind === ...` | `match` |
| `Array<T>`, `T[]` | `List[T]` |
| `Map<K, V>`, `{[k: string]: V}` | `Map[K, V]` |
| `console.log(x)` | `io.print(x)`, which needs `uses io` |

## Bindings

`let` never means "mutable". `let` binds once; `var` allows reassignment. A record bound with `let` cannot change; build a changed copy with `evolve(r, a: 3)`.

```
let total = 0
total += 1        # error: total is not var
var count = 0
count += 1
```

## Strings

Interpolation is `\(expr)`. Braces are literal, so `"${x}"` prints a dollar sign and the value of `x` in braces, and lint W854 flags it with a rewrite to `\(x)`.

```
let name = "Ada"
io.print("hello \(name)")
```

## Functions and closures

Functions have a signature with types on every parameter and result. The body follows `=` on an indented block, and the last expression is the value. `return` is for early exit.

```
pub area(w: int, h: int) -> int uses none =
  ex area(2, 3) == 6
  w * h
```

A closure is `x -> expr`. There is no `function` keyword and no `=>`. Methods are called with dots, and `xs.map(x -> x + 1)` is lazy until you call `.collect()`.

`ex` clauses are examples that `yan test` checks. `pre` and `post` state contracts. A function declares what it may touch: `uses none`, `uses io`, `uses net`. Nothing reaches the network or disk without a declared capability.

## Types

Records and sums replace interfaces, classes and discriminated unions.

```
type Auth = { id: int, merchant: str, minor: int }
type Decision = Approved(code: str) | Declined(reason: str)

describe(d: Decision) -> str uses none =
  match d
    Approved(code) -> "ok \(code)"
    Declined(reason) -> "no \(reason)"
```

Construction uses named fields: `Auth(id: 1, merchant: "m", minor: 5)`. A `match` must cover every variant, so adding a variant breaks the callers that forgot it, as a `never` check does in TypeScript but without the boilerplate. Behaviour is attached with `trait` and `impl`, not `class`. There is no inheritance.

## Numbers

`int` and `float` are different types and never convert on their own. Integer literals fit any integer or float type that holds them exactly, so `x * 2` works for a float `x`. A variable never converts: use `as` (checked, panics on overflow) or `wrap_as`, `sat_as`, `trunc_as`. Integer `//` divides and rounds down; `/` is for floats. There is no `NaN` surprise from `parseInt`: `"12".parse_int()` returns an `Option`.

## Missing values

No `null` and no `undefined`. A value that may be absent is `Option[T]`. Supply a default with `else`:

```
let n = text.parse_int() else 0
let s = acc.get(key) else Settled(merchant: copy key, gross: 0, fees: 0, net: 0)
```

## Errors

A function that can fail says so in its signature. The caller propagates with `?` or handles it.

```
fetch(sym: str) -> float fail PortErr uses net =
  let body = http.get_text("https://px.example/\(sym)")?
  json.parse_float(body) else fail Fetch(msg: "bad price: \(sym)")
```

`fail E` is the declared error type, `else fail X` fails when an `Option` is empty, and `?` converts error types through `impl From`. There are no unchecked exceptions; a panic is for bugs (overflow, failed `assert`) and is not caught by ordinary code.

## Ownership in one paragraph

Values are passed without copying. When you store a borrowed value, build a record from it or return it, you write `copy x`, as in `Settled(merchant: copy a.merchant, ...)`. The checker tells you where it is needed and offers the fix. There is no garbage-collector pause and no `clone()` you must guess at.

## Collections and loops

```
var seen: Map[str, int] = {}
for (k, v) in pairs
  seen[copy k] = v
for i in 0..n
  io.print("\(i)")
```

`for` iterates a list, a map or a range. `while` and `loop` exist; there is no C-style `for (;;)`.

## Modules

`use` imports by path and everything is private unless marked `pub`. There is no `export default` and no side effect at import time.

## Habit notes

Where a diagnostic is triggered by a TypeScript habit, its `habit` child (R662) names the habit and the Yan form. The common ones:

| You wrote | Diagnostic habit note |
|---|---|
| `let x = 1; x = 2` | `let` binds once; use `var` |
| `(a) => a` | closures are `a -> a` |
| `"${x}"` | W854; use `\(x)` |
| `null` | use `Option[T]` and `None` |
| `throw new Error()` | declare `fail E` and use `fail` |
| `class A extends B` | use `type`, `trait` and `impl` |

## What stays familiar

Indentation-structured blocks (as in Python), `if`/`else`, `while`, `break`, `continue`, `and`/`or`/`not` instead of `&&`, `||`, `!`, `==` that compares by value, and method-call chains.
