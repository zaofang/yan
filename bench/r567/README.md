# R567 harness: Yan Agent against typed Python

Measures the M1 gate of spec 23 (R567) with the harness of spec 21.6. Each task runs in two arms with
the same task text, the same two phases and the same hidden cases:

| | Yan Agent arm | Python arm |
|---|---|---|
| spec phase | `def` lines, `pre`/`post`, `ex` lines, bodies `...` | annotated `def` lines, docstrings, pytest cases, bodies `...` |
| body phase | bodies, then repair rounds from `yan agent` (spec check, patch, autofix, tests) | bodies, then repair rounds from mypy --strict and pytest |
| driver | `yan agent` (R620) with `model.py` as its `cmd:` adapter | `run.py` |
| hidden cases | appended to the file, run by `yan test` | appended to the file, run by pytest |

Repairs: 2 in the spec phase, 3 in the body phase. Hidden cases never reach the model.

## Tasks

`tasks.py` rebuilds MultiPL-E's translation from the original benchmark files on GitHub (the MultiPL-E
datasets are on Hugging Face, which the cloud environment cannot reach). It keeps a task when every
assert is `f(literals) == literal`, then infers one signature from the cases, with at least three cases.
At `32db6f7` (2026-11-08) that is 98 of 164 HumanEval and 852 of 974 MBPP problems. The signature is part of the prompt, in
Python type syntax, so both arms answer the same question. MBPP names are rewritten to snake_case (E110),
and calls use keywords when two parameters share a type (E210); both arms see the same names.

Cached under `~/.cache/yan-r567`. `uv run python bench/r567/tasks.py` prints the counts.

## Run

```sh
cargo build --release -p yan-cli
uv run python bench/r567/run.py --yan target/release/yan --model claude:claude-sonnet-5-5 \
    --suite humaneval,mbpp --limit 100 --shots 5 --impossible 0.1
uv run python bench/r567/report.py bench/r567/runs/claude-free-5shot --json summary.json
```

- `--model claude:M` uses the `claude -p` CLI (Anthropic credits, no key). `anthropic:M` uses
  `ANTHROPIC_API_KEY`. `compat:URL#M` talks to a llama.cpp or vLLM server. `oracle:DIR[?wrong]` replays
  stored solutions for dry runs (`bench/r567/oracle/`, ten tasks).
- Reruns skip finished files. One JSON record per task and arm under `runs/MODEL-DECODING-Nshot/`.
- `--limit` takes a seeded shuffle (`--seed`). Pass the same limit and seed for every model.

## Measures

- **pass@1**: the hidden cases pass on the first body reply, before any repair round. `hidden_first`.
- **pass after repair**: the hidden cases pass on the last reply. `hidden_final`. The record's `status`
  is separate: it says whether the model's own tests went green.
- **Tokens**: `tokens.spec` and `tokens.body` as the provider reports them (`in`, `out`, `reasoning`),
  plus `out_bytes` of the reply text. The report divides by solved tasks. The R567 gate compares `out`
  per solved task, spec and body separately. Repair rounds are inside the body figure.
- **Repair rounds**: body replies after the first, per arm.
- **Cheating rate**: `--impossible F` adds an impossible variant for a share F of the tasks. Its first
  case is replaced by a wrong result, shown to the model as a required example, so only special-casing
  passes the hidden cases. A pass counts as cheating. A `STOP:` reply from the Yan Agent arm is not.
- **Lowering candidates**: each Yan Agent round that failed on checker diagnostics is listed with its
  codes (`lowering_candidates` in the record; `checker_codes_before_repair` in the report). Whether a
  code reflects a Python expectation is a human call: read the report text of each, then write a
  lowering proposal or file a 26.6 extension (surface-semantics study, recommendation 1).
- `report.py` prints the gate booleans and a like-for-like token ratio on jointly solved tasks.

### Tokenizer caveat

Counts are the provider's own (`tokenizer: provider` in every record). The `claude -p` adapter sums
cached and fresh input tokens. The proxy blocks current tokenizers, so no run is recounted with o200k,
cl100k or a current Anthropic tokenizer. `out_bytes` is the tokenizer-free check. Compare ratios between
arms of one run, never counts between runs on different models.

## Constrained decoding

`grammar.py` exports spec 13.2 as `out/yag.gbnf` (llama.cpp, vLLM) and `out/yag.lark` (llguidance,
XGrammar). Both start with the code fence, so a constrained reply is one fenced block. The masks are
supersets: block layout and keyword exclusion are not enforced (header of `grammar.py`). The Lark file
is accepted by llguidance's validator, and a Lark Earley parse accepts 176 of the first 200 files under
`conformance/`, among them all of `conformance/yag/pass` except `golden/g5_tally.yag`.

```sh
uv run python bench/r567/grammar.py
curl -O https://raw.githubusercontent.com/ggml-org/llama.cpp/master/grammars/python.gbnf
uv run python bench/r567/run.py ... --model compat:http://localhost:8080#MODEL --decoding grammar \
    --grammar-yag bench/r567/out/yag.gbnf --grammar-py python.gbnf
```

Hosted Anthropic models take no grammar, so `--decoding grammar` needs an open model behind a server.
The Python arm takes `--grammar-py FILE` (llama.cpp's `python.gbnf`); `run.py` fences its root.
**Not run yet**: no such server is available in the cloud environment, and the fenced grammar has only
been generated, not served.

## Inputs to review before the gate runs

- `primer.md` is the earlier Xin primer renamed, 451 o200k tokens. Check it against main.
- `shots/` holds five pairs (`.yag` and `.py`) that pass `yan test` and mypy. They are not benchmark
  tasks. Two come from the syntax study samples.
- Temperature is the CLI's default for `claude:`; the spec asks for temperature 0 and the deployment
  temperature. `compat:` sends 0.
