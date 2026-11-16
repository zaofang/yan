# OpenRouter sweep, follow-up: hard Yan tasks, primer experiments, Python arm

Stage claimed: working. Follows [2026-11-14-openrouter-yag-sweep.md](2026-11-14-openrouter-yag-sweep.md).
Data: `bench/r567/results/openrouter-followup-2026-11-14/` (`attempts.jsonl`, `followup-summary.json`, `yag-summary.json`,
`ledger.jsonl.gz`). Scripts: `bench/r567/hard.py` (task set and validator), `followup_report.py`, `sweep_report.py`.
Toolchain: the binary built from main at 67d5a2a (2026-11-14), the same as the first sweep. Spend this round: USD 20.90 of the
USD 30 granted (provider-reported cost), 4,065 attempts. One sample per task and model unless stated; temperature 0.3.

## What was run

1. **Hard Yan suite** (`--suite hard`): 18 hand-written tasks in 8 categories (concurrency, ownership, failure, contracts, sums, numbers,
   bits, capabilities). Every task has a reference solution that passes its hidden `yan test` cases (`hard.py --validate`), so none is unsolvable.
   All 40 models, primer v2 (below), plus a second sample for 13 models.
2. **Primer experiments**, paired against the first sweep's primer v1 on the same tasks: v2 (7 added bullets) on 14 models, v3 (one added bullet) on 12 models
   (original 100-task set) and on 13 models (hard suite).
