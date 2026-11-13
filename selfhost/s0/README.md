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
