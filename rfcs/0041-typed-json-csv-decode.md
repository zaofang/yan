# 0041: Typed json and csv over Wire

- Status: accepted (the lead chose D1 internal tag on 2026-11-06 and accepted D2 to D8 as recommended on 2026-11-06)
- Research: none; this RFC is the proposal (issue #352, T12b3)
- Rows: R874 to R881

## Summary

Typed decode and encode for `json` and `std.csv` go only through `Wire`: `json.parse[T: Wire]` and
`json.dump[T: Wire]` (already named by the std.json rows), and a csv API of the same shape. The
`Wire` decoding rules of 17.4 already fix the safety properties (static types only, declared tags,
limits, unknown fields and duplicate keys rejected unless the type opts in). They do not fix how
each `Wire` shape looks as json text or as a csv row. This RFC fixes that mapping, the extra error
reasons, and the csv rules, so that a record and a sum type round-trip through both formats on both
backends. The untyped `json.Value` design of the std.json RFC is unchanged.

## Decisions

### D1: How a sum type is written in json

A sum `type S = A(x: int) | B()` needs a tag on the wire. Options:
- Internal tag: `{"tag": "A", "x": 1}`, and a field-less variant is `{"tag": "B"}`. One object per
  value, easy to read and to write by hand, the form model output and most APIs already use.
- External tag: `{"A": {"x": 1}}`, and `"B"` for a field-less variant. No clash with field names, but
  two shapes (string or one-key object).
- Adjacent tag: `{"tag": "A", "value": {"x": 1}}`. Always the same shape, longer.

Recommended: internal tag. The key defaults to `tag` and `@wire(tag: "kind")` on the sum renames it.
A variant with a field of that name is a compile error at the derive, so a clash never reaches run
time. The tag value is the variant name, or its `@wire(name: ...)`. Decoding checks the tag against
the declared variants only; an unknown tag fails and never selects a type.

- Decision: accepted, internal tag, 2026-11-06
- Row: R874

### D2: Other `Wire` shapes in json

- Record: an object, members in declaration order. `@wire(name: "x")` renames a field. A field with a
  default may be absent on input; `dump` always writes every field.
- `T | None`: `null` for `None`, the plain value otherwise. A field of this type may also be absent
  on input and reads as `None`. A nested optional (`T | None | None`) is not representable and is a
  compile error at the derive.
- `bool`, `str`: the json forms. `int`: an integer literal in the i64 range. `float`: any number;
  an integer literal widens exactly, else it fails `Range`. An `int` field given `1.0` fails with a
  mismatch, because no digits are dropped silently. A non-finite float has no json form, so `dump`
  panics with `value`, the rule already used for `dump_value`.
- `list[T]`, `tuple`, `set[T]`: arrays; a set decodes with duplicates rejected and dumps in its
  iteration order. Tuples are fixed-length arrays and a wrong length is a mismatch.
- `dict[str, V]`: an object; a duplicate key is rejected. `dict[K, V]` with another `K`: an array of
  `[key, value]` pairs.
- `bytes`: a base64 string (the standard alphabet with padding, from `std.encoding`).
- newtype `N` over `T`: the encoding of `T`; refinements and invariants run after construction.
- `json.Value`: itself, as the std.json rows say.

- Decision: accepted as recommended, 2026-11-06
- Row: R875

### D3: Unknown fields

Rejected by default, as 17.4 requires. `@wire(unknown: "ignore")` on a record opts in to skipping
them. There is no mode that collects them; a program that needs the rest reads `json.Value`.
Duplicate keys always fail.

- Decision: accepted as recommended, 2026-11-06
- Row: R876

### D4: Errors carry a path

`JsonReason` gains four reasons, so a typed failure says where it is:

```
Mismatch(path: str, expected: str, found: str)
Missing(path: str, field: str)
Unknown(path: str, field: str)
Invalid(path: str, msg: str)     // validation or invariant failure after construction
```

`path` is `$` for the root and then `.name` and `[i]` steps (`$.items[2].price`). `line` and `col`
point at the start of the offending value. Unknown tags are `Mismatch` with `expected` listing the
declared variants. Syntax-level reasons are unchanged and the type is still the module's only error.

- Decision: accepted as recommended, 2026-11-06
- Row: R877

### D5: csv API and file rules

```
pub def parse[T: Wire](s: str, delimiter: str = ",") -> list[T] fail CsvError
pub def dump[T: Wire](rows: list[T], delimiter: str = ",") -> str
```

`T` is the row type. The first line is a header and is required. Columns are matched by name (with
`@wire(name:)`) in any order. A missing column is allowed only when the field has a default or is
optional; an extra column fails, with the same opt-in as D3. Input is RFC 4180: quoted fields,
doubled quotes, embedded delimiters and newlines inside quotes; LF and CRLF both read; a byte order
mark, a bare quote in an unquoted field, a short or long row and an unterminated quote fail. Output
is deterministic: a header, then one row per element, `\n` after every row including the last, and a
field is quoted only when it contains the delimiter, a quote, CR or LF, or is the empty string.
Output uses LF, not RFC 4180's CRLF, so files are identical on every platform.

Options weighed: a streaming row iterator, a `header=False` mode that maps by position, and
untyped rows. All three are left out of the first cut; the table engine's `read_csv` stays the path
for untyped and large files.

- Decision: accepted as recommended, 2026-11-06
- Row: R878

### D6: What a csv cell may hold, and sums in csv

A cell is a scalar. Allowed row fields: `bool` (`true`, `false`), `int`, `float` (non-finite fails
on dump), `str`, `T | None` of those, newtypes over them, and sums whose variants all have no fields
(the cell is the variant name). Any other field type (list, dict, record, sum with payload) is a
compile error at the derive for csv use. `None` is an unquoted empty cell; `Some("")` is a quoted
empty cell, so `str | None` round-trips. A plain `str` field reads either as `""`.

A row type may itself be a sum, so a sum with payloads round-trips: the tag is the column `tag`
(renamed as in D1), and the columns are the union of the variants' fields. A field name shared by
variants must have one type. A cell for a field the row's variant does not have must be empty, and
`dump` writes it empty. Column order is the tag, then fields in first-appearance order across
variants.

Options for sums: only field-less sums inside cells (no row-level sums), or both (recommended). The
acceptance case needs a sum with payloads through csv, so the recommendation keeps it.

- Decision: accepted as recommended, 2026-11-06
- Row: R879

### D7: `CsvError`

```
type CsvError = { line: int, col: int, row: int, column: str, reason: CsvReason }
type CsvReason = Syntax(msg: str) | Header(msg: str) | Mismatch(expected: str, found: str)
               | Missing(field: str) | Unknown(field: str) | Invalid(msg: str) | Limit()
```

`line`, `col` and `row` start at 1 (`row` 1 is the first data row, 0 the header); `column` is the
header name or empty. `Show` prints `line L, col C: <reason>`. It derives `Show`, `Eq` and `Wire`.
Total size, row count and field length are limited like every `Wire` decoder; the limits are the
`Wire` defaults.

- Decision: accepted as recommended, 2026-11-06
- Row: R880

### D8: Acceptance and scope

Both formats are checked by round-trip cases run from a `.yag` program on the interpreter and the
compiled backend: a record, a record with optional and defaulted fields, a sum with and without
payload, and the error paths of D4 and D7. `std.toml` is out of scope and follows later over the same
mapping. Implementation waits on `derive(Wire)` codegen.

- Decision: accepted as recommended, 2026-11-06
- Row: R881

## Spec changes

17: the std.json paragraph (JsonReason, the typed mapping), a new std.csv paragraph, and the `json`,
`std.csv`, `std.toml` row of the module table. 17.4: a pointer to the per-format mappings. 25: one
compile error for a field type a codec cannot represent (nested optional, a non-scalar csv cell, a
variant field named like the tag). Conformance cases are proposed in a separate PR.
