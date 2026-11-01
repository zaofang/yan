# 0005: std.json

- Status: accepted (the lead chose "Accept, but wait for typed parse" on 2026-11-01: D2 to D7 accepted; D1 amended so that nothing ships before `derive(Wire)`)
- Research: none; this RFC is the proposal (Wave 3 of the std build plan)
- Rows: R569 to R575

## Summary

Section 17 gives `json` its 1.0 API: `parse[T: Wire](s) -> T fail JsonError`, `dump[T: Wire](x) -> str`
and `parse_float`. The typed calls need `derive(Wire)`, which needs stage-0 reflection, and the
compiler does not have that yet. This RFC ships the untyped core now: a `json.Value` sum type, a
strict parser and a deterministic printer. It also fixes the rules that the typed API will reuse:
numbers, limits, duplicate keys, output form and errors. Once `derive(Wire)` exists, `parse[T]` and
`dump[T]` decode and encode directly under the same rules, and `Value` becomes one more `Wire` type.

## Decisions

### D1: An untyped `json.Value` now, typed `parse[T]` with `derive(Wire)`

```
type Value = Null() | Bool(b: bool) | Int(i: int) | Float(f: float) | Str(s: str)
           | Array(items: list[Value]) | Object(fields: dict[str, Value])
pub def parse_value(s: str, max_depth: int = 128) -> Value fail JsonError
pub def dump_value(v: Value, indent: int | None = None) -> str
```

Options weighed:
- Wait for `derive(Wire)`. Nothing ships until stage 0 exists, which is months away.
- Ship only `Value`. This loses the "decoding produces only a statically known type" property of 17.
- Ship `Value` now and add the typed calls later. The 1.0 API is unchanged, and `Value` stays useful
  afterwards for documents whose shape is unknown, the way `serde_json::Value` is.

Recommended: ship `Value` now and add the typed calls later. `parse_value` and `dump_value` are
separate names, so the typed `parse` and `dump` keep their spec meaning.

- Decision: amended, 2026-11-01. `json.Value`, `parse_value` and `dump_value` keep this design but ship together with typed `parse[T]` and `dump[T]` once `derive(Wire)` exists; no untyped interim API ships first.
- Row: R569

### D2: Numbers

A number with no fraction and no exponent parses as `Int` when it fits `int`, the i64 range of 11.1.
An integer literal outside that range fails `JsonError` with reason `Range`. Any other number parses
as `Float`, correctly rounded. `-0` is `Int(0)`; `-0.0` is `Float(-0.0)`.

Options weighed:
- Python: arbitrary-precision ints.
- JavaScript: every number is a float.
- An exact range check (recommended). It never loses digits silently. A program that needs big
  integers reads them as strings.

`dump_value` prints a `Float` with the shortest round-trip digits, the same digits as `show`. It
always includes a `.` or an exponent, so the value parses back as a `Float`. A non-finite float has no
JSON form, so dumping one panics with `value`. A non-finite float cannot come from `parse_value`, so
only constructed values can panic.

- Decision: accepted, 2026-11-01
- Row: R570

### D3: Strict RFC 8259 input

The parser rejects the following, each with a `JsonError`:
- comments
- trailing commas
- single quotes
- `NaN` and `Infinity`
- leading zeros
- a byte order mark
- control characters in strings
- unpaired surrogate escapes
- trailing content after the value

Whitespace is space, tab, LF and CR only. A duplicate key in one object fails with reason
`Duplicate(key)`, as 17 requires ("duplicate keys are rejected unless the type opts in"); `Value` does
not opt in. Nesting deeper than `max_depth` fails with reason `Depth`.

Options weighed: a lenient mode, like JSON5 or Python's acceptance of `NaN`. Lenient input belongs to
a separate `@yong/json5`. Recommended: strict only.

- Decision: accepted, 2026-11-01
- Row: R571

### D4: Deterministic output

`dump_value` keeps an object's insertion order, since `dict` is insertion-ordered (17.3), and takes no
`sort_keys`. To get sorted output, build a `sorted_dict` and convert it.

The compact form has no spaces: `{"a":1,"b":[1,2]}`. With `indent=n`, the output has one member per
line, `": "` after keys, and no trailing newline.

Strings escape only `"`, `\` and control characters, using the short escapes `\b \f \n \r \t` and
`\u00XX` for the rest. Non-ASCII text is written as UTF-8.

So equal values always print the same bytes, on every platform and in every run.

Options weighed: Python's `", "` and `": "` separators, and `ensure_ascii`. Both make the output longer
and add nothing a reader needs. Recommended: compact output with no `ensure_ascii`.

- Decision: accepted, 2026-11-01
- Row: R572

### D5: `JsonError`

```
type JsonError = { line: int, col: int, reason: JsonReason }
type JsonReason = Syntax(msg: str) | Depth() | Duplicate(key: str) | Range()
```

`line` and `col` start at 1, and `col` counts chars. `Show` prints `line 3, col 7: <reason>`. The type
derives `Show` and `Eq`, as the errors rule of 17 requires; `Wire` follows when derives exist. This is
the only error type of the module.

- Decision: accepted, 2026-11-01
- Row: R573

### D6: Accessors on `Value`

`v.get(key) -> Value | None` (objects), `v.at(i) -> Value | None` (arrays, no negative index),
`as_bool`, `as_int`, `as_float` (an `Int` widens exactly or gives `None`), `as_str`, `as_list` and
`as_dict`, each `-> T | None`. `v.is_null() -> bool`.

Every accessor is total: the wrong kind of value gives `None` and never a panic. Agent-written code
can then read a config field with an `else` default (`port = cfg.get("port") else ...`) instead of a
nested `match`.

Options weighed: `match` only (no new API), or indexing that panics (Python's `d["k"]`). Recommended:
total accessors.

- Decision: accepted, 2026-11-01
- Row: R574

### D7: `parse_float`

`json.parse_float(s) -> float fail JsonError` parses a document that is exactly one JSON number, after
optional surrounding whitespace. It uses the number grammar of D2 and D3, and an integer literal
widens to `float` exactly or fails with `Range`. It is the call shown in the examples of 14 and 15.

- Decision: accepted, 2026-11-01
- Row: R575

## Spec changes

17: the `json` row of the module table and a new paragraph "std.json" after the iter paragraph, with
one row per accepted item.
