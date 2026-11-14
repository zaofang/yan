# OpenRouter sweep: models writing Yan Agent

Stage claimed: working (data and report only; no spec rows, no code outside `bench/r567/`).
Generated at commit 67d5a2a on 2026-11-14. Data: `bench/r567/results/openrouter-2026-11-14/`
(`attempts.jsonl`, `summary.json`, `ledger.jsonl.gz`, `task-ids.json`). Harness: `bench/r567/run.py` with the new
`openrouter:` vendor in `bench/r567/model.py`; stats from `bench/r567/sweep_report.py`.
Replaces the on-hold OpenAI R567 run for the model-family question. No keys are stored in the data.

## Setup

- 40 models through OpenRouter's OpenAI-compatible endpoint, resolved from the live model list on 2026-11-14:
  Claude (Opus 5.5, Sonnet 5.5, Haiku 5.5), Google (Gemini 4 Hydrogen, 3.8 Flash, 3.5 Flash Lite, Gemma 3 and 4),
  OpenAI (GPT-4.1 family, GPT-5.4 mini and nano, GPT-5.6 Luna and Terra, GPT-5.1 Codex Mini), Moonshot (Kimi K3.1 Pro,
  K2.7 Code), DeepSeek (V4 Pro, V4 Flash, V4.1 Flash), Qwen (3.7 Plus, 3.7 Flash, 3 Coder, 3 Coder 30B), MiniMax (M3, M3.1 Flash),
  Mistral (Large, Small 3.2, Ministral 8B, Codestral, Devstral), Meta (Llama 3.1 8B, 3.3 70B, 4 Maverick), NVIDIA Nemotron 3 Super,
  IBM Granite 4.2 8B, StepFun Step 3.7 Flash, Microsoft Phi-4, Amazon Nova Lite.
- Task set: the R567 MultiPL-E-style HumanEval and MBPP tasks (950 translatable). The sweep uses 100: the 50 hardest by earlier
  Sonnet 5.5 runs (failed or needed repair in `bench/r567/results/*pertask.json`) and 50 seeded random others. `core` is a 40-task
  subset (30 hard, 10 other) that every model has in common; frontier models ran only the core set to stay inside the budget.
- Protocol: the full R567 Yan Agent arm only (`yan agent`, two phases, 5 shots, 2 spec and 3 body repairs, primer and shots as committed),
  temperature 0.3, one sample per task and model, hidden cases run by `yan test`. Pass means the hidden cases pass after repairs;
  pass@1 means they pass on the first body reply. The Python arm and impossible (cheating) variants were not run: not enough budget.
- Spend: USD 18.93 of the USD 20 cap (ledger, provider-reported cost). 3,601 attempts. A few tasks per model are missing after
  429 and connection resets that outlasted retries; `Tasks` column shows the count.
- Limitation: the existing task set is algorithmic string, list and number programs. It has no concurrency, ownership, comptime,
  bit-layout or capability tasks, so the "hard" slice is hard for models to express in Yan, not hard in language-feature terms.
  See action item A1.

## Results

Models sorted by pass rate. Models marked 40 tasks ran the core set only, so compare them with the core table below, not with 100-task rows.

