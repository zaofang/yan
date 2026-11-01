# 0007: std.encoding and `bytes`

- Status: draft
- Research: none; this RFC is the proposal (Wave 3 of the std build plan)
- Rows: filled in when accepted

## Summary

R475 makes `std.encoding` the declaring module of `bytes`, and the module table of 17 lists only
`hex` and `base64`. The compiler has no `bytes` type yet. This RFC defines `bytes` as an immutable
byte string with a small method set, UTF-8 conversion in both directions, strict hex and base64, and
one error type. It adds no literal syntax: Xin syntax rows wait for the model probe (R566), so bytes
are built by calls.

## Decisions

### D1: `bytes` is an immutable value

`bytes` is an immutable sequence of `u8` with value semantics, like Python's `bytes` and Rust's
`Box<[u8]>`. It has the following methods:
- `len()`
- `b[i] -> u8`, which panics with `bounds`
- `at(i) -> u8 | None`
- `slice_exact(start, stop) -> bytes`, which follows R499
- `concat(other) -> bytes`
- `to_list() -> list[u8]`
- `starts_with` and `ends_with`
- `find(sub) -> int | None`

It is built by `bytes.from_list(xs: list[u8])`, by `s.bytes()` (UTF-8, 17), and by the decoders below.
`bytes` implements `Eq`, `Ord` (lexicographic) and `Hash`.

Options weighed:
- A mutable `bytearray` as well. Building output in place is what `var b: bytes` plus `concat` does
  until `View`/`MutView` arrive. A second type doubles the surface.
- `list[u8]` only. That loses `Hash` keys, compact storage and a type that says "binary data".

Recommended: one immutable `bytes`, with no literal until the syntax hold lifts.

- Decision:
- Row:

### D2: UTF-8

`b.decode_utf8() -> str fail EncodingError` is strict. Overlong forms, surrogates and truncated
sequences fail at the byte where they start. `b.decode_utf8_lossy() -> str` replaces each maximal
invalid subpart with U+FFFD, as the Unicode standard recommends and as Rust's
`String::from_utf8_lossy` does. `s.bytes()` never fails.

- Decision:
- Row:

### D3: hex

`encoding.hex(b) -> str` writes lowercase digits. `encoding.hex_decode(s) -> bytes fail EncodingError`
accepts either case. It rejects whitespace, `0x` prefixes and odd length.

- Decision:
- Row:

### D4: base64

`encoding.base64(b, url=False, pad=True) -> str` uses the RFC 4648 standard alphabet, or the URL-safe
alphabet when `url=True`. `encoding.base64_decode(s, url=False) -> bytes fail EncodingError` accepts
input with or without padding. It rejects whitespace, characters from the other alphabet, padding in
the middle, and non-zero trailing bits, so each byte string has one accepted encoding per alphabet.

Options weighed: Python's lenient decoder, which discards characters outside the alphabet unless
`validate=True`. Recommended: strict decoding. It follows the strict-input rule of R571, and it closes
the malleability that signatures and cache keys trip over.

- Decision:
- Row:

### D5: `EncodingError`

```
type EncodingError = { at: int, reason: EncodingReason }
type EncodingReason = Utf8() | Hex() | Base64()
```

`at` is the byte offset for `Utf8` and the char offset for `Hex` and `Base64`. Offsets start at 0, as
indexes do. `Show` prints `invalid <kind> at <at>`. The type derives `Show` and `Eq`, and it is the
module's only error type.

- Decision:
- Row:

### D6: `Show` for `bytes`

`show(b)` prints CPython's repr: `b'...'`, with printable ASCII as itself; `\\`, `\'`, `\t`, `\n`
and `\r` as escapes; and every other byte as `\xNN` in lowercase. This matches the CPython style of
R493 for collections, so an agent reading output sees familiar text.

- Decision:
- Row:

## Spec changes

17: the `std.encoding` row of the module table, and a new paragraph "std.encoding and bytes" after the
json paragraph, with one row per accepted item.
