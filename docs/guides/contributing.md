# Contributor guide

How to change Yan. The rules are in [CLAUDE.md](../../CLAUDE.md) and [development-laws.md](../development-laws.md)
(DL1 to DL16); this page is the order of commands, and links to the rule instead of restating it. Policy
(DCO, AI disclosure, what needs an RFC) is in [CONTRIBUTING.md](../../CONTRIBUTING.md). Start with
[getting started](getting-started.md) if you have not written a Yan program.

## 1. Set up

```sh
cargo build --release -p yan-cli      # target/release/yan, the default of tools.difftest and tools.fuzz
uv sync                               # Python tools; uv is required
```

`cargo build -p yan-cli` gives `target/debug/yan`; pass `--yan target/debug/yan` to the tools, or
`--executor NAME=target/debug/yan` to `conform.py`.

## 2. Find work and name the stage

Work is a GitHub issue. Every issue is in a group tracker (`TRACK-P<n>`); a known hole is a numbered item in a gap
issue ([known gaps](known-gaps.md)). One issue, one PR, closed on merge. A PR states its stage
(DL1): **working**, then **right**, then **fast**. You may not start fast while right is red.

| Stage | Evidence in the PR |
|---|---|
| Working | the program on the interpreter and on native (`--backend machine` or `cranelift`), both outputs |
| Right | conformance case proposed, `tools.difftest` clean, spec rows and code agree |
| Fast | callgrind instruction count against the committed baseline (`bench/`), never wall time |

## 3. Run a program on every backend

```sh
yan run FILE                      # interpreter: the oracle
yan run --backend machine FILE    # register VM
yan run --backend cranelift FILE  # native
yan run --ub FILE                 # abstract-byte machine with the 6.7 undefined-behavior checks
```

The interpreter defines correct output. A backend that disagrees, or says `not supported yet`, is a bug in
the backend. `--ub` reports `ub(N)`, N being the 6.7 item, and exits 101 on the first violation (a data race
between `par` arms, a double drop, a misaligned access). A program that passes everywhere else and trips `--ub`
has a real fault. `conformance/ub/` holds one case per item.

## 4. Differential run

```sh
uv run python -m tools.difftest                      # every conformance run case, bench program and listed example
uv run python -m tools.difftest --only bits_layout   # substring filter on program paths
uv run python -m tools.difftest --strict-refusals    # also count `not supported yet` as a mismatch (#758)
```

An unexplained mismatch fails the run. An accepted divergence goes in `tools/difftest_allow.toml` as an
`[[allow]]` entry with `program`, `backend` and `issue = "#N"`; an entry that no longer mismatches is stale and
fails the run, so remove it when you fix the bug.

## 5. Fuzz

```sh
uv run python -m tools.fuzz --seeds 200               # seeds 0..199, interpreter against every backend
uv run python -m tools.fuzz --start 1000 --seeds 50 --keep out/
uv run python -m tools.fuzz --show 17                 # print the program of seed 17
```

A seed always produces the same program. A failing program is reduced by line deletion and reported; file it as
an issue with the reduced program. There is no allowlist for fuzz findings.

## 6. Conformance cases

`conformance/` is the executable spec (spec 24). It is owned by the lead and **read-only to agents**: CI's
path guard rejects any agent PR that touches `tests/` or `conformance/`. Propose a case in its own PR on a
`conform/*` branch and link it from the code PR. A bug fix files its reproducing case in the same sitting (DL4).

A case is one file whose leading comment lines are headers; the layout and header table are in
[conformance/README.md](../../conformance/README.md). A run case:

```yag run
# spec: 10.1
# stdout:
# 42
def main() uses io:
    io.print("42")
```

```text out
42
```

Place it by kind (`yag/pass`, `yag/fail`, `yag/run`, `ub`, ...). Run one case, or a few, by giving the runner a
root with the same layout:

```sh
mkdir -p /tmp/one/yag/run && cp my_case.yag /tmp/one/yag/run/
uv run python tools/conform.py --root /tmp/one --executor interp=target/release/yan
uv run python tools/conform.py --executor interp=target/release/yan --executor machine="target/release/yan --backend machine"
```

The coverage report is not committed. CI generates it on every run (`conformance-coverage` artifact and job summary);
locally: `uv run python tools/conform.py --coverage /tmp/coverage.md`.

## 7. Spec rows and diagnostic codes

The spec is `spec/`, changed only for rows the lead accepted, in a PR the lead merges. Pipeline: study, RFC
(`rfcs/0000-template.md`, items labelled D1, D2), lead decision, rows. Numbers are taken last, in the writing PR:

```sh
uv run python tools/rows.py next 3          # three free row numbers
uv run python tools/rows.py next-code E3    # next free code in a family
uv run python tools/rows.py render          # regenerate Appendix A.1; never hand-edit it
uv run python tools/rows.py check           # duplicates, uncited or unregistered rows, code-map drift
uv run python tools/rows.py renumber --base origin/main --check   # before merging main
```

Every new row lands with its conformance case in the same PR set (CLAUDE.md, "Changing the spec"). Every new
E/W code lands with its crate symbol, catalogue entry and one case. Drafts and studies carry D-labels, never R
numbers or codes. If another PR took your numbers, merge main and renumber.

## 8. Before you push

```sh
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test
uv run pytest -q tools/selftest
uv run python tools/conform.py --executor interp=target/release/yan
uv run python -m tools.difftest
uv run python docs/guides/check.py --yan target/release/yan   # when you touched docs/guides
```

Commit with `git commit -s` (DCO) and an `Assisted-by:` trailer for AI-assisted work. The PR body follows
`.github/pull_request_template.md`. `main` requires `guard`, `canary`, `check`, `formal`, `rust` and `signoff`;
a red check is fixed, never merged over, and a code failure is never re-run (DL15).

## 9. Code rules that review will check

- One fact, one representation (DL6): ownership, moves, spans and newtype identity are computed once. Before
  patching such a bug, grep the other passes for the same pattern.
- Fail loud (DL7): an unsupported construct gets a numbered diagnostic or a real implementation. A Python builtin
  Yan Agent lacks becomes a real function, or an N-code alias, never a bare E200.
- No new code in `yan-elab/src/lower.rs`, `yan-hir/src/raise.rs` or `yan-mir/src/lower.rs` (DL13); put new
  lowering in a new module. `tools/line_ratchet.py` caps files at 3,000 lines.
- Frozen names change only by RFC (DL12); `tools/names_lint.py` checks retired names.
- Every figure in a report or README carries a commit and a date (DL14).

## Docs

Pages in `docs/guides/` embed code in blocks `check.py` verifies: `include=PATH` (equal to a file, which stays the
single source), `run` (a complete program), `text out` (its stdout on interpreter, machine, cranelift and
`--ub`), `fail=CODE` and `gap=PATH`. Never paste spec text into a guide; link the section.
