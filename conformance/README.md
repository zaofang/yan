# Conformance suite

The executable form of the spec (section 24). Every file is one case, owned by the spec owner and
read-only to agents (22.6).

## Layout (24.1)

| Path | Kind | A case passes when |
|---|---|---|
| `jian/pass/`, `wen/pass/`, `yong/pass/` | pass | `check` reports no errors (warnings and notes allowed) |
| `jian/fail/`, `wen/fail/`, `yong/fail/` | fail | `check` reports exactly the `expect` lines, all severities, and nothing else |
| `jian/run/`, `wen/run/`, `yong/run/` | run | `run` prints exactly the `stdout` block and exits with `exit` (default 0) |
| `golden/` | run | as run; G1, G2 and G4 of 24.3 |
| `solver/` | fail | as fail; no `expect` line means no diagnostics at all (8.6 boundaries) |
| `ub/`, `semver/`, `patch/`, `roundtrip/`, `sim/` | | not populated yet |

`yong/run/` is not in the 24.1 tree; M0 needs it because its interpreter runs Yong text before any
surface exists (question Q2).

## Headers

Headers are the file's leading comment lines (`#` in Jian, `--` in Wen and Yong):

```
# spec: 4.3, 9.2          sections the case tests
# rows: R342, SQ15        change rows and resolved SPEC_QUESTIONS it tests
# milestone: M2a          only when later than the surface default (yong M0, jian M1, wen M3)
# flags: --strict         extra arguments to check and run
# note: ...               why the expectation reads the spec this way
# expect: E402 at 11:18   one per expected diagnostic: code, line, column of the primary span
# stdout:                 expected output, one prefixed line per output line
# exit: 101
```

Lines and columns are 1-based and count the header lines. Columns are UTF-8 byte offsets, as in
the Yong text parser; every primary span here is preceded only by ASCII on its line, so code points
give the same numbers.

The primary span starts at the construct the code's meaning names: the offending argument for
E402, E404 and E405, the use for E401, E413 and E705, the item name for item-level rules (E421,
E602, E601, W830), and the start of the whole expression for operator rules (E101, E104, E530).

`manifest.json` lists every case with its headers, and `COVERAGE.md` maps diagnostic codes and spec
sections to cases. Both are generated from the headers and `catalogue.toml`:

```sh
uv run python tools/conform.py --manifest conformance/manifest.json --coverage conformance/COVERAGE.md
```

## Assumptions

Where the spec does not fix a name or a std API that a case needs, the case says so in a `note`
naming one of these. Each is a proposed answer; change the cases if the spec decides otherwise.

| Id | Assumption |
|---|---|
| A1 | `io.print(s)` writes `s` and a newline. |
| A2 | A `list[T]` argument coerces to a `View[T]` parameter. |
| A3 | `Channel[T](n)` returns `(Sender[T], Receiver[T])`. |
| A4 | Manifest fields in a script's `package` constant are spelled as in the 19.1 table, with `capabilities` a list of names. |
| A5 | The std unit values (`m`, `s`, `ms`, `day`) are in scope unqualified, as in 13.1's `150 * ms`. |
| A6 | `wrap_as` and `sat_as` are generic functions, called as `wrap_as[u8](x)`. |
| A7 | `fake_fs(files)` takes a `dict[str, str]`; `fs.read(path)` returns `str fail FsError`. |
| A8 | `Max(v)` builds a `Max[T]` lattice value and `.value` unwraps it. |
| A9 | A case file inside a directory named `ffi` is checked as an ffi module (12). |
| Y1 | Yong text: the entry point is `fn main` of the file's module; a capability operation is a call such as `std.io.print(cap io, let s)`. |
| Y2 | Yong text: `ret` of a `sink` parameter or local consumes it. |
| Y3 | Yong text: std functions are called by full path (`std.list.len[i64](let xs)`, `std.list.new`, `std.list.push`, `std.result.is_ok`). |
| Y4 | Yong text: intrinsics are `<op>_<scalar>` (`add_checked_i64`, `floordiv_i64`, `mod_i64`, `div_i64` returning `f64`, `eq_f64`), and `exclusive_check(let i, let j)` is a statement. |
| Y5 | Yong text: `Result` matches as `Ok { value: v }` and `Err { error: e }`. |

Yong text lexing (`true`, `false`, `()`, literals, labels) follows the "Yong text lexical rules"
entry of SPEC_QUESTIONS.md; every `yong/` case parses with the Rust `yong-core` parser.

## Spec questions

Answered by the grammar work: `become postfix` is an `arg` in Jian and Wen, and E143 comes from the
checker when it is not the last-evaluated argument; Wen subscripts are `subdef = "sub" sig "=" body`
with `yield expr`, and Jian has no subscript definition. The cases follow the checked grammars of
spec rev 474 (R460 to R465), including the R465 spellings of `ghost`, `lemma` and `reveal`, and the R466 `@@prove` module
attribute (spec rev 479).

| Id | Question |
|---|---|
| Q1 | Decided 2026-10-31: a capability captured by an escaping closure is E306 only, not E406. |
| Q2 | Decided 2026-10-31: 24.1 gains `yong/run/` for M0 Yong text run cases. |
| Q3 | Decided 2026-10-31: E250 is reported at the second definition of the name. |
| Q4 | Decided 2026-10-31: a list literal's length is an 8.6 fact, so `xs = [1, 2, 3]; xs[3]` is E501. |
| Q5 | Decided 2026-10-31: a `par` branch reading another branch's write is E705 only, not E402. |
| Q6 | Decided 2026-10-31: `with deadline(t)` always requires `Timeout` in the fail type. |
| Q7 | Decided 2026-10-31: E422 goes on the subscript name and E423 on the yielded place. |
