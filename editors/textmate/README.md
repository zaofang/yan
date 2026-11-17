# Yan TextMate grammars

- `yan.tmLanguage.json`: Yan (`.yn`, spec 15.1), scope `source.yan`.
- `yag.tmLanguage.json`: Yan Agent (`.yag`, spec 13), scope `source.yag`. Covers `#`, `#:` and `#>` comments, docstrings, f, t, r, b and c strings with `{expr:spec}` interpolation, `def`, `class`, `type`, `trait`, `impl`, contract lines, `uses` lists, decorators, the built-in type words, and the keywords the lenient reader rejects (`invalid.illegal`).

Licensed Apache-2.0 with the repo.

Covers comments (`--`, `---` doc, `-->` watch), strings (plain, raw, bytes, C, char, triple-quoted, `\(expr:spec)` interpolation), numbers with unit suffixes, dates and times, attributes, declarations, reserved and soft keywords, and operators.

Samples are in `samples/` (`tally.*` copies `examples/tally/g5_tally.*`; `literals.*` are lexical tours). `editors/vscode/test/` tokenizes them with `vscode-textmate` and compares against `editors/vscode/test/snapshots/`; it also fails when a scope the grammars emit is not styled by the themes. Run `npm test` there; `npm run snapshots` rewrites the snapshots after an intended change. The VS Code extension that ships these grammars is `editors/vscode/`.

Not covered: Yan Shell `.ynsh`, Yan Meta `.ymt`, Yan Binary Format `.ybf`. Each needs its own grammar.
