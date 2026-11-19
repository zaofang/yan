Comparison with `hard-2026-11-18b` (commit ddbce3a). This run: commit 1a8a0b9, 2026-11-19, 36 tasks x 3 seeds x 3 arms x 2 models, OpenRouter, temperature 0.3, USD 9.24. Figures from `summary.md`.

First-reply pass@1 (108 runs per cell), 11-18b then 11-19:

| model | arm | 11-18b | 11-19 | p vs py, 11-18b | p vs py, 11-19 |
|---|---|---:|---:|---:|---:|
| Sonnet | yag | 83.3% | 85.2% | 0.185 | 0.308 |
| Sonnet | yn | 72.2% | 82.4% | 0.0008 | 0.122 |
| Sonnet | py | 90.7% | 90.7% | | |
| Haiku | yag | 80.6% | 79.6% | 0.0003 | <0.0001 |
| Haiku | yn | 58.3% | 67.6% | <0.0001 | <0.0001 |
| Haiku | py | 97.2% | 98.1% | | |

After repair: Sonnet yag 91.7 to 92.6, yn 98.1 to 96.3, py 91.7 to 92.6; Haiku yag 90.7 to 89.8, yn 88.9 to 87.0, py 99.1 to 99.1.

M1 gate (first-reply pass@1 at or above typed Python): not met on any arm. Sonnet yag is 5.5 points below Python (not significant, p 0.31), Sonnet yn 8.3 points below (p 0.12), Haiku yag and yn are significantly below. Sonnet yn gained 10 points; Haiku yn gained 9.

Remaining first-reply misses by cause (yag/yn, both models), with issues:
- yag spec-phase E301 on `ex` lines of capability-using functions (19): #1746 (new)
- sized ints `uint8` conversions and `u8` methods, rotl8 and bit_parity (about 24): #1747 (new)
- `try_send` taken as bool, failing call in a comprehension, E201/E315 (10): #1748 (new)
- yn `par for` E017 (4): #1749 (new)
- fs.sub attenuation naming and `with` colon in escape tasks: #1726, #1724 (open)
- `List.push` in yn, `Chan[int](cap)`, `?` on a non-call: #1725, #1727 (open; `push`, `?` on bound Result and Chan near-miss names landed in #1728, #1731, #1734)
- escape yn run-mode behavior misses, `fs.read_text` E017 in `rt_dotdot_forms` yn, E312 on `rt_plugin_confine`: not yet grouped; no new issue (fewer than 6 runs each)
