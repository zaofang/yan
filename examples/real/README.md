# Real programs sweep (2026-11-16)

Programs a user would write, run on interpreter, machine and cranelift (`yan run --backend ...`).
Figures: main at the build used for this sweep, 2026-11-16.

| program | interp | machine | cranelift |
|---|---|---|---|
| actor_counter | ok | ok | ok |
| bst | ok | I001 (#1398) | I001 (#1398) |
| csv_report | ok | ok | ok |
| json_walk | ok | StrJoin (#1399) | ok |
| lru_cache | ok | ok | ok |
| matrix | ok | ok | ok |
| par_fetch | ok | ok | ok |
| pipeline | ok | ok | ok |
| primes_par | ok | ok | ok |
| stack_calc | ok | list.pop (#1399) | list.pop (#1399) |
| tokenizer | ok | ok | ok |
| wordfreq | ok | ok | ok |

`gaps/` holds minimal repros, each failing today; the first comment line gives the issue.
Issues: #1398 to #1404 (#1405 fixed).
