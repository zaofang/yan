# R567 lowering candidates: Python expectations the spec forbids or leaves open

Source: `docs/reports/2026-11-04-r567-sonnet-haiku.md`. Each item below changes language semantics if
accepted, so none is implemented. Row ids are the spec sections that govern today's behaviour; this list
proposes no new rows. Failed-round counts are Sonnet / Haiku before repair; E017 is 10 / 19 in total across all its forms and the report does not split it.

Already fixed as code-only gaps where the spec allows the construct: `range` with a negative literal step and a
name bound again after its block ended (#223), `x in iterator` (#224).

## Decide

| # | Gap | Counts | Spec today | Recommendation |
|---|---|---|---|---|
| 1 | E201 `char` vs `str`: `c >= "0"`, `c in "abc"`, `res += c`, `char` passed as `str`, `float * int` | 45 / 53 | 10.1, 14.1: no implicit conversion; `"a"` is a `str` literal | Accept a one-character string literal as `char` when the other side is `char` (literal-only, like integer literals in 3.4). Keep variables strict. Add `+= c` only through `str.push(c)`. Leave `float * int` an error (R656 says a variable never converts). Largest single win. |
| 1b | `"a" in xs` with `xs: list[char]` or `iter[char]` compares `str` with `char` and returns false with no diagnostic (found while fixing #224) | n/a | `in` does not check the left operand against the element type | Treat as a bug: emit E201 as `==` does. Do it together with item 1. |
| 2 | E241 `xs = []` without annotation | 7 / 10 | SQ7, 10.1: inference would need search | Infer from the first `append` in the same block, or from the return type when the name is returned. Narrow; keeps "no search" because it looks at one statement. Otherwise keep E241 and add the fix `xs: list[T] = []` to its diagnostic. |
| 3 | E210 positional arguments to two same-typed parameters | 9 / 9 | 14.1, 15.3: keywords required, except literal or same-named argument | Keep. It is a deliberate law. Put the rule in `primer.md` (one line) rather than change the checker. |
| 4 | W806 branch that special-cases one input | 5 / 7 | 21.1: error in the spec gate | Keep. Models write `if n == 0: return 1`. Check the heuristic against base cases (`n == 0`, empty list) that no `ex` names; if it fires on those, that is a checker bug to file. |
| 5 | `ord`, `chr`, `divmod`, `pow` | E017 | 17 prelude list has none; the elaborator reports them as unsupported | Add `ord(c) -> int` and `chr(n) -> char fail` (or `char.code()` / `char.from_code(n)`). Models reach for them in cipher and digit tasks. `divmod` and `pow(a, b)` are one-liners over `//`, `%`, `**`. Needs a spec row (17). |
| 6 | Iterating a tuple (`for x in (a, b)`, `for x in p`) | E017 (counts below) | silent; 3.x types a tuple as a product, not a sequence | Accept only for a tuple whose fields share one type (iterates as `list[T]` in order). Heterogeneous stays an error. |
| 7 | Indexing `iter[char]` as a place (`cs = s.chars(); cs[1]`) | E017 | 17.3: `iter[T]` is single pass, no indexing | Keep forbidden. Replace the E017 with E201-style "an `iter` is not indexable; use `.collect()`" so the repair is one step. Diagnostic change only. |
| 8 | Missing names: prelude `map`, `re` / `regex`, `str.count`, `str.matches`, `char.isdigit`, `char.code`, `iter[char].len` | 5 / 2 (E200), 3 / 10 (E203) | `map` is an `iter` adapter (R510), `list.map` is eager (R512); regex is `std.regex` | Add `str.count(part)`, `char.is_digit()` if the spec's char methods lack it. `list.map` exists, so the fix for `map` is in the primer. `re`: point E200's `maybe` at `std.regex`. `iter.len` stays absent (single pass); the diagnostic should say `.count()`. |
| 9 | Untyped lambdas bound to a name used later (`g = lambda x: x + 1`) | E017 | 15: lambda parameters are inferred from the expected type | Works today for local binding and `sorted(key=...)`. The remaining E017 cases are lambdas passed to a non-existent `map` (item 8). Nothing to do beyond item 8. |

## Not a gap

- `str(x)` on `int` and `char` runs and passes today. The report lists it under E017 without a task id and the
  per-task records are not in the repo, so the failing case is not reproduced; it was probably `str` of another type.
- Rebinding a name in sibling or later scopes is not E230; E230 is for the same scope only (14.1).

## Order

Items 1, 1b and 5 remove most remaining checker rounds. Items 2 and 6 are small. Items 3, 4 and 7 are
documentation or diagnostic wording.
