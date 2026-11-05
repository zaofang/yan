# 0035: Docs on parameters, fields, variants, dict entries and tuple positions

- Status: draft
- Research: `docstrings-survey.md` (project studies folder, 2026-11-05); a read of 13, 15, 16.5 and the doc
  model of 18.9
- Rows: filled in when accepted

## Summary

A doc attaches today to a module, function, class or trait, and in Yan to the item or method below a
` --- ` comment. Python models also document parameters (`Args:`), attributes (a string after the field)
and variants, and a reader of a record literal wants a note beside an entry. This RFC gives each of
these positions a doc: one hash-neutral `Doc` trivia node per position, keyed by item hash and a node
path from the RFC 0034 walker, with the spellings below. Nothing is added to the hashed tree. The lenient
reader accepts the Python styles (`:param:`, Google, NumPy, PEP 257 attribute strings) and canonicalizes
them, as the Python-fidelity policy requires.

## Decisions

### D1: a sub-item doc is a Doc node at a node path

The doc model is unchanged: CommonMark plus the existing additions, first sentence is the summary. Only
the anchor is new. The walker of RFC 0034 gains paths for nodes it does not visit yet. The root of a path
is the item, so a type item and a function item do not clash on `[0, k]`:

| Node | Path |
|---|---|
| field k of a record or class | `[0, k]` |
| variant k of a sum | `[0, k]`; field j of that variant `[0, k, j]` |
| associated type k of a trait | `[0, k]` |
| entry k of a dict, list or tuple literal, at literal path p | `p` then `k` |

A parameter keeps `[0, k]` of RFC 0034. A dict entry is one node (key and value together), so a doc
describes the entry. Docs stay out of the hash: they are not item attributes, because the survey did not
verify how field and variant attributes are hashed, and a doc there would change the item hash. A test
shows that adding or removing a doc at any position leaves every item hash unchanged.

Recommendation: reuse the one trivia kind and the one walker.

- Decision: (lead, date)
- Row: (R number once accepted)

### D2: Yan Agent writes sub-item docs as `#:` lines; Yan writes `---`

In Yan Agent, a run of `#:` lines (each followed by a space and text) on the lines directly above a
field, a dict or tuple entry, a list element or an associated type is its doc. Plain `#` comments keep
their meaning, so existing comments do not become docs. In Yan the position takes ` --- ` lines, as an
item does. A literal that holds a documented entry is written one entry per line; the formatter breaks a
one-line literal when any entry has a doc.

The lenient reader accepts a PEP 257 attribute string: a prefix-free string statement on its own line
directly after a field, and canonicalizes it to `#:` lines above the field. It also accepts the `Attributes:`
section of a class docstring (Google and NumPy forms) and moves each entry to its field. A string that
could be an expression statement elsewhere is a doc only after a field. A doc and a plain comment may sit
above the same node; the doc must come last.

Options weighed: plain `#` (every note above a field, including a TODO, becomes a doc); a PEP 257 string
for fields and a comment for the rest (two spellings, and a string statement in a class body is not an
expression the printer can place for entries). Recommendation: `#:`.

- Decision: leading `#:` comment (lead, 2026-11-05)
- Row: (R number once accepted)

### D3: parameter docs come in two spellings, and the choice is kept

Spelling A is an `Args:` tagged section in the function doc: one `name: text` entry per parameter, with
continuation lines indented. Spelling B is a doc above the parameter in a signature broken one parameter
per line (`#:` lines in Yan Agent, ` --- ` in Yan). Both elaborate to the same Doc node at `[0, k]`.
Which spelling the author used is a spelling mark of RFC 0034 on that node, so law 1 holds for both; a
node without a mark prints as A. A function may mix them for different parameters but not both for one
(an error at the parameter). Section entries print in parameter order. The formatter breaks the signature
when a parameter uses B.

The lenient reader accepts `:param name:` (reST), `Args:` and `Arguments:` (Google) and `Parameters` with
dashes (NumPy), including a type written there, and canonicalizes them to A. A written type that agrees
with the signature is dropped; one that disagrees is a diagnostic, never a silent drop.

- Decision: both spellings, the choice preserved (lead, 2026-11-05)
- Row: (R number once accepted)

### D4: variant docs are a `Variants:` section in the type doc

A sum is written on one line (`type S = A(x: int) | B`), so a variant has no line to carry a doc. The doc
of the type holds a `Variants:` tagged section: `Name: text` for a variant, `Name.field: text` for a field
of an inline variant. Entries elaborate to Doc nodes at the variant and field paths of D1 and print back in
declaration order. The lenient reader accepts a PEP 257 string after a variant of a Python-style enum and
the `Members:` form, and canonicalizes them to the section. The grammar does not change.

Options weighed: a multi-line variant layout with a doc above each variant (docs sit beside variants, but
the grammar changes in both surfaces); both. Recommendation was both; the lead chose the section alone.

- Decision: `Variants:` section in the type doc (lead, 2026-11-05)
- Row: (R number once accepted)

### D5: tooling

`yan doc` shows each sub-item doc in the table of its parameters, fields or variants, and the agent view
carries them in the same untrusted channel as item docs. `yan doc --coverage` counts sub-item docs beside
item docs. An `Args:`, `Attributes:` or `Variants:` entry whose name matches no parameter, field or
variant is a lint, always on, like the existing check that `Fails:` and `Uses:` agree with the signature.
The warning for a public item without a doc is unchanged. A second lint, off by default and enabled in
`yan.toml`, warns for each public parameter, field and variant without a doc.

- Decision: opt-in lint for missing sub-item docs (lead, 2026-11-05)
- Row: (R number once accepted)

## Not proposed

Return docs (the type carries them; the lenient reader keeps a `Returns:` section as prose), docs on
capability declarations, doc attributes, and docs on closure parameters (closure bodies are not walked).

## Staged plan

One thread per step, in order. Each step runs `tools/conform.py --laws` locally and reports the law 1
counts against the current baseline; no step lowers the pass count, and none edits `tests/` or
`conformance/`.

1. **Paths.** Extend `yan_core::path::walk` with D1's nodes; selftests on a record, a sum with inline
   fields, a trait, and nested dict, list and tuple literals. Accept: hash-neutrality test of D1.
2. **Fields in both surfaces.** `#:` and ` --- ` on fields, lowered to Doc nodes, printed back; the PEP 257
   string accepted by the lenient reader. Accept: round trip on new cases supplied in a separate
   conformance PR.
3. **Parameters.** The `Args:` section, spelling B, and the mark. Accept: law 1 on both spellings.
4. **Variants.** The `Variants:` section.
5. **Entries and positions.** Dict, list and tuple docs and the formatter's literal break.
6. **Lenient reader styles.** reST, NumPy and Google forms of D2 to D4, each with an alias diagnostic.
7. **Tooling.** `yan doc`, coverage, the two lints.

## Spec changes

- 13.1 and the Yan Agent grammar: the `#:` doc line above fields and entries.
- 15: the ` --- ` doc above fields, entries and parameters; one entry per line in documented literals.
- 16.5: the new paths in the walker table and the spelling mark of D3.
- 14.4: the lenient-reader forms of D2 to D4.
- 18.9: `Args:`, `Attributes:` and `Variants:` tagged sections, sub-item docs in `yan doc`, coverage and
  the two lints (section 25 gets the lint codes).