| Model | Tasks | Pass | 95% CI | pass@1 | Hard | First-reply checker errors | Cost USD | USD per pass | Median call s |
|---|---|---|---|---|---|---|---|---|---|
| anthropic/claude-opus-5.5 | 40 | 0.97 | 0.87 to 1.00 | 0.85 | 29/30 | 0.05 | 1.575 | 0.0404 | 4.41 |
| anthropic/claude-sonnet-5.5 | 40 | 0.90 | 0.77 to 0.96 | 0.88 | 26/30 | 0.00 | 0.945 | 0.0263 | 4.32 |
| google/gemini-3.8-flash | 99 | 0.87 | 0.79 to 0.92 | 0.71 | 41/50 | 0.02 | 1.610 | 0.0187 | 7.16 |
| google/gemini-4-hydrogen | 40 | 0.80 | 0.65 to 0.90 | 0.70 | 23/30 | 0.03 | 1.394 | 0.0436 | 7.36 |
| anthropic/claude-haiku-5.5 | 100 | 0.71 | 0.61 to 0.79 | 0.60 | 31/50 | 0.05 | 0.160 | 0.0023 | 4.0 |
| minimax/minimax-m3.1-flash | 98 | 0.64 | 0.54 to 0.73 | 0.49 | 29/50 | 0.17 | 0.465 | 0.0074 | 7.47 |
| google/gemini-3.5-flash-lite | 100 | 0.63 | 0.53 to 0.72 | 0.49 | 26/50 | 0.03 | 0.262 | 0.0042 | 0.97 |
| qwen/qwen3.7-plus | 98 | 0.62 | 0.52 to 0.71 | 0.51 | 26/49 | 0.05 | 0.714 | 0.0117 | 27.91 |
| openai/gpt-5.6-luna | 100 | 0.61 | 0.51 to 0.70 | 0.50 | 25/50 | 0.11 | 0.155 | 0.0025 | 5.56 |
| nvidia/nemotron-3-super-120b-a12b | 98 | 0.60 | 0.50 to 0.69 | 0.47 | 26/49 | 0.28 | 0.148 | 0.0025 | 5.25 |
| moonshotai/kimi-k3.1-pro | 40 | 0.60 | 0.45 to 0.74 | 0.55 | 15/30 | 0.12 | 1.514 | 0.0631 | 6.98 |
| moonshotai/kimi-k2.7-code | 95 | 0.59 | 0.49 to 0.68 | 0.43 | 22/45 | 0.22 | 2.107 | 0.0376 | 22.88 |
| deepseek/deepseek-v4-flash | 99 | 0.58 | 0.48 to 0.67 | 0.38 | 23/49 | 0.17 | 0.133 | 0.0023 | 4.96 |
| deepseek/deepseek-v4.1-flash | 98 | 0.57 | 0.47 to 0.67 | 0.48 | 23/48 | 0.21 | 0.352 | 0.0063 | 3.85 |
| ibm-granite/granite-4.2-8b | 98 | 0.57 | 0.47 to 0.67 | 0.51 | 24/48 | 0.41 | 0.212 | 0.0038 | 35.0 |
| openai/gpt-5.4-mini | 98 | 0.57 | 0.47 to 0.67 | 0.42 | 23/50 | 0.02 | 0.333 | 0.0059 | 1.25 |
| minimax/minimax-m3 | 100 | 0.57 | 0.47 to 0.66 | 0.46 | 21/50 | 0.20 | 0.471 | 0.0083 | 4.83 |
| google/gemma-4-31b-it | 100 | 0.55 | 0.45 to 0.64 | 0.42 | 24/50 | 0.08 | 0.077 | 0.0014 | 1.33 |
| deepseek/deepseek-v4-pro | 100 | 0.53 | 0.43 to 0.62 | 0.39 | 21/50 | 0.09 | 0.380 | 0.0072 | 12.24 |
| qwen/qwen3.7-flash | 97 | 0.51 | 0.41 to 0.60 | 0.37 | 19/47 | 0.10 | 0.183 | 0.0037 | 31.93 |
| mistralai/devstral-2512 | 100 | 0.50 | 0.40 to 0.60 | 0.37 | 20/50 | 0.03 | 0.170 | 0.0034 | 1.13 |
| qwen/qwen3-coder | 100 | 0.49 | 0.39 to 0.59 | 0.33 | 20/50 | 0.12 | 0.142 | 0.0029 | 1.26 |
| openai/gpt-5.1-codex-mini | 98 | 0.48 | 0.38 to 0.58 | 0.35 | 19/50 | 0.27 | 0.855 | 0.0182 | 4.83 |
| openai/gpt-4.1-mini | 100 | 0.45 | 0.36 to 0.55 | 0.34 | 17/50 | 0.10 | 0.227 | 0.0051 | 1.53 |
| qwen/qwen3-coder-30b-a3b-instruct | 100 | 0.45 | 0.36 to 0.55 | 0.34 | 19/50 | 0.15 | 0.114 | 0.0025 | 0.98 |
| openai/gpt-5.6-terra | 40 | 0.42 | 0.28 to 0.58 | 0.38 | 11/30 | 0.10 | 0.333 | 0.0196 | 2.83 |
| google/gemma-3-27b-it | 97 | 0.41 | 0.32 to 0.51 | 0.36 | 14/48 | 0.23 | 0.048 | 0.0012 | 3.89 |
| stepfun/step-3.7-flash | 95 | 0.41 | 0.32 to 0.51 | 0.37 | 14/45 | 0.52 | 2.067 | 0.0530 | 17.1 |
| openai/gpt-4.1 | 39 | 0.39 | 0.25 to 0.54 | 0.39 | 10/29 | 0.15 | 0.531 | 0.0354 | 1.53 |
| meta-llama/llama-3.3-70b-instruct | 100 | 0.38 | 0.29 to 0.48 | 0.28 | 16/50 | 0.48 | 0.233 | 0.0061 | 1.81 |
| mistralai/mistral-large-2512 | 100 | 0.38 | 0.29 to 0.48 | 0.31 | 15/50 | 0.39 | 0.240 | 0.0063 | 2.53 |
| mistralai/mistral-small-3.2-24b-instruct | 100 | 0.38 | 0.29 to 0.48 | 0.30 | 16/50 | 0.26 | 0.038 | 0.0010 | 1.3 |
| openai/gpt-5.4-nano | 99 | 0.35 | 0.27 to 0.45 | 0.23 | 13/49 | 0.24 | 0.217 | 0.0062 | 1.5 |
| mistralai/codestral-2508 | 100 | 0.35 | 0.26 to 0.45 | 0.32 | 14/50 | 0.12 | 0.085 | 0.0024 | 1.06 |
| microsoft/phi-4 | 100 | 0.32 | 0.24 to 0.42 | 0.30 | 11/50 | 0.68 | 0.047 | 0.0015 | 2.24 |
| mistralai/ministral-8b-2512 | 99 | 0.31 | 0.23 to 0.41 | 0.23 | 11/50 | 0.29 | 0.105 | 0.0034 | 2.0 |
| meta-llama/llama-4-maverick | 99 | 0.29 | 0.21 to 0.39 | 0.24 | 12/49 | 0.64 | 0.175 | 0.0060 | 2.21 |
| amazon/nova-lite-v1 | 100 | 0.25 | 0.17 to 0.34 | 0.24 | 9/50 | 0.96 | 0.054 | 0.0022 | 1.08 |
| openai/gpt-4.1-nano | 100 | 0.21 | 0.14 to 0.30 | 0.18 | 9/50 | 0.69 | 0.037 | 0.0018 | 1.52 |
| meta-llama/llama-3.1-8b-instruct | 97 | 0.11 | 0.07 to 0.19 | 0.11 | 3/48 | 0.90 | 0.018 | 0.0016 | 2.98 |

