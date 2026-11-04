# 0033: The data profile, a typed literal file in place of TOML

- Status: accepted
- Research: none; the 2026-11-03 assessment of a Yan-based TOML alternative, and the
  [build system study](../docs/studies/build-system.md) (D7, `yan.lock` stays TOML)
- Rows: R826 to R829

## Summary

19.1 already makes the manifest Yan: `package.yag` is one constant built from literals only, read
without running code. Config files elsewhere still need `std.toml` or `json`, which are untyped until
decoded and drop comments on rewrite. This RFC names the 19.1 rule the data profile and lets any
program load such a file as a typed value. The file is ordinary Yan Agent, so the editor and
`yan check` already understand it; the reader is decidable; the canonical printer makes rewrites one
line per change. It is not a new surface, has no evaluation (not Dhall, CUE, Nickel or Pkl), and
changes nothing about `yan.lock` (build system D7).

## Decisions

### D1: a profile of Yan Agent, not a surface

A data file is a `.yag` file that satisfies the profile. There is no new extension, seal or name.

Options weighed: a new niche surface (it adds no meaning over Yan Agent literals, so it fails the
R536 admission rules, and names are frozen); a standalone TOML competitor (an ecosystem nobody asked
for); a profile (one rule, reused by the manifest, the lint set and user config).

Recommendation: a profile.

- Decision: accepted as recommended (lead, 2026-11-04)
- Row: R826

### D2: what the profile admits

A data file holds top-level `NAME: Type = expr` constants, comments and a module docstring, and
nothing else: no imports, functions, types or statements. `expr` is built only from int, float, str,
bytes and bool literals, `None`, unary minus on a number, list, dict, set and tuple displays, and
record or variant constructors with named arguments. No operators, interpolation, comprehensions,
function calls, or references to other constants. Checking is syntactic and linear, so tools read the
file without the elaborator.

Options weighed: references to earlier constants (sharing, at the cost of an evaluator and of
one-line diffs); arithmetic (same cost). Both are deferred, not rejected.

Recommendation: literals only, as above.

- Decision: accepted as recommended (lead, 2026-11-04)
- Row: R827

### D3: typed loading through `Wire`

`std.data` gets `parse[T: Wire](s: str, name: str) -> T fail DataError` and
`dump[T: Wire](x: T, name: str) -> str`. `parse` finds the constant `name`, checks that its annotation
names `T`, and decodes the literal tree with the `Wire` decoder of 17.4, so unknown fields, duplicate
keys and limits behave as in every other codec. Constructor names resolve against `T`'s declared
types and variants, never against scope. `DataError` carries `line`, `col` and a reason (profile
violation, missing constant, type mismatch, decode failure), shaped like `JsonError`. `dump` prints
canonical form: `yan fmt` layout, fields in declaration order, so `dump(parse(s))` equals `s` for a
formatted file without comments.

Options weighed: an untyped `Value` tree like `json.Value` (useful later, not needed now); keeping
comments through `dump` (needs a concrete syntax tree API; deferred).

Recommendation: typed `parse` and `dump` only.

- Decision: accepted as recommended (lead, 2026-11-04)
- Row: R828

### D4: the manifest and lint set use it

19.1's literal-only rule for `package.yag` and its `Lints(...)` field (R819) is restated as the data
profile, with one parse-family diagnostic (section 25) for a violation. `yan.lock` stays TOML (build
system D7), and `std.toml` stays for interop.

Recommendation: restate 19.1 in terms of the profile.

- Decision: accepted as recommended (lead, 2026-11-04)
- Row: R829

### D5: timing

Rows land once decided; `std.data` and the shared checker ship after M1, with `derive(Wire)`, which
they need.

Recommendation: rows now, implementation after M1.

- Decision: accepted as recommended (lead, 2026-11-04)
- Row: none (timing; `std.data` ships after M1 with `derive(Wire)`)

## Spec changes

- 17.3: a `std.data` row in the module table and a paragraph for D2 and D3.
- 19.1: the manifest sentence cites the profile (D4).
- 25: E144 for a profile violation.
