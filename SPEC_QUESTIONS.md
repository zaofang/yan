# Spec questions

## Conformance header layout

Spec 24.1 shows `# expect: E501 at 14:9` (Wen and Yong: `-- expect:`), `# stdout:` blocks and
`# exit: 101`, but not how a `stdout:` block is laid out.

Assumed in `tools/conform.py` until resolved: headers are the file's leading comment lines (`#` in
Jian, `--` in Wen and Yong); a `stdout:` block is the following comment lines with one space
stripped (a bare marker is an empty line) up to the next header or the first non-comment line;
several blocks concatenate. `patch/` cases are directories and are skipped until their format is
defined.

## Yong text lexical rules

Spec 3.5 uses `Name`, `Literal` and `String` without defining them. Assumed in `yong-core` until
resolved:

- `Name` is `[A-Za-z_][A-Za-z0-9_]*`; `Path` is names joined by `.`. Keywords are contextual.
- Integer literals are decimal, `0x`, `0o` or `0b`, with `_` separators and an optional leading
  `-`; float literals add a fraction and/or an exponent. Literals carry no type suffix.
- Strings are `"..."` and chars `'c'`, with escapes `\n \r \t \0 \\ \" \'` and `\u{hex}`.
  A `'` followed by a name and no closing quote is a label.
- `true`, `false` and `()` (the `Unit` value) are literals.
- Diagnostic columns are 1-based UTF-8 byte offsets.

## Yong canonical layout

Spec 3.5 fixes the grammar, not the printed layout, and comments are not part of the IR. Assumed:
four-space indentation, one statement per line, contracts on their own lines before the body,
records and sums on one line, one blank line between items. The printer keeps only the file's
leading comment lines (the conformance headers); round-trip means print(parse(print(parse(x))))
equals print(parse(x)).

## `uses` lists inside other lists

`Fn(...) -> T uses a, b` inside a parameter list or a type-argument list is ambiguous with that
list's commas. Assumed: a comma continues the capability set unless the next name is followed by
`:`, `[` or `.`.

## Checker readings not fixed by the spec

Assumed in `yong-check` until resolved:

- Scalars (integers, floats, `bool`, `char`, `Unit`) are copied, so a `sink` argument does not
  move them, and passing a scalar local to `become` with `let` is not E420.
- E602 applies to direct and mutual recursion found in function bodies; `example` and other
  contract calls are not edges.
- E401 is flow-sensitive across `if` and `match`: a branch that ends in `ret`, `fail`, `jump`,
  `become`, `break`, `continue` or `unreachable` does not carry its moves forward.
- A type the checker cannot work out (std results, generic instances, tuples, closures) is unknown
  and never produces E201. An integer literal fits any numeric type, a float literal any
  non-integer numeric type.
- `yong run` checks the module first and exits 1 with the diagnostics on stderr when it has errors.

## Standard library declarations (resolved: R474 to R476, spec rev 496)

Raised by the std build plan; Antonio accepted all three answers on 2026-10-31.

- R474: a std function with a native body is `@intrinsic def name(params) -> T: ...`. `@intrinsic`
  is reserved to the `std` package; the intrinsic list, including the Y4 scalar ops, lives in
  `yong-std`.
- R475: for step 2 of method resolution (17.2), `str` and `char` are declared in `std.text`,
  `list` in `std.list`, `dict` in `std.dict`, `set` in `std.set`, `bytes` in `std.encoding`, the
  scalars and `bigint` in `std.num`, and `Result` and `Option` in `std.result`.
- R476: in Yong text, `Option[T]` matches as `Some { value: v }` and `None`, alongside Y5's
  `Ok { value: v }` and `Err { error: e }`.

## Jian elaboration readings

Assumed in `yong-elab` until resolved:

- E210 exempts the arguments of a `become` call. `become count(n - 1, acc + 1)` with two `int`
  parameters is a run case (`jian/run/tail/become_constant_stack`) and appears in
  `jian/pass/syntax/every_production`, though 14.1's exemptions (literals, names equal to the
  parameter, positional-only parameters, the receiver) do not cover it.
- E210 is reported once per call, at the first argument that needs a keyword (the note in
  `jian/fail/types/e210_record_keyword_required`).

- Bitwise `&`, `|` and `^` on integers lower to the intrinsics `and_<int>`, `or_<int>` and
  `xor_<int>`, which Y4 does not name; they cannot overflow. Shifts stay unsupported until their
  overflow rule is settled.
- `_ = e` evaluates `e` and discards it.
- `par [e for x in xs]` runs sequentially in the interpreter. The result order and the
  lowest-index failure (9.2) are what a parallel run must also produce.
- `&xs[i]` and `&r.f` pass a copy that is stored back after the statement. Two `&` elements of
  the same list in one call insert `exclusive_check` on their indices first (4.3).
- A lambda's parameter types come from the expected callable type. `k = lambda x: x + 1` with
  no annotation takes them from the first call `k(...)`, where the closure is created; that is
  the same as creating it at the binding only because nothing it captures is reassigned, so a
  lambda that captures a reassigned name, or whose name is used before a call, is not lowered yet. Lambda arguments are lowered after the call's other
  arguments, so `twice(lambda s: s + "!", "hi")` finds `T` from `"hi"`; creating a closure has no
  effects, so the order is not observable.
