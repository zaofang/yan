# R567 benchmark: Sonnet 5.5 and Haiku 5.5, free decoding

Harness: `bench/r567/` at `fb1d2cf` (#207). Release build of main at that commit. Raw summaries:
`bench/r567/results/`. Per-task records stay in `bench/r567/runs/` (gitignored).

## Setup

- Models: `claude-sonnet-5-5`, `claude-haiku-5-5` through `claude -p` (Anthropic credits), CLI default temperature.
- Tasks: 100 from the translated HumanEval and MBPP pools (seeded shuffle, seed default, same for both
  models), plus 10 impossible variants. 5-shot, free decoding, both arms, same task text and hidden cases.
- Repairs: 2 in the spec phase, 3 in the body phase.
- Inputs checked first: all five `shots/` pairs pass `yan test` and `mypy --strict` on this build.
  `primer.md` already says Yan Agent; no primer line was changed. Not every primer claim was checked
  against the spec one by one: the shots and the runs exercise most of them.
- Tokens are the provider's counts (`tokenizer: provider`); `out` per solved task, as the gate defines it.
  Reasoning tokens are reported separately by the provider and are not added here.

## Results

| | Sonnet Python | Sonnet Yan Agent | Haiku Python | Haiku Yan Agent |
|---|---|---|---|---|
| pass@1 | 91 | 63 | 77 | 33 |
| pass after repair | 91 | 85 | 77 | 56 |
| own tests green | 100 | 90 | 100 | 80 |
| repair rounds (total) | 5 | 41 | 6 | 60 |
| spec `out` per solved | 783 | 522 | 1532 | 1744 |
| body `out` per solved | 810 | 847 | 1041 | 2256 |
| `out_bytes` per solved, spec / body | 1485 / 1682 | 620 / 1108 | 1390 / 1719 | 932 / 2090 |
| impossible runs, cheated | 10, 2 | 10, 1 | 10, 2 | 10, 1 |

Like-for-like on jointly solved tasks (Yan Agent over Python, n = 81 Sonnet, 55 Haiku):

| | spec `out` | body `out` | spec bytes | body bytes |
|---|---|---|---|---|
| Sonnet | 0.62 | 0.93 | 0.38 | 0.59 |
| Haiku | 0.84 | 1.73 | 0.46 | 0.96 |

R567 gate, as `report.py` computes it: pass@1 not below Python fails on both models. Body tokens lower
fails on both. Spec tokens lower holds for Sonnet only (per solved and jointly).

Cheating: a pass on an impossible variant counts as cheating. One read by hand (Sonnet, Python, mbpp/3)
special-cases the wrong example and says so in a comment; the rest were not read. The Yan Agent arm stopped with `STOP:` on 4 (Sonnet) and 3
(Haiku) impossible variants, which the harness does not count as cheating.

## Reading

- Yan Agent is behind on first-try correctness and the gap grows on the smaller model (28 points on
  Sonnet, 44 on Haiku). Repair closes most of it for Sonnet (85 vs 91) and not for Haiku (56 vs 77).
- Most first-try losses are checker diagnostics, not logic: 41 repair rounds (Sonnet) and 60 (Haiku)
  against 5 and 6 for Python.
- Spec-phase tokens favour Yan Agent on Sonnet because contracts replace pytest cases. Body-phase tokens
  do not, mainly because repair rounds sit inside the body figure.
- Bytes are well under Python everywhere (0.38 to 0.96); the token result is therefore partly a
  tokenizer and repair-count effect, not verbosity of the surface.

## Lowering candidates (Yan Agent rounds that failed on checker diagnostics)

Counts are failed rounds before repair across non-impossible tasks. P003 (patch rejected, wrapper of the
rest) is omitted. Candidate means a Python expectation the checker refuses; whether to lower or to
document is a human call.

| Code | Sonnet | Haiku | What the model wrote |
|---|---|---|---|
| E201 | 45 (20 tasks) | 53 (26) | `c >= "0"` and `c in "abc"` on `char` vs `str`; `res += c`; passing `char` where `str` is wanted; `float * int` |
| E017 | 10 (9) | 19 (16) | rebinding a name after its block ended; `range(a, b, -1)`; `ord`, `str(c)`; lambdas without a known type; indexing `iter[char]` as a place; iterating a tuple |
| E210 | 9 (5) | 9 (5) | positional arguments to functions with two same-typed parameters |
| E241 | 7 (7) | 10 (7) | `xs = []` without an annotation |
| E203 | 3 (3) | 10 (7) | `s.count`, `s.matches`, `c.isdigit`, `c.code`, `chars.len` |
| E200 | 5 (3) | 2 (2) | `map`, `re` / `regex` (no such name) |
| W806 | 5 (5) | 7 (5) | branch special-casing one input (reads as a hard-code to the checker) |
| E301 | 3 (1) | 4 (1) | `io.print` without `uses io` |
| E104 / N22 / N21 | 0 | 3 / 3 / 1 | chained comparisons |
| E100, E403, E413, E619, E202, E401 | 2 | 10 | parse slips, mutability, copy, example order, arity, moved value |

Strongest candidates: E201 (char/str comparison is the single largest source), E017 `range` with a
negative step, E241 empty-list annotation (inference from the return type), E210 (keyword rule).

## Elaborator and std gaps the runs expose

1. `std.prelude.all` call unsupported at execution (7 final failures on each model); `any`, `sum`,
   `min`/`max` with these arguments, and `std.text.contains` with these arguments also (Haiku: 8 more).
2. `range` with a step that is not a positive literal (E017).
3. Rebinding a name after its block ended (E017).
4. Indexing `iter[char]` as a place; iterating a fixed tuple; `str(x)` as a call of a value; `ord`;
   lambdas without a known callable type (E017).
5. No `map`, `re`; `str` has no `count`, `matches`; `char` has no `isdigit`/`code`/`to_str`; `iter[char]`
   has no `len` (E200/E203). Some of these may be intended absences; they are listed because models reach
   for them.

## Caveats

- One seed, 100 tasks, one temperature (CLI default, not 0). Differences of a few tasks are noise.
- Constrained decoding arm: not run. It needs an open model behind a llama.cpp or vLLM server, which the
  cloud environment does not have.
- Two model families are not covered: both are Anthropic. R566-style second-family runs still need an OK.
- 5 (Sonnet) and 24 (Haiku) Yan Agent tasks end with the model's own tests green and the hidden cases
  failing, mostly MBPP specification ambiguity; Python shows the same pattern (9 and 23 failing).
- Hidden-case failures from unsupported calls (item 1 above) are toolchain gaps, not model errors, and
  depress Yan Agent pass-after-repair by 7 (Sonnet) and about 15 (Haiku) points.
