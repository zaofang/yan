# R567 three-seed repeat with a paired sign test (T18 D4, #698)

Main at `533dcb5`, release build. Harness `bench/r567/` with the settings of the #289 rerun (100 tasks per
seed, humaneval + mbpp, 10 impossible variants, 5-shot, free decoding, `claude -p`, both arms), on seeds
2, 3 and 4 (the #289 rerun used seed 1). Each seed draws a different 100 tasks, so pairs are within a
seed and pooled across seeds. Impossible variants are excluded from the counts below. Paired test:
`bench/r567/paired.py`, exact two-sided sign test on first-reply (pass@1) discordant pairs. Summaries:
`bench/r567/results/{sonnet,haiku}-5-5-t18-seed{2,3,4}-5shot.json`; full records
`/mnt/project-files/r567/t18/`.

## First reply, paired

| model | seed | both | Yan only | Python only | neither | p |
|---|---|---|---|---|---|---|
| Sonnet | 2 | 82 | 1 | 11 | 6 | 0.0063 |
| Sonnet | 3 | 86 | 0 | 10 | 4 | 0.0020 |
| Sonnet | 4 | 85 | 1 | 11 | 3 | 0.0063 |
| Sonnet | pooled | 253 | 2 | 32 | 13 | < 0.0001 |
| Haiku | 2 | 73 | 1 | 11 | 15 | 0.0063 |
| Haiku | 3 | 63 | 5 | 15 | 17 | 0.0414 |
| Haiku | 4 | 70 | 4 | 12 | 14 | 0.0768 |
| Haiku | pooled | 206 | 10 | 38 | 46 | 0.0001 |

## pass@1 per arm and Python spread

| model | seed | Yan Agent | Python | gap | Yan final | Python final |
|---|---|---|---|---|---|---|
| Sonnet | 2 | 83 | 93 | 10 | 92 | 93 |
| Sonnet | 3 | 86 | 96 | 10 | 93 | 96 |
| Sonnet | 4 | 86 | 96 | 10 | 93 | 96 |
| Haiku | 2 | 74 | 84 | 10 | 82 | 84 |
| Haiku | 3 | 68 | 78 | 10 | 74 | 79 |
| Haiku | 4 | 74 | 82 | 8 | 82 | 82 |

Python pass@1 spreads over 3 points across seeds for Sonnet (93 to 96) and 6 for Haiku (78 to 84). The
Yan Agent gap is 10 points on five of six runs (8 on one), well outside that spread. Seed 1 (#289 rerun)
gave 6 (Sonnet) and 8 (Haiku).

## Reading

The gap is not noise. Discordant pairs run 16 to 1 against Yan Agent for Sonnet and 3.8 to 1 for Haiku
pooled; every seed is significant at 0.05 except Haiku seed 4 (p = 0.077). After repair the gap closes to
0 to 3 points (Sonnet 92 to 93 vs 93 to 96; Haiku 74 to 82 vs 79 to 84), so the loss is mostly first-reply.
This run does not split the no-diagnostic losses from the others; that needs the failure-class pass of
T18 on these records.

Token ratios (Yan Agent over Python `out`, all solved tasks): Sonnet spec 0.57 to 0.61, body 0.65 to 0.79;
Haiku spec 0.85 to 0.88, body 0.95 to 1.43.