3. **Impossible variants** (10 tasks, the first case's result replaced by a wrong one, shown as a required example): 14 models, 138 runs.
4. **Python arm** (typed Python with mypy --strict and pytest, same two phases, same hidden cases): 7 models on the 40-task core.

## Results

### 1. The hard Yan suite (primer v2, 18 tasks, one sample)

| Model | Pass of 18 | 95% CI | pass@1 | Cost USD |
|---|---|---|---|---|
| anthropic/claude-haiku-5.5 | 18 | 0.82 to 1.00 | 0.72 | 0.036 |
| anthropic/claude-opus-5.5 | 18 | 0.82 to 1.00 | 0.83 | 0.908 |
| anthropic/claude-sonnet-5.5 | 18 | 0.82 to 1.00 | 0.83 | 0.524 |
| qwen/qwen3.7-plus | 18 | 0.82 to 1.00 | 0.72 | 0.181 |
| google/gemini-4-hydrogen | 17 | 0.74 to 0.99 | 0.83 | 0.660 |
| moonshotai/kimi-k3.1-pro | 17 | 0.74 to 0.99 | 0.78 | 0.883 |
| google/gemini-3.8-flash | 16 | 0.67 to 0.97 | 0.78 | 1.177 |
| openai/gpt-5.6-terra | 16 | 0.67 to 0.97 | 0.61 | 0.335 |
| openai/gpt-5.6-luna | 15 | 0.61 to 0.94 | 0.67 | 0.042 |
| deepseek/deepseek-v4-flash | 14 | 0.55 to 0.91 | 0.67 | 0.027 |
| deepseek/deepseek-v4.1-flash | 14 | 0.55 to 0.91 | 0.72 | 0.173 |
| minimax/minimax-m3 | 14 | 0.55 to 0.91 | 0.44 | 0.210 |
| deepseek/deepseek-v4-pro | 12 | 0.44 to 0.84 | 0.61 | 0.090 |
| google/gemma-4-31b-it | 12 | 0.44 to 0.84 | 0.61 | 0.016 |
| moonshotai/kimi-k2.7-code | 12 | 0.44 to 0.84 | 0.61 | 0.602 |
| openai/gpt-4.1 | 12 | 0.44 to 0.84 | 0.56 | 0.178 |
| openai/gpt-5.1-codex-mini | 12 | 0.44 to 0.84 | 0.50 | 0.274 |
| google/gemini-3.5-flash-lite | 11 | 0.39 to 0.80 | 0.50 | 0.040 |
| ibm-granite/granite-4.2-8b | 11 | 0.39 to 0.80 | 0.39 | 0.044 |
| minimax/minimax-m3.1-flash | 11 | 0.39 to 0.80 | 0.50 | 0.209 |
| mistralai/codestral-2508 | 11 | 0.39 to 0.80 | 0.50 | 0.013 |
| nvidia/nemotron-3-super-120b-a12b | 11 | 0.39 to 0.80 | 0.50 | 0.033 |
| openai/gpt-5.4-mini | 11 | 0.39 to 0.80 | 0.44 | 0.061 |
| qwen/qwen3.7-flash | 11 | 0.39 to 0.80 | 0.44 | 0.052 |
| mistralai/devstral-2512 | 10 | 0.34 to 0.75 | 0.44 | 0.036 |
| openai/gpt-4.1-mini | 10 | 0.34 to 0.75 | 0.50 | 0.033 |
| mistralai/mistral-large-2512 | 9 | 0.29 to 0.71 | 0.39 | 0.033 |
| qwen/qwen3-coder-30b-a3b-instruct | 9 | 0.29 to 0.71 | 0.50 | 0.016 |
| meta-llama/llama-3.3-70b-instruct | 8 | 0.25 to 0.66 | 0.44 | 0.077 |
| google/gemma-3-27b-it | 7 | 0.22 to 0.64 | 0.41 | 0.009 |
| meta-llama/llama-4-maverick | 7 | 0.22 to 0.64 | 0.29 | 0.027 |
| openai/gpt-5.4-nano | 7 | 0.20 to 0.61 | 0.22 | 0.035 |
| qwen/qwen3-coder | 7 | 0.20 to 0.61 | 0.33 | 0.023 |
| stepfun/step-3.7-flash | 7 | 0.20 to 0.61 | 0.39 | 0.585 |
| microsoft/phi-4 | 6 | 0.16 to 0.56 | 0.22 | 0.009 |
| mistralai/ministral-8b-2512 | 5 | 0.12 to 0.51 | 0.28 | 0.009 |
| mistralai/mistral-small-3.2-24b-instruct | 5 | 0.12 to 0.51 | 0.17 | 0.008 |
| amazon/nova-lite-v1 | 2 | 0.03 to 0.33 | 0.11 | 0.009 |
| openai/gpt-4.1-nano | 1 | 0.01 to 0.26 | 0.06 | 0.007 |
| meta-llama/llama-3.1-8b-instruct | 0 | 0.00 to 0.18 | 0.00 | 0.002 |

Pass rate by category (all models and samples, primer v2): contracts 0.95 (50/52), sums 0.79, failure 0.74, bits 0.66, concurrency 0.63, numbers 0.55,
**ownership 0.40, capabilities 0.35**. By task, the five lowest: `append_copy` 0.17, `swap_ends` 0.24, `inout_bump` 0.26, `checked_add` 0.32, `channel_capacity` 0.33.

The top of the ranking compresses (Haiku, Opus, Sonnet and Qwen3.7 Plus all 18/18), unlike on the MBPP-derived set, so this suite separates weak from strong
models better than it ranks the strong ones. The bottom is the same: Llama 3.1 8B 0/18, GPT-4.1 nano 1/18, Nova Lite 2/18.

Cause of the low ownership and capability scores: the spec phase asks for `ex` lines, and models write them for functions that return nothing or only mutate a `var`
parameter or print. The checker answers `E201 ... this is always None; annotate the binding` or `E403 ... changes its receiver, but the receiver is a temporary` and the
model does not recover (about 40 failing runs per task end with the checker error, not a wrong answer). Other repeat causes: `checked_add` (`return a + b` in a function
returning `int | None` reports E201 on the operand, a checker bug, #1266; the reference solution routes through a local), `channel_capacity` (`else e -> x = ...` is an unsupported assignment target, E017, and `try_send` needs `uses nondet`).

### 2. Primer experiments

- **v2 hurts.** Seven added bullets (len, chained comparisons, comprehensions, str methods, bit_length, chr, annotations) on 14 models, 1,262 paired tasks:
  719 passes with v1, 684 with v2 (-35, -2.8 points); 11 of 14 models worse, 2 better. pass@1 568 to 554. Longer primer, worse result.
- **v3 helps where it targets.** One bullet: a function that returns nothing or only mutates takes no `ex` lines. On the hard suite (13 models, same tasks, v2 vs v3):
  the four none-returning tasks (`append_copy`, `swap_ends`, `inout_bump`, `shout_caps`) go 20/56 to 38/55; ownership 27/56 to 39/55; capabilities 7/14 to 12/14;
  all 18 tasks 174/252 to 183/250. A repeat of the v2 sample on the same models scored 158/246 against 174/252, so sample noise is about 5 points on a 250-attempt set;
  the none-returning gain (+33 points) is outside it, the overall gain is not.
- **v3 on the original 100-task set** (12 models, 1,152 paired tasks): 629 passes with v1, 614 with v3 (-1.3 points), pass@1 492 to 494. No regression beyond noise.
- Adopted: `bench/r567/primer.md` is now v3; v1 is kept as `primer-v1.md` and v2 as `primer-v2.md` for reproduction. Earlier results are against v1.

### 3. Cheating on impossible variants

5 of 138 runs (3.6 percent) passed an impossible variant by special-casing the wrong example: GPT-5.6 Luna 2/10, Sonnet 5.5 1/10, MiniMax M3.1 Flash 1/10, Gemini 3.5 Flash Lite 1/10;
Haiku 5.5, Gemini 3.8 Flash, DeepSeek V4 Flash, Qwen3.7 Plus, Llama 3.3 70B, Gemma 4 31B, Nemotron, Mistral Small, Phi-4 and GPT-5.4 mini 0.
A `STOP:` reply is not counted as cheating. Small samples: treat as a floor.

### 4. Yan Agent against typed Python (core 40, primer v1 for Yan Agent, 7 models)

| Model | Core 40 Python pass | Core 40 Yan Agent pass | Jointly solved | Out tokens Python | Out tokens Yan Agent | Yan/Python |
|---|---|---|---|---|---|---|
| anthropic/claude-haiku-5.5 | 23 | 21 | 21 | 34213 | 45253 | 1.32 |
| anthropic/claude-sonnet-5.5 | 38 | 36 | 35 | 63480 | 45632 | 0.72 |
| deepseek/deepseek-v4-flash | 19 | 15 | 15 | 37888 | 45829 | 1.21 |
| google/gemini-3.5-flash-lite | 16 | 17 | 15 | 5271 | 5462 | 1.04 |
| google/gemma-4-31b-it | 17 | 16 | 15 | 11116 | 8887 | 0.80 |
| minimax/minimax-m3.1-flash | 20 | 18 | 15 | 45495 | 98668 | 2.17 |
| openai/gpt-5.6-luna | 18 | 17 | 16 | 14568 | 15596 | 1.07 |

Pass rates are within a few tasks of each other (Python ahead for 6 of 7, by 1 to 4 tasks). Output tokens per jointly solved task, pooled over the seven models: Yan Agent / Python = 1.25
(per model 0.72 to 2.17). The R567 token gate (Yan Agent at or below Python) is not met on this sample; Sonnet 5.5 (0.72) and Gemma 4 31B (0.80) are under 1.0.
Both figures include repair rounds. Python pass@1 equals its final pass in every run here; not investigated.

## Conclusions

- The hard suite finds what the algorithm set cannot: ownership (`var`, `&`, `take`) and `ex` handling of None-returning functions drive the failures, not concurrency or contracts.
- A single targeted primer line beats seven general ones. Primer changes need a paired before/after and a noise baseline; do not add guidance by intuition.
- Most of the none-returning failure is a diagnostics problem: E201 "this is always None" on an `ex` of a None function should say that such a function takes no `ex` lines.
- Cheating is rare (3.6 percent) but non-zero, even for the strongest models.
- Yan Agent costs about a quarter more output tokens than Python on jointly solved tasks; the gate is open.

## Action items

- B1 (done in this PR). Primer v3 adopted; hard suite added; harness options `--primer`, `--tag`, `--impossible-only`.
- B2. Diagnostics: `ex` on a function that returns None or only mutates: say so in the E201/E403 text and suggest dropping the `ex`. (Added to #1223.)
- B3. Checker bug #1266 (`return a + b` in a `T | None` function); `else e -> x = ...` assignment target (E017) in `else` handlers; `str.find` absent although specified.
- B4. Report ownership and capability as headline categories in the next progress report; extend the hard suite with actors, comptime, bits layouts and `uses` handlers (needs a conform PR for conformance-side tasks: #1222).
- B5. Rerun the Python arm with 3 samples and more models before judging the R567 gate; this sample is 7 models on 40 tasks.
- B6. Rerun the 40-model hard suite after #1266 and #1221 land to measure their effect.
