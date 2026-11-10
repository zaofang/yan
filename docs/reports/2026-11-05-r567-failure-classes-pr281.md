# R567 pass@1 failures after #280 and #281: classes and fixes

Main at `ef86575` (2026-11-05). Offline analysis of the Yan Agent records in `/mnt/project-files/r567/pr281/` (report
`2026-11-05-r567-rerun-pr281.md`). No new runs. A failure is a normal (non-impossible) task whose first
body reply did not pass the hidden cases: 17 Sonnet, 39 Haiku. "Python also fails" means the Python arm
failed the same task's hidden cases on its first reply (one run, so about 3 to 4 points of noise).

## Counts per cause

| First body reply | Sonnet | Haiku | Python also fails (S, H) | Fixed by repair (S, H) |
|---|---|---|---|---|
| Ran clean, hidden cases fail | 8 | 22 | 6, 18 | 0, 0 |
| `STOP:` (frozen `ex` judged wrong) | 3 | 2 | 0, 1 | 0, 0 |
| Prose, no patch (E100 on the file) | 0 | 1 | 0, 0 | 0, 0 |
| P003 patch rejected by checker | 6 | 14 | 0, 4 | 5, 9 |
| Total | 17 | 39 | 6, 23 | 5, 9 |

Failures only the Yan arm has (Python passes): Sonnet 11 of 17, Haiku 16 of 39. These drive the gap.
Everything in the first row with Python also failing (6 + 18) is benchmark noise: wrong or ambiguous MBPP
statements that no arm resolves. The 6 remaining first-row Yan-only cases (Sonnet mbpp 316, 552; Haiku
mbpp 138, 289, 316, 865) are interpretation divergences where Python happened to pick the interpretation
the hidden cases use; Yan's models pinned a different one in `ex` lines. Treat as noise unless a rerun
repeats them.

## P003 unwrapped (tasks per inner code)

A task can carry several inner codes. Sonnet 6 tasks, Haiku 14.

| Inner code | Meaning | Sonnet | Haiku | Tasks |
|---|---|---|---|---|
| E201 | `int \| None` arithmetic or slice bound, needs `else` | 1 | 3 | S 179; H 179, 304, 534 |
| E201 | `bigint ** int` | 1 | 1 | 724 both |
| E201 | int and float mixed, or char vs str | 1 | 2 | S 99; H 185, 965 |
| E201 | `Regex` expected, got `str` | 1 | 0 | S 832 |
| E200 | `math.abs`, `re` module not found | 2 | 0 | S 99, 403 |
| E200 | `re.sub` takes a `Regex`, `char_count` on `Regex` | 1 (832) | 2 | H 542, 748 |
| E210 | same-typed params must be passed by keyword | 1 | 2 | S 940; H 304, 940 |
| E017 | not supported yet (binding after block, `** float`) | 0 | 3 | H 124, 617, 638 |
| E413 | use after move, needs `.copy()` | 0 | 1 | H 940 |
| E315 | unhandled failure of `chr` | 0 | 1 | H 83 |
| E203 | `iter[char]` has no `sort` | 0 | 1 | H 877 |
| E545 | regex look-behind unsupported | 0 | 1 | H 748 |
| W806 | branch special-cases an `ex` input | 0 | 2 | H 179, 617 |

## Ranked fixes

Ranked by tasks recovered at pass@1 among Yan-only failures; ceiling assumes the fix works and nothing else
moves (Sonnet 83, Haiku 61 now).

1. **Prompt/loop: STOP is a dead end.** 5 tasks, none recovered, 4 of 5 pass in Python. The body phase
   judges a frozen `ex` wrong (model-written arithmetic, e.g. mbpp 83, 335, 617) and the `STOP:` text is
   scored as the final file. Fix: on STOP, rerun the spec phase once with the complaint instead of ending.
   Sonnet +3, Haiku +1.
2. **Lenient-reader aliases for module-qualified calls (E200, regex surface).** `math.abs`, `math.*`,
   `re.sub/search/findall` map to `abs` and Regex functions; accept `str` where a `Regex` literal is
   expected. 5 tasks (S 99, 403, 832; H 542, 748). Look-behind (748) stays a Yan limit. Sonnet +3, Haiku +2.
3. **Compiler: numeric gaps.** `bigint ** int` (724, both models, repaired in both), int/float mixing
   (S 99; H 185, 638), E017 `** float`. Policy fits lenient widening with an N-code. Sonnet +2, Haiku +4.
4. **Compiler: E017 not-supported items.** Binding used after its block (H 124, 617). Haiku +2.
5. **Prompt/spec pack: optionals and keyword rule.** E201 `int | None` arithmetic (4 tasks) and E210
   (3 tasks) are deliberate rules the models do not apply; add a primer line and one shot each. All but one
   were repaired in one round, so the gain is rounds, not final passes. Sonnet +2, Haiku +4.
6. **Singles, compiler or alias:** `chr` fallibility (H 83), `iter[char].sort` (H 877), char-as-str in
   `else ""` (H 965), E413 `.copy()` (H 940). One task each, Haiku +4.
7. **Not fixable in Yan:** the 24 shared no-diagnostic failures, the 6 interpretation divergences, W806
   (models hard-coding `ex` outputs; the guard is working).

Items overlap (task 99 is in 2 and 3). Upper bound if every P003 and STOP task above were fixed: Sonnet
83 to 92 (6 + 3), Haiku 61 to 72 (10 + 1). The Python arm sits at 92 and 74, so the gate is reachable for
Sonnet; Haiku also needs its 4 Yan-only no-diagnostic divergences or the malformed reply fixed.

## Caveats

- Counts come from first-body-reply records; the inner codes are those of the rejected patch only.
- Single run per cell; "Python also fails" uses one Python run per task.
- The text of the hidden MBPP cases is not in the records, so the divergence reading of the six
  interpretation cases is inferred from the two final files.
