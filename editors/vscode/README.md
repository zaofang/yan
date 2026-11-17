# Yan for VS Code

Syntax highlighting, language configuration, file icons, snippets and the Yan colour themes for Yan (`.yn`) and Yan Agent (`.yag`).
Stage E1 of the editor plan (`docs/research/2026-11-17-editor-and-lsp-plan.md`): no language server yet.

The extension has no code. `tools/vscode_ext.py` assembles it from single sources:

| In the extension | Source |
| --- | --- |
| `syntaxes/` | `editors/textmate/` |
| `themes/` | `editors/themes/` (generated from `brand/tokens.json`) |
| `icons/` | `brand/` |
| `package.json`, `languages/`, `snippets/` | this directory |

## Develop

```
cd editors/vscode && npm ci
npm test                  # tokenization snapshots and theme coverage
npm run snapshots         # rewrite snapshots after an intended grammar change
cd ../.. && uv run python -m tools.vscode_ext package
```

`package` writes `editors/vscode/build/yan-<version>.vsix`. Install it with `code --install-extension`.
The same file goes to the VS Code Marketplace (`vsce publish --packagePath`) and Open VSX (`ovsx publish`).
Publishing needs the publisher account, the extension name and the icon terms, which the lead owns (plan D10); nothing in this repository publishes.

## Grammar notes

TextMate scans lines, so the grammar approximates the layout-sensitive parts of the spec:

- `pre`, `post`, `ex`, `invariant`, `decreases` and `cost` are scoped as keywords at the start of a line unless an assignment operator, `.`, `,`, `:` or a closing bracket follows (13.1, R462). A later line that uses one as a name is still coloured as a keyword.
- Contract lines end at the line end; a contract expression continued inside brackets over several lines loses its contract scopes on the continuation lines.
- `try`, `except`, `finally`, `raise`, `global`, `nonlocal`, `del`, `yield`, `async` and `await` carry `invalid.illegal` (E120, E121).
- `.yd` is not registered: its extension spelling is open in RFC 0084 (plan D7).
