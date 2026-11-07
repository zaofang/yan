# 0046: Std readings assumed by the implementation

- Status: accepted
- Research: none; starts from the std group of `SPEC_QUESTIONS.md` and issue #408
- Rows: R908 to R914

## Summary

Settles the std group of `SPEC_QUESTIONS.md`: Wave 0 runtime readings, `std.regex`, `std.collections`, Wave 2
elaboration readings and the intrinsic registry. The three std declaration answers already carry rows (R474 to R476).

## Decisions

### D1: dict key miss and bigint spelling

Options for `d[k]` on a missing key: (a) panic kind `bounds`; (b) add a panic kind `key`. Recommendation: (a). 8.3's
kind set is closed and conformance cases already expect `bounds`; Python's KeyError is a different name for the same
class of fault. `bigint` arithmetic is `<op>_bigint` with no `_checked` suffix (it cannot overflow), floor division
and modulo round toward negative infinity.

- Decision: as recommended, taken under the lead's standing rule for choices not worth a card; Antonio, 2026-11-07
- Row: R908

### D2: string parsing and lines

`str.parse_int` takes an optional sign and ASCII digits only, returning None outside `int`; `str.parse_float` adds `.`,
an exponent and `inf`, `infinity`, `nan` in any case; `str.splitlines` uses CPython's boundaries and drops a final
empty line. Recommendation: accept. Radix prefixes and `_` stay out; a `parse_int_radix` is the extension point.

- Decision: as recommended; Antonio, 2026-11-07
- Row: R909

### D3: std.regex

`split` keeps captured groups between pieces and a group that did not take part is `""`. A `RegexError` reason and the
E545 message are the engine's text; the wording is not part of the contract, so tests match on the code. `group(i)`
past the last group and `named(n)` for a missing name panic with kind `bounds`. A free std function is called by its
module's name with no import (`regex.compile`, `math.sqrt`).

- Decision: as recommended; Antonio, 2026-11-07
- Row: R910

### D4: std.collections

One submodule per type (`std.collections.deque`, `.heap`, `.sorted_dict`, `.sorted_set`), each the declaring module for
R475. `for` over a `sorted_dict` yields keys ascending; `for` over a heap is E201. `sorted_set(xs)` and `heap(xs)` take a
list, `sorted_dict(d)` a dict. `m[k] op= v` on a `sorted_dict` inserts `Default` first, as R438 does for `dict` (changed:
the implementation panics today, so this is a fix owed). The storage is unobservable except by timing, and the spec
promises the bounds of a balanced tree, not the representation.

- Decision: as recommended; Antonio, 2026-11-07
- Row: R911

### D5: error types and fakes

`FsError` is a record `{path: str, reason: str}`, `not found` for a missing file; `fake_fs(files)` takes the map and
`with fs = fake_fs(...)` installs it; `fs.read` of an absent path fails with `FsError`. `std.test` is in scope without an
import. A std class reaches a module when the module's source names the class or its module, lowered as a record unless
the module defines the name.

- Decision: as recommended; Antonio, 2026-11-07
- Row: R912

### D6: `Error`

`Error` is `{message: str, context: list[str]}`, outermost first, the message the wrapped failure's `Show`. `?` into a
function that fails with `Error` converts through `std.error.wrap`; `f().context(msg)` yields the value or fails with
the added context. Options for downcast: (a) none now, text only; (b) keep the wrapped value as a `Dyn` field so
`e is HttpError(...)` works. Recommendation: (a) for 1.0 contracts (`Wire` stays total), with (b) a post-1.0 additive
field; recorded as deferred.

- Decision: as recommended; Antonio, 2026-11-07
- Row: R913

### D7: intrinsic registry

The registry is generated at build time from `@intrinsic` functions and `@capability` operations in `yan-std/lib/<tier>/`
plus the scalar `<op>_<ty>` family and `exclusive_check`. An entry's signature is its declaration; purity is `Cap` for a
capability operation, `Mutates` with a `var` parameter, else `Pure`; only `Cap` entries run at stage 1 or later. Ids are
indexes in path order, stable within one build and never persisted; paths key any cache. Yan Core text names std
functions by path, resolved to an id when the interpreter compiles a call site and by each code generator from
`yan-mir`.

- Decision: as recommended; Antonio, 2026-11-07
- Row: R914

## Spec changes

8.3 (D1), 17.2 and 17.3 (D2 to D6), 5.3 and 22 (D7). Rows carry the rules; conformance cases are proposed in a
separate PR.
