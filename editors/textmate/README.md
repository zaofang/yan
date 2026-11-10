# Yan TextMate grammar

`yan.tmLanguage.json` highlights Yan (`.yn`, spec 15.1). Scope name `source.yan`. Licensed Apache-2.0 with the repo.

Covers comments (`--`, `---` doc, `-->` watch), strings (plain, raw, bytes, C, char, triple-quoted, `\(expr:spec)` interpolation), numbers with unit suffixes, dates and times, attributes, declarations, reserved and soft keywords, and operators.

Check it with the samples in `samples/` (`tally.yn` is a copy of `examples/tally/g5_tally.yn`; `literals.yn` is a lexical tour). Tokenize with `vscode-textmate` and `vscode-oniguruma`, or open the folder in VS Code with a one-line extension manifest pointing at the grammar.

Not covered: Yan Agent `.yag`, Yan Shell `.ynsh`, Yan Meta `.ymt`, Yan Binary Format `.ybf`. Each needs its own grammar.