- A lambda captures locals by copy, `var` parameters `inout` (E406 if it escapes) and
  capabilities by name (E306 if it escapes, R467).

## Std runtime readings (Wave 0)

Assumed in `yong-interp`'s native std functions until the spec says otherwise:

- `d[k]` on a dict without `k` panics with kind `bounds`; 8.3 lists no key-specific kind.
- `bigint` arithmetic intrinsics are spelled `<op>_bigint` (`add`, `sub`, `mul`, `floordiv`, `mod`
  and the six comparisons), with no `_checked` suffix because they cannot overflow. Floor division
  and modulo round toward negative infinity, as for the sized integers.
- `str.parse_int` accepts an optional sign and ASCII digits only (no spaces, underscores or radix
  prefixes) and returns None outside `int`. `str.parse_float` accepts the same plus `.`, an
  exponent, and `inf`, `infinity` and `nan` in any case.
- `str.splitlines` splits on CPython's line boundaries and drops a final empty line.

## std.regex readings (R482 to R486)

Assumed in the regex implementation until the spec says otherwise:

- `split` keeps each captured group between the pieces, as Python does. A group that did not
  take part is `""`, because the result is `list[str]`, where Python gives None.
- A `RegexError` reason, and the E545 message, is the crate's text (for example `unclosed group`).
  It is not Python's wording.
- `group(i)` past the last group, and `named(n)` for a name the pattern lacks, panic with kind
  `bounds`. Python raises IndexError there.
- A free std function is called by its module's name with no import (`regex.compile`,
  `math.sqrt`), as the N01 reading of `import` implies.

## std.collections readings (R488 to R493)

Assumed in the collections implementation until the spec says otherwise:

- std.collections declares one submodule per type (`std.collections.deque`, `.heap`,
  `.sorted_dict`, `.sorted_set`), because the four types share method names such as `len`, `pop`
  and `first`. R475's declaring module for each type is its submodule.
- `for k in m` over a `sorted_dict` iterates its keys in ascending order, as over a Python dict.
  `for` over a heap is E201, the code for a type that does not fit.
- `sorted_set(xs)` and `heap(xs)` take a list, and `sorted_dict(d)` takes a dict.
- `m[k] op= v` on a `sorted_dict` does not insert `Default` first yet; R438 is implemented for
  `dict` only, so a missing key panics with `bounds`.
- The interpreter keeps the sorted types as sorted vectors, with O(n) insertion. No program can
  tell this from a B-tree except by timing.

## Std elaboration readings (Wave 2)

Assumed in `yong-elab`'s std loading until the spec says otherwise:

- `FsError` is a record with `path: str` and `reason: str`. 17.3 says only that each module's
  error type is a record or sum with `Show`, `Eq` and `Wire`. A missing file has reason
  `not found`.
- `fake_fs(files)` takes the map, and `with fs = fake_fs(...)` installs it as the `fs`
  capability for the block. `fs.read` of a path the map lacks fails with `FsError`.
- Like the prelude, `std.test` is in scope without an import, which extends assumption A7 from
  `fake_fs` to every `std.test` function.
- A std class reaches a module when the module's source names the class or the class's module
  (for example `fs`). Such a class is lowered into the module as a record, unless the module
  defines that name itself.
- `Error` is a record with `message: str`, the wrapped failure's `Show` text, and
  `context: list[str]`, outermost first. Keeping the text instead of the value means
  `e is HttpError(...)` cannot downcast yet. `?` into a function that fails with `Error`
  converts through `std.error.wrap`. `f().context(msg)` calls `std.error.context` on
  `try f()`: on success it gives the value, and otherwise it fails with an `Error` that has
  `msg` added.

## Wen lexical and grammar readings

15.1 and 15.2 leave these open. Assumed in `yong-wen` until resolved:

- **Signature continuation.** A line whose first token is `fail` or `uses` continues the previous
  logical line when that line is indented less, starts (after `pub`, `unsafe`, `gen`, `sub`,
  `lemma` or `safe`) with a name followed by `(` or `[`, has no `=` outside brackets and does not
  end in `->`. Other continuation lines start with `.`, `and`, `or` or a binary operator other
  than `@`, which would read as an attribute.
- **E134.** Inside a grid, a signed element is E134 when the element before it in the row or the
  signed operand is not a literal: `[1 -2]` is two elements, `[a -b]` and `[1 -b]` are E134. The
  span starts at the element before the sign.
- **SIGN and SPLPAREN outside grids.** The lexer marks them wherever the innermost bracket is
  `[`. In an index (`xs[i -1]`) they are read as a binary operator and a call; in a list after a
  `,` a SIGN is a unary sign.
- **Time literals.** A standalone time needs seconds (`09:00:00`), so `{10:20}` stays a map.
  Inside a date-time, `T09:00` may omit them.
- **Stage-0 calls.** `#` applies to the whole postfix chain after it: `#deep(0)` is the stage-0
  value of the call.
- **Grammar gaps.** Accepted although 15.2 does not list them: a labelled inout argument
  (`swap(x: &a)`), a bare value name as a type (`Ptr[c_int]`, the `core.ffi` C types), and a
  negative number literal as a pattern (`-1 -> ...`).
- **E112.** Two consecutive capitals are allowed only as a one-letter first word followed by a
  lower-case letter, so the `core.ffi` names `CStr` and `CString` lex, and `HTTPError` is E112.
