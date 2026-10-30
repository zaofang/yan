# Spec questions

## Conformance header layout

Spec 20.1 shows `# expect: E501 at 14:9`, `# stdout:` blocks and `# exit: 101`, but not how a
`# stdout:` block is laid out, and `.ben` has no comment syntax yet (proposed SQ2 in the Technical
Implementation Spec).

Assumed in `tools/conform.py` until resolved: headers are the file's leading `#` lines; a
`# stdout:` block is the following `# `-prefixed lines (`#` alone is an empty line) up to the next
header or the first non-comment line; several blocks concatenate. `patch/` cases are directories
and are skipped until their format is defined.
