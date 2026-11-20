# 0099: The tail value of `with` and `unsafe` blocks, and diverging branches of a conditional

- Status: accepted
- Research: #1836 (gap issue); R567 hard run 2026-11-20 (`bench/r567/results/hard-2026-11-20`, branch `bench/r567-hard-2026-11-20b`, main 4556487); report `r567-hard-gap-2026-11-20`
- Rows: R1131 (D1); D2 pending, numbered with its implementation

## Summary

In Yan a block body's last expression is the function's result (spec 15, Functions). The rule reaches through `if`/`else`, `match` and `select` in tail position, and stops at `with` and `unsafe`: a value at the end of a `with` block is a discarded result, so a function that narrows a capability and then computes its answer must write `return` inside the block. Nothing in the surface predicts the difference. In the 2026-11-20 hard run, 9 of the 20 Sonnet first-reply misses in the yn arm are this one form (`with fs = fs.sub(dir)` followed by the answer); the yag arm cannot make the mistake because Yan Agent always writes `return`. This RFC decides whether `with` and `unsafe` pass their tail value on, and whether a diverging form may be a branch of the conditional expression.

State on main 00dd7d6 (2026-11-20), interpreter:

```
f(c: bool) -> int =            -- accepted: the if/else tail is the result
  if c
    1
  else
    2

read_note(name: str) -> str uses fs =
  with fs = fs.sub("notes")
    fs.read(name) else ""      -- rejected: no value reaches the end of the body

run(path: str) -> str uses fs =
  with fs = fs.sub("notes")
    plugin(path)               -- rejected: discarded result

safe_div(a: int, b: int) -> int fail Zero =
  if b == 0 then fail Zero() else a // b   -- rejected: `fail` is not an expression
```

## Decisions

### D1: which blocks pass their tail value on

Options:

- A. A `with` block and an `unsafe` block in tail position pass their last expression on as the result, exactly as `if`/`else`, `match` and `select` do today. Tail position stays a statement rule: the block must be the last statement of a function body, or of a block that is itself in tail position. `for`, `while` and `loop` never yield, because their body runs zero or many times; a value at the end of a loop body stays a discarded result. Outside tail position nothing changes. The canonical printer drops the `return` it would otherwise print on the last line of such a block, as it does for `if` and `match`, so `fmt` has one spelling. Yan Agent is unchanged: it keeps the explicit `return` inside `with`, and the two surfaces lower to the same core.
- B. Make `with` and `unsafe` expressions, usable anywhere a value is (`let t = with fs = fs.sub(d)` and a block). Rejected for now: Yan has no block expressions; `if` as a value is the one-line conditional, and block `if` is a statement whose tail is passed on. B would add a second way for one construct to be a value, needs a Yan Agent projection that has no Python shape, and no run shows the form being attempted.
- C. Keep today's rule; rely on the discarded-result diagnostic and the primer. Rejected: the rule is an exception the reader has to memorize, and the benchmark gate counts the first reply, which a diagnostic cannot help.

Recommended: A. It removes the exception rather than documenting it: every non-looping block in tail position behaves the same way. The implementation is two arms in the tail rewrite of the Yan reader and the matching arms in the printer; elaboration, MIR and both backends are untouched, because the reader already produces the `return` that Yan Agent writes by hand.

Consequences of A the lead should see before deciding:

- A unit function is unaffected: a non-unit last expression in a unit body stays a discarded result, inside `with` as elsewhere.
- Two existing conformance cases assert today's rejection of a `with` tail value (`yn/fail/failure/e312_with_tail_value`, and the bare-name case added in #1837). Under A both programs become valid. Those cases are the lead's to edit or retire; the loop half of the same diagnostic keeps its cases.
- `with deadline(d)`, `with timeout(d)` and `with shield(d)` are `with` blocks and follow the same rule.

- Decision: accepted as recommended: A (lead, 2026-11-20)
- Row: R1131

### D2: a diverging form as a branch of the conditional expression

The grammar already has `diverge` (`return`, `fail`, `break`, `continue`, `unreachable`) after `else` in a fallback: `x else fail E()`. The conditional `if c then a else b` takes only values, so `if b == 0 then fail Zero() else a // b` is a syntax error, while the block form of the same `if` is accepted.

Options:

- A. Either branch of `if c then a else b` may be a `diverge`. The expression has the type of the other branch; two diverging branches are rejected as they would be for a fallback. One rule: a diverging form is allowed wherever the grammar offers a value as an alternative to another value.
- B. Allow only `fail` there. Rejected: a second, narrower list of diverging forms beside the one `fallback` already uses.
- C. Leave it; the block form works. Acceptable, and the cost today is small: no Sonnet miss in the 2026-11-20 run, three Haiku misses in the 2026-11-19 run.

Recommended: A, on consistency with `fallback`. It is independent of D1 and can be deferred without affecting it.

- Decision: accepted as recommended: A (lead, 2026-11-20)
- Row: pending, numbered with the implementation

## Spec changes

D1: section 15.3, Functions (R1131). The two cases that asserted the rejection of a `with` tail value are now passing cases of R1131 (lead's word, 2026-11-20).

D2: section 15 grammar (`cond`) and the Failure paragraph, with its implementation.
