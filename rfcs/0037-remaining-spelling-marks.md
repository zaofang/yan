# 0037: Remaining spelling marks for law 1

- Status: accepted
- Research: none; `docs/reports/2026-11-05-law1-ledger.md`, `yan view --as yag --strict` on main at 5abf3e2
  for each case below, and a read of `crates/yan-elab/src/lower.rs` (`for_`, `assign`, `builtin`, `arith`)
  on 2026-11-05
- Rows: R857 to R861

## Summary

After T05, T06 and T07 merged, eight law 1 cases still fail because elaboration erases a spelling that no
mark records: `range(0, n)`, `s + "!"`, the N36 widenings, tuple assignment, `divmod` and `pow`, and the
`**` operator. Each D-item below is one mark or one decision. Each also weighs the alternative of taking the
case out of law 1, either by making `yan fmt` rewrite the spelling (the case then stops being canonical and
law 1 skips it) or by editing the case text. Dropping is a sane choice only where the spelling is incidental
to what the case tests; the per-item notes say where. Expected impact if every recommendation is accepted:
law 1 gains all eight (concat, n36_char_as_str, proven_by_guard, assign_return_swap, prelude_divmod_pow,
prelude_divmod_zero, bigint_pow_int, n38_int_as_float).

## Decisions

### D1: an `Infix` mark for `+` on strings

`s + "!"` and `f"{s}!"` both lower to the same `std.text.concat` chain (R478), and raise prints the
f-string. 14.5 already says the printer chooses f-strings "where the source used them", which needs a fact
to read. Add `Infix` on the first `concat` call of a chain written with `+`; raise validates that every part
is a `str` operand with no format spec. Options: (a) the mark; (b) make `yan fmt` rewrite `a + b` on strings
into an f-string, so the two cases stop being canonical (contradicts 14.5 and rewrites ordinary model
output, so not a sane drop); (c) edit `strings/concat` to use an f-string (loses the only run case for str
`+`). Recommend (a). Cases: strings/concat, and the `shout`, `join` and `pair` lines of n36_char_as_str.

- Decision: Add `Infix` (Antonio, 2026-11-05). R856 (#320) had meanwhile added `Operator` for `**`, the
  same fact; the row extends `Operator` to concat chains instead of adding a second kind
- Row: R857

### D2: `Lenient(code)` on statements and atoms

R853 puts `Lenient(code)` on a call. N36 also rewrites a `char` literal into a one-character `str` literal
(`x: str = c'x'` prints `"x"`) and a `list[char]` into an element-by-element loop (`return cs` prints four
statements). Extend R853 so the mark may sit on an atom or on the first statement of a rewrite; raise
reprints the alias only if re-applying the code's rewrite to the alias reproduces the node, or the
statements up to the binding the alias uses. No new mark kind. Options: (a) extend R853; (b) a separate
`Widened` mark (rejected: same fact as `Lenient(N36)`); (c) drop n36_char_as_str from law 1 (it is a
lenient case, so its text is by design not what canonical output looks like; but the lenient spellings
are exactly what models write, and law 1 is the check that they survive a view). Recommend (a). Case:
n36_char_as_str, with D1.

- Decision: Extend R853, no new mark kind (Antonio, 2026-11-05)
- Row: R858

### D3: an `Explicit` mark for a written default argument

`range(0, n)` and `range(n)` lower to the same counting loop (`for_`, lower.rs:2554). Add `Explicit` on the
loop statement (and on a `std.prelude.range` call outside a loop) when the start was written; raise
validates that the start is the literal `0`. The mark is general: it means "an argument equal to the
parameter's default was written", so later defaults can reuse it. Options: (a) the mark; (b) make
`yan fmt` rewrite `range(0, n)` to `range(n)`, so proven_by_guard stops being canonical and law 1 skips it
(sane: one meaning, and Python linters make the same rewrite, but models write `range(0, n)` often and the
rewrite would show up as churn in their diffs); (c) edit proven_by_guard to `range(len(xs))` (sane: the
case tests a bounds proof, not the spelling, but the spelling then has no law 1 coverage). Recommend (a).
Case: proven_by_guard.

- Decision: Add `Explicit` (Antonio, 2026-11-05)
- Row: R859

### D4: a `Parallel` mark for tuple assignment

`a, b = b, a` lowers to snapshot bindings and then one assignment per target (R216, `assign`,
lower.rs:3156). Raise inlines the snapshots and prints `a = b` then `b = a`, which is a different program;
the bare companion would also fail. Add `Parallel` on the first snapshot binding; raise validates k
snapshot bindings followed by k assignments to the targets, and prints `t1, .., tk = e1, .., ek`.
Independent of the mark, raise must keep the snapshots when the mark is absent or dropped, so the default
print is correct; that is a code fix inside this item. Options: (a) the mark; (b) lower a tuple assignment
to one tuple value and a destructure, which raise already prints as `x, y = e`, needing no mark (changes
the Yan Core of every tuple assignment and its item hashes, and relies on the optimizer to remove the
tuple); (c) drop assign_return_swap (not sane: tuple assignment is the case's subject). Recommend (a).
Case: assign_return_swap.

- Decision: Add `Parallel`, and raise keeps snapshots without it (Antonio, 2026-11-05)
- Row: R860

### D5: a `Prelude` mark for `divmod` and `pow`

The prelude `divmod(a, b)` expands to `(a // b, a % b)` with each operand bound once, and `pow(a, b)` to
the power call `**` would produce (R801, `builtin`, lower.rs:7449). Add `Prelude` on the tuple or the power
call; raise validates by expanding the call again and comparing. Options: (a) the mark; (b) make `divmod`
and `pow` real generic functions in Yan Core, inlined by the optimizer, so raise prints the call with no
mark (needs a trait over the operand types `//`, `%` and `**` accept, and changes R801 lowering); (c) drop
the two cases (not sane: `divmod` and `pow` are their subject, and `yan fmt` cannot rewrite
`divmod(next(8), 4)` to a tuple without a temporary). Recommend (a). Cases: prelude_divmod_zero, and
prelude_divmod_pow with D6.

- Decision: Add `Prelude` (Antonio, 2026-11-05)
- Row: R861

### D6: the `**` operator

`**` lowers to `int_pow`, `bigint_pow`, `math.pown` or `math.pow` by operand types (`arith`,
lower.rs:5328, 5513), and raise prints the call. Options: (a) `Infix` (the kind from D1) on the power
call: written as `**`; raise validates that the callee is one of the four; (b) invert the default: raise
prints `**` for these four calls, and an explicit `math.pown(f, n)` or `math.pow(x, y)` call carries the
existing `Function` mark (fewer marks in typical code, but `Function` today means "not a method call", so
it would gain a second meaning); (c) drop bigint_pow_int and n38 (not sane: `**` is canonical per 14.1, so
canonical code would fail law 1 everywhere). Recommend (a). Cases: bigint_pow_int, n38_int_as_float, and
the `pow` line of prelude_divmod_pow with D5.

- Decision: `Infix` on the power call (Antonio, 2026-11-05); already specified as `Operator` by R856 (#320)
- Row: R856 (no new row)

## Spec changes

16.5: `Operator` covers concat chains (R857), `Lenient(code)` covers atoms and statements (R858), the table
gains `Explicit`, `Parallel` and `Prelude` (R859 to R861), and the validation list gains their checks. 17.2
(R801): `divmod` and `pow` record `Prelude`. The RFC 0034 plan gains one step per accepted item, one thread
each.
