# Conformance suite

The executable form of the spec (section 24). Every file is one case, owned by the spec owner and
read-only to agents (22.6).

## Layout (24.1)

| Path | Kind | A case passes when |
|---|---|---|
| `yag/pass/`, `yn/pass/`, `yir/pass/` | pass | `check` reports no errors (warnings and notes allowed) |
| `yag/fail/`, `yn/fail/`, `yir/fail/` | fail | `check` reports exactly the `expect` lines, all severities, and nothing else |
| `yag/run/`, `yn/run/`, `yir/run/` | run | `run` prints exactly the `stdout` block and exits with `exit` (default 0) |
| `golden/` | run | as run; G1, G2 and G4 of 24.3, and the proposed G5 (the Tally slice, in Yan Agent and Yan) |
| `solver/` | fail | as fail; no `expect` line means no diagnostics at all (8.6 boundaries) |
| `ub/` | ub | `run --ub` reports `ub(N)` for the case's `ub: N` header (the 6.7 item number), or runs clean for `ub: none` |
| `patch/<case>/` | patch | `patch` applied to `m.yag` gives `result.yag`, or is refused with the `code` of `expect.toml` and leaves `m.yag` unchanged |
| `semver/<case>/` | semver | `semver` on `old/` and `new/` computes the `bump` of `expect.toml` (one pair per row of the 20.6 table) |
| `pkg/<case>/` | pkg | the `command` of `expect.toml`, run in `root/`, reports `code` (at `at` when given) and exits nonzero, or exits 0 with no errors when there is no `code` |
| `roundtrip/`, `sim/` | | not populated yet |

`yir/run/` is not in the 24.1 tree; M0 needs it because its interpreter runs Yan Core text before any
surface exists (question Q2). `pkg/` holds package and toolchain diagnostics, which need a package
tree, not one file (question Q8, R479).

## Directory cases

A `patch/` case holds `m.yag`, the patch as `patch.json` (the 18.3 JSON form) or `patch.yag` (the
plain-file form), and `expect.toml` with `spec`, optional `flags` (such as `["--check"]`) and either
`result = "result.yag"` or `code = "P00N"`. A `semver/` case holds `old/lib.yag`, `new/lib.yag`
and `expect.toml` with `spec` and `bump`. A `pkg/` case holds a package tree in `root/`, an optional
`base/` (an earlier version for `--base ../base`), and `expect.toml` with `spec`, optional `rows`,
`command` (the arguments after `CMD`), and optionally `code` and `at` (`file:line:col`, the file
relative to `root/`). The runner copies the case and runs the command in the copy's `root/`. The
runner adds these commands to the executor contract:

```
CMD run --ub [FLAGS] FILE                exit status; a UB report names ub(N) on stderr
CMD patch --format json [FLAGS] PATCH    run in the case's copied directory; JSON records on refusal
CMD semver --format json --base OLD NEW  a JSON object with "bump"
CMD ARGS... --format json                a pkg/ case's command, run in root/; JSON records
```

## Headers

Headers are the file's leading comment lines (`#` in Yan Agent, `--` in Yan and Yan Core):

```
# spec: 4.3, 9.2          sections the case tests
# rows: R342, SQ15        change rows and resolved SPEC_QUESTIONS it tests
# milestone: M2a          only when later than the surface default (yir M0, yag M1, yn M3)
# flags: --strict         extra arguments to check and run
# note: ...               why the expectation reads the spec this way
# expect: E402 at 11:18   one per expected diagnostic: code, line, column of the primary span
# stdout:                 expected output, one prefixed line per output line
# exit: 101
```

