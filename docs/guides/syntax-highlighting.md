# Syntax highlighting for Yan on GitHub

## Now

- Grammar: `editors/textmate/yan.tmLanguage.json`, samples in `editors/textmate/samples/`.
- `.gitattributes` sets `*.yn linguist-language=Elm`. Of the languages with `--` comments, Elm was the only one that also colours `---` doc comments, `-->` lines, PascalCase types, sum types, `True`/`False`, `if/then/else` and `let` (Lua and Ada also took the comments but miss types or keywords). Interpolation `\(expr)` and soft keywords (`match`, `uses`, `pre`, `ex`) are not coloured, and Elm will show in the language bar. This is a stopgap; remove the line when Yan lands in linguist.
- Works on private repos: the attribute is read by github.com's linguist at render time.

## Follow-ups

- `.yag` (Yan Agent): Python-like surface, own grammar.
- `.ynsh` (Yan Shell), `.ymt` (Yan Meta, after M1), `.ybf` (Yan Binary Format): own grammars once their syntax is frozen.

## Steps for a github-linguist PR (blocked while the repo is private)

1. Usage threshold: linguist asks for roughly 200 public repositories or users with files of the extension, searched on github.com. Not reachable while the repo is private (decision: repo stays private).
2. Add the grammar as a git submodule under `vendor/grammars/` from a public repo with a permissive licence (Apache-2.0 here), then run `script/add-grammar`.
3. Add the `Yan` entry to `lib/linguist/languages.yml`: `type: programming`, `extensions: [.yn]`, `tm_scope: source.yan`, `ace_mode: text`, `color`, `language_id` from `script/update-ids`. Keep it alphabetical.
4. Add at least two real sample files from separate sources under `samples/Yan/` (here: from `editors/textmate/samples/`, all Apache-2.0), plus heuristics in `lib/linguist/heuristics.yml` if `.yn` collides with another language.
5. Run `script/update-ids`, `script/licensed`, `bundle exec rake test`; follow the PR template and `CONTRIBUTING.md` of github/linguist.
6. Do not open the PR until the threshold is met and the lead approves.
