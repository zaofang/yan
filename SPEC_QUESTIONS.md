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
