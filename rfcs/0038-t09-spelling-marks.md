# 0038: Spelling marks for the T09 law 1 cases

- Status: accepted
- Research: none; `docs/reports/2026-11-05-law1-ledger.md` (Miscellaneous class) and `yan view --as yag --strict`
  on each case, 2026-11-05
- Rows: R863 to R868

## Summary

Four law 1 cases still fail and no accepted mark covers them: views_from, r832_if_else, cli_subcommands and
g2_vwap. Each fails because elaboration erases a fact and raise cannot tell it from author-written code.
Each D-item below is one new mark kind (or one extension of an accepted kind), written only by elaboration
and validated by raise as RFC 0034 requires. Each weighs dropping the case from law 1 instead. Expected
impact if all are accepted: law 1 gains four cases (the dict comprehension and `ex` parts of g2_vwap are
D4 and D5; the case passes only with both).

## Decisions

### D1: a `Hoisted` mark for a declaration elaboration inserted before an if or with

A name bound on every path of an `if`/`else` or in a `with` body is declared before the statement at its
type's zero (14.1). Raise prints `s = ""` because the declaration is an ordinary `let`. Origin marks
(`Inserted`) record what elaboration inserted but have no kind for this. Add `Hoisted` on the declaring
statement; raise drops the statement only if the next statement is an `if` or `with` that binds the name on
every path, else keeps it. Options: (a) the mark; (b) extend `Inserted` with a hoist kind (same fact, but
`Inserted` marks are consumed by trivia readers elsewhere); (c) drop r832_if_else from law 1 (it tests
visibility after the statement, so the printed form is the case; not a sane drop). Recommend (a).

- Decision: Accepted as recommended (Antonio, 2026-11-05)
- Row: R863

### D2: a `View` mark for a view type written in a signature

`View[T]` lowers to the owned type, so raise prints `list[int]` for `xs: View[int]`. Add `View` on the
parameter or result type node; raise validates that the printed type is the collection the view stands for.
Options: (a) the mark; (b) infer the view from a `from` clause (rejected: `from xs` on an owned parameter
is also legal, so the inference prints the wrong program); (c) drop views_from (it is the only case for
`View` in a signature, so no law 1 coverage remains). Recommend (a).

- Decision: Accepted as recommended (Antonio, 2026-11-05)
- Row: R864

### D3: a `Schema` mark for `cli.parse_args[T]`

`cli.parse_args[Tool](xs)` elaborates to `cli.parse_args(xs, "{...schema...}")`, the schema built from
`Tool`'s fields and `@help` text in elaboration's globals, which raise does not have. Add `Schema(type)` on
the call; raise prints `[type]` only if the type exists in the module and the literal's JSON `name` equals
it, else prints the call as is. The same applies to `cli.parse[T]()`. Options: (a) the mark; (b) move the
schema builder out of `yan-elab` so raise can recompute it (larger, and the builder reads elaboration
globals); (c) drop cli_subcommands (the only run case for sum-of-records subcommands, so not a sane drop).
Recommend (a).

- Decision: Accepted as recommended (Antonio, 2026-11-05)
- Row: R865

### D4: a `Comprehension` mark for a comprehension lowered to a loop

`[e for x in xs]` and `{k: v for ...}` lower to a fresh binding, an empty collection and an appending loop
(`c2 = []; for ...: c2.append(..)`). Add `Comprehension` on the binding statement; raise prints the
comprehension only if the loop has the exact shape elaboration produces (one `for`, one append or store, no
other statement, the binding read once afterwards), else prints the loop. Options: (a) the mark; (b) drop
the comprehension cases from law 1 by making `yan fmt` print the loop (rewrites every model comprehension,
so churn in diffs, not a sane drop). Recommend (a).

- Decision: Accepted as recommended (Antonio, 2026-11-05)
- Row: R866

### D5: carry `ex` examples and tuple parentheses

Two smaller losses in g2_vwap. `ex f(..) == v` is dropped by elaboration, so raise has nothing to print;
carry it in the item's trivia as text-free structure, or lower it to a `Contract::Example` raise already
handles (it handles that variant but elaboration never produces it). A parenthesized tuple on the right of
an assignment prints bare because tuple parentheses are not recorded; add a `Parenthesized` mark that raise
validates against the node being a tuple. Options: (a) both; (b) only the tuple mark and edit g2_vwap to
remove `ex` (not allowed: agents do not edit conformance); (c) drop g2_vwap. Recommend (a).

- Decision: Accepted as recommended (Antonio, 2026-11-05)
- Row: R867 and R868

## Spec changes

16.5 gains the new mark kinds. 14.1 and 4.5 gain a note that the printed form keeps a hoisted declaration
and a view type respectively. No change to semantics or hashes.
