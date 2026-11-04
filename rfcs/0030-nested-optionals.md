# 0030: Nested optionals keep two levels

- Status: draft
- Research: none; source reading of `crates/yan-elab/src/lower.rs` (`inject`) and `crates/yan-elab/src/ty.rs` (`fits`)
- Rows: filled in when accepted

## Summary

The spec says `T | None` is `Option[T]` (R285, R421, R476) but never says what happens when `T` is itself
optional. The question is not academic: `d.get(k)` on a `dict[K, V | None]` returns `V | None` with
`V = int | None`, and the caller must be able to tell a missing key from a stored `None`. Today the
elaborator answers it by accident. `inject` skips the `Some` wrap whenever the value is already
`Option`, and `Ty::fits` accepts `Option[V]` where `Option[Option[V]]` is wanted, so a stored `None`
reaches the caller as the outer `None` and reads as "missing". This RFC makes nested optionals two
distinct levels, as in Rust and Swift, and fixes how they are spelled, injected and tested.

## Decisions

### D1: two levels, no collapse

`Option[Option[T]]` is its own type. `None` and `Some { value: None }` are different values.

Options weighed:

- Collapse, as Python's `Optional[Optional[T]]` and Kotlin's `T??`. Simpler to read, but it breaks
  parametricity: a generic function over `V` behaves differently when `V` happens to be optional, so
  `dict.get`, `list.get`, iterator `next` and `index -> int | None` (R218) all lose information for
  some element types. Agents writing generic code cannot see the case coming.
- Keep two levels, as Rust's `Option<Option<T>>` and Swift's `T??`. Generic code means the same thing
  for every `V`, and the cost is a rare explicit spelling.

Recommendation: keep two levels. Substituting an optional type for a type parameter or alias never
flattens, so `get` on `dict[str, int | None]` returns `Option[int | None]`.

- Decision: (lead's answer, date)
- Row: (R number once accepted)

### D2: spelling

Nesting is written `Option[T | None]` (or `Option[T?]` in plain Yan). A flat `T | None | None`, or `T??`
in plain Yan, is rejected with a diagnostic that suggests `Option[T | None]`, because a union lists
`None` once and one type has one spelling (R425). Yan Core text spells the inner value
`Some { value: None }` (R476).

- Decision:
- Row:

### D3: injection adds one level

Where `Option[U]` is expected and the value's type fits `U`, the elaborator wraps it in `Some` once,
including when `U` is itself optional (today's `inject` skips this case). The literal `None` takes the
expected type, so it is the outer `None`; the inner one is written `Some(None)`. A plain `T` where
`Option[Option[T]]` is expected is a type error with the fix `Some(v)`; injection never adds two
levels. `Ty::fits` stops accepting `Option[V]` for `Option[Option[V]]` without that wrap.

- Decision:
- Row:

### D4: tests and operators work one level at a time

`x is None`, `x is not None`, `x else y` and `x?` act on the outer level, and narrowing `x is not None`
on `Option[Option[T]]` gives `Option[T]`. The lenient reader does not apply N15 to a nested optional:
Python's `if d.get(k):` is false for both a missing key and a stored `None`, so no single explicit test
preserves the author's meaning, and strict mode reports E201 as it does for any truthiness test.

- Decision:
- Row:

### D5: conformance (proposed, separate PR)

Cases to add only with the lead's go, in a PR of their own: `get` on `dict[str, int | None]`
distinguishes a missing key from a stored `None`; `int | None | None` is rejected; a `V | None` value
returned as `Option[V | None]` arrives as `Some`; `if d.get(k):` on a nested optional is not rewritten.

- Decision:
- Row:

## Spec changes

- §14, "None and narrowing": D1, D2, D4 (nesting, spelling, one-level tests, N15 exclusion).
- §17, "Optional values in Yan Core text": the nested Core spelling from D2.
- §14 lenient reader table: the N15 row excludes nested optionals.
- Elaborator (not spec): `inject` and `Ty::fits` per D3.
