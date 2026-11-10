# 0064: Trim the release-candidate scope to the polish floor

- Status: accepted (D1, lead card 2026-11-09)
- Research: [Yan pre-1.0 polish plan](https://claude.ai/artifact/LqVkrNAL8pERD5rGacTrku); issue #875, tracked by #862.
- Rows: R990, R991

## Summary

The M3 list in section 23 carries tools and studies that a first release does not need: reactive
notebooks, live values, depth-lens inlay hints, the item-level merge driver, the full book and the
randomized syntax comparison. The polish plan keeps the language server, the Yan tree-sitter
grammar, the tour, by-example pages and the std reference, and the REPL. This RFC moves the rest to a
new "1.x" bucket of section 23, to be built after 1.0 under the release train of 20.7.

## Decisions

### D1: move six items from M3 to 1.x

Moved: reactive notebooks and live values computed on examples (#382, the `yan nb` command);
depth-lens inlay hints; the item-level merge driver (#383 keeps the tree-sitter grammar in M3); the
full book (#386 keeps the tour, by-example pages and the std reference); the randomized comparison
on contested Yan syntax (#385). `yan repl` stays in M3.

Options: (a) move them to 1.x with no gate; (b) delete them; (c) keep them in M3. (a) keeps the
design and drops the schedule; (b) loses accepted design; (c) delays the first release for tools
nobody needs to start with. Recommended (a).

Consequences that follow from (a):

- R969 and 22.7 keep compiler sources off the contested forms until the comparison reports. The
  comparison now reports after 1.0, so that constraint lasts through the 1.0 compiler.
- Law 11 of section 2 already makes post-1.0 surface changes depend on that comparison, so no row
  there changes.
- The docs gate (R319) covers the fences of whatever learning material exists; the book's fences join
  it when the book lands.

- Decision: accepted, lead card 2026-11-09
- Row: R990 (scope in 23, 18 and 22), R991 (the 1.x bucket and what gates entry to it)

## Spec changes

- 23: M3 list and the docs gate lose the six items; new `### 1.x · After 1.0` section after RC holds them.
- 18: `yan merge-driver` and `yan nb` rows read 1.x; `yan repl` stays M3; the learning set marks the book 1.x.
- 13: the watch-expression sentence names the language server only.
- 22: R969 text says the comparison reports after 1.0.