Lines and columns are 1-based and count the header lines. Columns are UTF-8 byte offsets, as in
the Yan Core text parser; every primary span here is preceded only by ASCII on its line, so code points
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
| A10 | A patch's `"module": "m"` names `m.yag` in the directory it runs in, and `base` may be omitted. |
| A11 | Raw pointers: `xs.as_ptr()`, `Ptr.of(&x)`, `p.offset(n)`, `p.cast[T]()` are safe to build; `p.read()` and `p.write(v)` need `unsafe`. |
| A12 | `@yan/i18n` is reached as `i18n` (`i18n.Locale("en")`), and `@messages` records give each message as a field default. |
| A13 | The `semver` and `patch` command lines above, and a UB report line `ub(N): message` on stderr with a nonzero exit. |
| A14 | The std.evented scheduler handler is `evented.sched()`, bound as `with sched = evented.sched():`. |
| A15 | `tests.lock` is TOML: `format = 1` and one `[[test]]` table per approved test with `path` (`module.item`, a module named by its path under `src/` without the suffix) and `hash` (`blake3:` and 64 hex digits). It is unsigned before M1. |
| A16 | `yan publish --dry-run` runs every publish check (20.6, 19.2) and uploads nothing. |
| Y1 | Yan Core text: the entry point is `fn main` of the file's module; a capability operation is a call such as `std.io.print(cap io, let s)`. |
| Y2 | Yan Core text: `ret` of a `sink` parameter or local consumes it. |
| Y3 | Yan Core text: std functions are called by full path (`std.list.len[i64](let xs)`, `std.list.new`, `std.list.append`, `std.result.is_ok`). |
| Y4 | Yan Core text: intrinsics are `<op>_<scalar>` (`add_checked_i64`, `floordiv_i64`, `mod_i64`, `div_i64` returning `f64`, `eq_f64`), and `exclusive_check(let i, let j)` is a statement. |
| Y5 | Yan Core text: `Result` matches as `Ok { value: v }` and `Err { error: e }`. |

Yan Core text lexing (`true`, `false`, `()`, literals, labels) follows the "Yan Core text lexical rules"
entry of SPEC_QUESTIONS.md; every `yan/` case parses with the Rust `yan-core` parser.

## Spec questions

Answered by the grammar work: `become postfix` is an `arg` in Yan Agent and Yan, and E143 comes from the
checker when it is not the last-evaluated argument; Yan subscripts are `subdef = "sub" sig "=" body`
with `yield expr`, and Yan Agent has no subscript definition. The cases follow the checked grammars of
spec rev 474 (R460 to R465), including the R465 spellings of `ghost`, `lemma` and `reveal`, and the R466 `@@prove` module
attribute (spec rev 479).

| Id | Question |
|---|---|
| Q1 | Decided 2026-10-31 (R467): a capability captured by an escaping closure is E306 only, not E406. |
| Q2 | Decided 2026-10-31 (R468): 24.1 gains `yir/run/` for M0 Yan Core text run cases. |
| Q3 | Decided 2026-10-31 (R469): E250 is reported at the second definition of the name. |
| Q4 | Decided 2026-10-31 (R470): a list literal's length is an 8.6 fact, so `xs = [1, 2, 3]; xs[3]` is E501. |
| Q5 | Decided 2026-10-31 (R471): a `par` branch reading another branch's write is E705 only, not E402. |
| Q6 | Decided 2026-10-31 (R472): `with deadline(t)` always requires `Timeout` in the fail type. |
| Q7 | Decided 2026-10-31 (R473): E422 goes on the subscript name and E423 on the yielded place. |
| Q8 | Decided 2026-10-31 (R479): 24.1 gains `pkg/` for multi-file package cases. |
| Q9 | Decided 2026-10-31: E006 is the registry's check of its recomputed capability set at publish, so it is tested with the registry double, not as a `pkg/` case. |

## Not covered yet

The other package codes need machinery the suite cannot hold as plain files:

- E003, E004, E005, E006, E007, E009, E011 need a registry, transparency log and Sigstore test double.
  The spec defines the index paths (19.3) but no local registry fixture format.
- E010 needs a build that differs between roots, which a conforming toolchain cannot produce
  from source.
- E012 is std CI's check, and the spec gives no command or flag that runs it on a package.
