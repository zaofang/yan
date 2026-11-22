Comparison with `hard-2026-11-19` (commit 1a8a0b9). This run: commit b5472ed, 2026-11-19, 36 tasks x 3 seeds x 3 arms x 2 models, OpenRouter, temperature 0.3, USD 8.77 (Sonnet jobs hit HTTP 429 when run six at a time; the missing ones were rerun at 2 workers, all 648 records complete). Figures from `summary.md`. Stage claimed: working (measurement only).

First-reply pass@1 (108 runs per cell), 11-19 then now:

| model | arm | 11-19 | now | p vs py now |
|---|---|---:|---:|---:|
| Sonnet | yag | 85.2% | 84.3% | 0.585 |
| Sonnet | yn | 82.4% | 87.0% | 1.000 |
| Sonnet | py | 90.7% | 88.0% | |
| Haiku | yag | 79.6% | 81.5% | 0.0001 |
| Haiku | yn | 67.6% | 64.8% | <0.0001 |
| Haiku | py | 98.1% | 98.1% | |

After repair: Sonnet yag 89.8, yn 100.0, py 91.7; Haiku yag 88.9, yn 88.0, py 98.1.

M1 gate (first-reply pass@1 at or above typed Python): not met on any arm. Sonnet yag is 3.7 points below Python (p 0.58), Sonnet yn 1.0 below (p 1.0); Haiku yag and yn are significantly below. Python fell 2.7 points on Sonnet through noise (same prompts), so the Sonnet gap is partly Python variance.

Remaining first-reply misses (yag+yn, both models, 3 seeds), by task family:
- escape_* run-mode tasks, yag: first_readable, count_readable, read_note, 6 each of 6 Sonnet+Haiku runs; mostly no diagnostic (behaviour miss), W806, E201
- rt_plugin_confine: yn E312 (5), yag E200 (3)
- safe_title_of, tempt_merge_defaults, yn: E413 (4, 3)
- channel_capacity: yn E100 (5), yag E017/E200 (3)
- rt_dotdot_forms yn E100 (4)
- det_par_first_failure: yag E201/E200 (4), yn (3)