### Common core (40 tasks, every model)

Rank by pass on the same 40 tasks (counts of 40; 95% Wilson CI):

| Model | Pass | CI |
|---|---|---|
| anthropic/claude-opus-5.5 | 39 | 0.87 to 1.00 |
| anthropic/claude-sonnet-5.5 | 36 | 0.77 to 0.96 |
| google/gemini-4-hydrogen | 32 | 0.65 to 0.90 |
| google/gemini-3.8-flash | 31 | 0.62 to 0.88 |
| moonshotai/kimi-k3.1-pro | 24 | 0.45 to 0.74 |
| anthropic/claude-haiku-5.5 | 21 | 0.37 to 0.67 |
| minimax/minimax-m3.1-flash | 18 (of 39) | 0.32 to 0.61 |
| 20 models | 15 to 17 | about 0.25 to 0.58 |
| 12 lowest, from Llama 3.3 70B down | 4 to 13 | 0.04 to 0.48 |

Worst: Llama 3.1 8B (4 of 39), GPT-5.4 nano (5), GPT-4.1 nano (6), Phi-4 (7), Nova Lite and Ministral 8B (8).

## Findings

1. A clear top tier, then a flat middle. Opus 5.5 (0.97 on 40) and Sonnet 5.5 (0.90) lead; Gemini 4 Hydrogen (0.80) and Gemini 3.8 Flash
   (0.87 on 99, 0.78 on core) follow. Below them every model sits between 0.2 and 0.6 and most confidence intervals overlap. Do not rank within the middle.
2. Size and price do not predict Yan skill. GPT-4.1 (0.39), GPT-5.6 Terra (0.42), Mistral Large (0.38) and DeepSeek V4 Pro (0.53) trail Haiku 5.5 (0.71) and
   Gemini 3.5 Flash Lite (0.63), which cost a tenth as much. Code-specialized models (Qwen3 Coder, Codestral, Devstral, Kimi K2.7 Code, Codex Mini) do not beat
   general ones: 0.35 to 0.59.
3. The top models barely need the checker. Sonnet 5.5, Gemini 3.8 Flash and GPT-5.4 mini show first-reply checker errors in 0 to 2 percent of attempts, against
   0.90 for Llama 3.1 8B and 0.96 for Nova Lite. For weak models the repair loop is the main source of passes; for strong ones the primer and shots already carry the language.
4. Repairs help but cap out. Across all models pass@1 trails pass by 8 to 20 points for the middle tier (for example Gemini 3.8 Flash 0.71 to 0.87,
   Haiku 0.60 to 0.71). 340 of 3,601 attempts ended in `spec_failed`, 528 in `body_failed`.
5. Hard slice: 0.42 pass overall against 0.57 on the rest. By return type, `float` is worst (0.29, 154 attempts); `dict` (0.61) and tuple (0.56) are not harder than `int` (0.49).
   Only three tasks were never solved by any model: mbpp/769, mbpp/798, mbpp/899.
