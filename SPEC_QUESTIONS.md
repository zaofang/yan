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
