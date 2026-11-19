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

The reader is split into files that the check scripts concatenate, `main` file first: `use .sibling` is E017, and a
program whose `main` follows the helpers fails natively (#1744), so each main goes first.

Slice 1, `core_lex.yn` (library) and `core_lex_main.yn`: the lexer of `yan-core`, token for token, positions in bytes.
`core_lex_check.sh` compares its output on interpreter and native with `crates/yan-core/examples/lex_dump.rs` over
`conformance/yir` and `lex_edge/`.

Slice 2, `core_parse.yn` (AST and parser) and `core_parse_main.yn` (S-expression dump): the AST of `ast.rs` and the
parser of `parser.rs`, including error recovery and diagnostics. `core_parse_check.sh` compares the dump on interpreter
and native with `crates/yan-core/examples/parse_dump.rs` over `conformance/yir`, `lex_edge/` and `parse_edge/`
(every grammar form, and one file per error family), and requires `live: 0`.

The Yan AST keeps statement lists, types, right-hand sides and functions in `Pools` and refers to them by index.
Natively a type is expanded inline at every use and its drop and copy glue with it; the nested tree needed more than
10 GB to build, the pooled one 1.1 GB. Struct values are moved out of a `let` parameter or a field only after a
`.copy()`: the compiler does not reject the move and the native binary frees twice.

Slice 3, `core_print.yn` (printer) and `core_print_main.yn`: the canonical printer of `printer.rs`. `core_print_check.sh`
compares the printed text of every file the parser check covers, on interpreter and native, with
`crates/yan-core/examples/print_dump.rs` (the Rust printer), byte for byte, and requires `live: 0`. A `diag` line
(code, line, column) precedes the text of a file that has parse diagnostics.
