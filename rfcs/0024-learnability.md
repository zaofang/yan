# 0024: Learnability for Python and TypeScript developers

- Status: accepted (the lead declined D1 and accepted D2 to D5 on cards, 2026-11-01)
- Research: Yan Agent and Yan learnability
- Rows: R655 to R664

## Summary

The learnability study ran 88 everyday Python idioms, 15 Yan Agent tasks written the way a Python developer
writes them, and 11 Yan tasks written by a TypeScript developer. Most of the friction it found is
implementation lag behind the spec. The rest comes from five places where the spec itself gets in a
newcomer's way: `print` takes only `str`; an integer literal never types as a float; common Python
idioms (`max(xs)`, `sorted(xs, key=len)`, `"-" * 40`, `[0] * n` and nested `def`) have no Yan Agent form;
nothing names a source-language habit when a diagnostic is triggered by one; and Yan's TypeScript
traps (`${x}`, `=>`, `let`) go unexplained. This RFC fixes those five. The study's implementation bugs
are fixed in separate PRs.

## Decisions

### D1: a learnability gate at M1 and M3

Check the study's habit corpora in as conformance cases. Gate M1 (Yan Agent) and M3 (Yan) on three
conditions: no silent wrong result, a fix or habit note on every rejection, and no "not supported
yet" on a spec'd form. Add a timed study with 6 to 8 developers at M3.

- Decision: declined ("No gate", 2026-11-01). The corpus stays a study artifact in project files.
- Row: none

### D2: `print` takes any `Show` value

`io.print[T: Show](x: T)` and `io.eprint[T: Show](x: T)` print `show(x)`. A `str` prints unchanged,
as before. In the study, `print(n)` failed in 3 of 15 Python tasks. Printing a number is every
learner's second program.

Options weighed: keeping `str` only and adding a fix to `str(n)`. That leaves the first-hour error in
place for no safety gain.

- Decision: accepted ("Any Show", 2026-11-01)
- Row: R655

### D3: exact integer literals as floats, and `float ** int`

An integer literal also takes a binary float type (`f16`, `bf16`, `f32`, `f64`, `f128`) when its
value is exactly representable in that type: `1.5 * 2`, `x * 2` for a float `x`, and `f(3)` for a
`float` parameter. Variables still never convert. `x ** n` with `x` a float type and `n` of any
integer type is defined as C23 `pown(x, n)` from the bundled libm, correctly rounded (11.1). Today
each of these is E201 with no fix. Python, TypeScript, Go and Swift accept all of them.

- Decision: accepted ("Literals plus float ** int", 2026-11-01)
- Rows: R656 (literals), R657 (`**`)

### D4: Python idiom parity in Yan Agent

1. `min` and `max` also take one iterable: `max(xs, key=None, default=...)`. On an empty iterable
   with no `default`, they panic with kind `value`, as Python raises `ValueError`. With
   `default=d`, they return `d`. The two-argument positional form stays.
2. `sorted(xs, key=None, reverse=False)` and `xs.sort(key=None, reverse=False)` are stable for any
   `reverse`.
3. `*` between a `str` or `list[T]` and an `int`, in either order, repeats the sequence. A count of 0
   or less gives an empty sequence.
4. Lenient reader: a `def` nested in a function body that reads no local of the enclosing function
   is lifted to a module-level function. The lifted function keeps its name when that name is free
   at module level, and is named `<outer>_<inner>` otherwise. A nested `def` that captures a local
   stays an error and carries a habit note (D5).

- Decision: accepted ("All four", 2026-11-01)
- Rows: R658 (`min` and `max`), R659 (`sorted` and `sort`), R660 (repetition), R661 (N31)

### D5: habit notes, a Yan `${name}` lint and a TypeScript guide

1. A diagnostic triggered by a known Python or TypeScript habit carries a `habit` child. The child
   names the source language, the habit, and the Yan Core form. The toolchain ships a habit table that
   covers at least the study's corpus. Examples: `class E(Exception)`, `with open(...)`, nested
   `def`, `print(n)`, `try`, `=>`, `${x}`, and `let` meant as mutable.
2. Yan lint: a non-raw string literal that contains `${` followed by a name or expression and `}`
   gets a warning. The fix rewrites it to `\(...)`.
3. R319's learning set gains a TypeScript migration guide, beside the Python and Rust guides.

- Decision: accepted ("All three", 2026-11-01)
- Rows: R662 (habit notes), R663 (W854), R664 (TypeScript guide)

## Spec changes

- 3.4: R656.
- 11.1: R657.
- 14.1: R655 (Effects), R656 and R657 (Numbers), R660 (Numbers).
- 14.4: N31 (R661).
- 15.1, 15.5: W854 (R663), habit notes in Yan prose (R662).
- 17.1: R655 (`io` row).
- 17.2: R658, R659 (prelude).
- 18.2: R662 (record fields).
- 18.9: R664.
- 25: W854; the N range becomes N01 to N31.
