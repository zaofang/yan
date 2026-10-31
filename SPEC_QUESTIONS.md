# Spec questions

## Conformance header layout

Spec 24.1 shows `# expect: E501 at 14:9` (Wen and Yong: `-- expect:`), `# stdout:` blocks and
`# exit: 101`, but not how a `stdout:` block is laid out.

Assumed in `tools/conform.py` until resolved: headers are the file's leading comment lines (`#` in
Jian, `--` in Wen and Yong); a `stdout:` block is the following comment lines with one space
stripped (a bare marker is an empty line) up to the next header or the first non-comment line;
several blocks concatenate. `patch/` cases are directories and are skipped until their format is
defined.