6. Cost: Haiku 5.5 is the cheapest way to a pass with real quality (USD 0.0023 per pass, 0.71). Gemma 4 31B (0.0014 per pass, 0.55) and Mistral Small 3.2 (0.0010, 0.38) are
   cheapest overall. Reasoning-heavy models cost most per pass: Kimi K3.1 Pro 0.063, Step 3.7 Flash 0.053, Gemini 4 Hydrogen 0.044, Opus 5.5 0.040, Kimi K2.7 Code 0.038.
   Latency: Gemini and Claude mid-tier 4 to 7 s per call; Qwen3.7 Plus, Qwen3.7 Flash, Granite 4.2 and Kimi K2.7 Code 23 to 35 s.

### Failure codes (what the checker told the model)

Attempts whose first reply hit the code (any model): E100 266, E201 175, E017 116, E203 97, E200 65, W823 58, E301 45, E110 37, W812 35.
Among attempts that ended failing, the codes seen at any round: E201 341, E100 259, E017 249, E203 247, E200 127, W806 81.

- E100 (syntax): typed Python habits the grammar rejects. Top messages: `unexpected end of line; expected an expression` (123), `unexpected X; expected end of line` (102),
  indentation (87). Concrete causes seen: `fail NoMatch` in `ex` lines, chained comparison (E104 `a < b < c`), multi-`for` comprehensions (E107), `try` blocks.
- E201 (type mismatch): `2 * x` where bool expected, `char` vs `str`, int vs float, empty collections.
- E017 (unsupported construct): calls on values (`list(...)`, `sorted` variants), features the toolchain lacks.
- E203/E200 (unknown method or name): the most-missed names are Python builtins and methods: `len` (161 hits as method or call), `get` (57),
  `parse_int` (54, an invented name), `bit_length` (24), `ord` (22), `sqrt` (21), `find` (19), `capitalize` (17), `char_count` (16), `byte_at` (15), `slice`, `to_string`, `to_int`.
  `regex`, `collections`, `std`, `math`, `io` imports (E200).
- W823 (annotated local the checker infers), W812 (unused parameter), W806, E403 (mutating a temporary receiver in `ex`), E315 (fallible `chr` call needs `?`), E504 (`str` has no indexing).
- Python-idiom leakage: 0.29 lenient-reader notes (N codes) per first reply on average; the rates by model range from 0 (Opus, Gemini 4 Hydrogen, GPT-5.4 nano)
  to 1.0 (Nova Lite) and 0.73 (Phi-4). N notes mean the toolchain accepted a Python form; they are the direct measure of which lowerings pay off.

## Conclusions

- Yan Agent is writable by frontier models from a primer and 5 shots, with near-zero checker friction (Opus, Sonnet). Below the frontier it depends on the repair loop.
- The failures are concentrated in a short list of missing Python builtins and methods and a few syntax forms. Those are cheap to fix on the language side and
  would lift the middle tier more than model choice would.
- The pass-rate gap between a USD 0.002-per-pass model and a USD 0.04-per-pass one is small relative to the gap between families. Choose model by
  checker friction and cost per pass, not by price tier.
- Results are one sample at temperature 0.3, and a single task family. Treat gaps under about 10 points as noise.

## Action items

- A1 (#1222, P3). Add Yan-specific hard tasks (concurrency with `par` and pools, ownership and moves, comptime, bits packing, `uses` capabilities, error handling with `fail`/`?`) as a
  proposed `conform/*` task set, then rerun. The current set cannot test the language's differentiators.
- A2 (#1221, P2). Python builtins and methods most requested and missing: `len(x)`, `x.get(k)`, `ord`/`chr`, `x.bit_length()`, `math.sqrt`, `str.find`, `str.capitalize`, `int(str)`. Per
  the Python-fidelity policy each becomes a real function if deterministic and capability-safe, else an N-code alias; none a bare E200.
- A3 (#1223, P5). Primer and docs: state explicitly that `ex` failure lines use the documented form, that chained comparisons and multi-`for` comprehensions are rejected, that `str` has no indexing
  (`s.chars()`), and that `chr`/`parse` are fallible. These are the top repeat E100, E104, E107, E504, E315 sources.
- A4 (#1223). Make E100 messages carry the fix when the cause is a known Python form (for `fail X` in `ex`, chained comparison, `try`).
- A5. Rerun with 3 samples on the top 8 models and add the Python arm and impossible variants once budget is raised, for the token-gate and cheating numbers R567 needs.
- A6. Harness: `run.py` output tag ignores the model name, so runs for different models need separate `--out` directories; the new vendor retries 429/5xx.
