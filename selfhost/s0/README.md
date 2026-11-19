# S0: compiler-shaped readiness program (#852)

`lex.yag` lexes the source files named on its command line into an arena of tokens, counts token
kinds in a dict and prints a sorted summary. It exercises `fs.read`, `env.args`, lists of records,
dict updates and string/char iteration under native AOT.

```sh
files=$(find conformance/yag/run -name '*.yag' | sort)
yan run selfhost/s0/lex.yag -- $files > interp.out
yan build selfhost/s0/lex.yag -o lex && YAN_REPORT_LIVE=1 ./lex $files > native.out   # live: 0
cmp interp.out native.out
```

Heap elements read out of a list must be copied before they are stored (`arena.append(t.copy())`);
see the open hole on #852.

## Readiness programs (#1505)

Each program runs on interpreter and native with identical output and `live: 0`.

| Item | Program |
|---|---|
| 1 recursive sums, moving match; 7 `par for` in input order | `expr.yn` |
| 2 index tree, newtype handle, generic functions | `tree.yn` |
| 3 `Map` with str and newtype keys, interning (insertion order) | `intern.yn` |
| 4 char classification, string building | `text.yn` |
| 5 `Result` through layers, `inout` sink; 6 traits, closures, constants | `layers.yn` |

`Set`, generic sums, `Bytes` iteration, `u8` to `int` and `fs.list_dir` now work on both backends (rechecked 2026-11-19).

## Yan Core reader (#1505)

Slice 1, `core_lex.yn`: the lexer of `yan-core`, token for token, positions in bytes. `core_lex_check.sh` compares its
output on interpreter and native with `crates/yan-core/examples/lex_dump.rs` over `conformance/yir` and `lex_edge/`.
Parser and printer follow in slices 2 and 3. `main` is declared first in the file: see the order-dependent native
failure filed under #449.
