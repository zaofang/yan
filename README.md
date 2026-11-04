# Yan

Yan 言 is a programming language with one core and many surfaces. Every surface lowers to the same core, Yan Core (`.yir`),
which is fast, concurrent, deterministic and capability-safe. The compiler is written in Rust (R215), as one Cargo
workspace (spec 22.2).

| Surface | Ext | Status |
|---|---|---|
| Yan Agent | `.yag` | Built. Python-shaped, typed, written by LLMs; measured in total tokens per solved task. A lenient reader repairs common model mistakes, and R807 turns each missing Python builtin or method into a real std function or an N-code alias, never a bare E200. |
| Yan | `.yn` | Built. The human syntax, called plain Yan where it needs telling apart. |
| Yan Meta | `.ymt` | Research and canary surface, built after M1. |
| Yan Shell, Yan Binary Format, and the other named job surfaces | `.ynsh`, `.ybf`, ... | Named niche surfaces, none built. See [docs/surfaces.md](docs/surfaces.md). |

Status: milestone M1 is 410 of 410 conformance cases passing; the whole suite is 492 of 544.
Design laws, pipeline and measured state: [docs/architecture.md](docs/architecture.md).
Accepted decisions and studies: [docs/decisions-2026-11-04.md](docs/decisions-2026-11-04.md), [docs/studies/](docs/studies), [docs/research/README.md](docs/research/README.md).

The developer owns the spec, tests and gates; agents write the code and never edit tests.
Authoritative documents: the Yan 1.0 spec (draft) in [`spec/`](spec/README.md) and the 1.0 Development Roadmap.

## Layout

| Path | Owner | Notes |
|---|---|---|
| `spec/` | Antonio | Agents open spec PRs for accepted rows; Antonio merges them. Rows are registered in `spec/rows.toml`. |
| `conformance/`, `tests/` | Antonio | Agents propose changes in a separate PR and never edit these. CI rejects agent PRs that touch them. |
| `crates/` | agents | Cargo workspace, `yan-*` crates per spec 22.2 (`yan-yag`, `yan-yn`, `yan-core`, `yan-cli`, ...). Agent self-tests live in `#[cfg(test)] mod selftest` (SQ28). |
| `bench/` | agents | Benchmark set (R389): each program in Yan, typed Python and Rust, run by `bench/run.py`. |
| `tools/conform.py` | agents | Conformance runner (spec 24.1), independent of the implementation. Python, run with uv. |
| `tools/guard.py` | Antonio (CODEOWNERS) | Path guard used by CI. |
| `tools/rows.py` | agents | Change-row registry check (`spec/rows.toml`): duplicate rows, unregistered citations, stale Appendix A.1. `next` prints free row numbers. |
| `SPEC_QUESTIONS.md` | Antonio | Spec ambiguities; code cites them with `SPEC-Q`. |

Held-out evaluation tests live in a separate private repository that agents cannot read.

## Development

```sh
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test
cargo build -p yan-cli
uv sync && uv run pytest -q tools/selftest
uv run python tools/conform.py --executor interp=target/debug/yan
```

An executor is any command that implements `CMD check --format json FILE` (prints a JSON array of
spec 18.2 diagnostic records) and `CMD run FILE`. Exit status 2 from either is a tool failure.

## Licensing

| What | License |
|---|---|
| Code: compiler, toolchain, std, runtime support, conformance suite | [Apache-2.0 WITH LLVM-exception](LICENSE) |
| Spec text, docs, tour, book | [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/) |
| Grammars, normative tables, spec code samples | CC-BY-4.0, or Apache-2.0 WITH LLVM-exception |
| Doc examples, `yan new` templates, published eval task statements | [0BSD](https://opensource.org/license/0bsd) |

Programs compiled with Yan belong to their authors. Std and runtime code embedded in them falls under the
LLVM exception, so binaries need no Yan notice.

Copyright the Yan authors. "Yan" and the 言 logo are trademarks claimed by the project lead; the trademark policy will be
published before the repository goes public.

Contributions: see [CONTRIBUTING.md](CONTRIBUTING.md) (DCO sign-off, AI disclosure),
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) and [SECURITY.md](SECURITY.md).

## Required repository settings

CI runs on a self-hosted runner (no hosted Actions minutes): register one under Settings → Actions → Runners.


Branch protection on `main`: require a pull request, require review from Code Owners, and require
the `guard`, `canary`, `check`, `rust` and `conformance` status checks. The runner needs rustup
(the toolchain is pinned in `rust-toolchain.toml`) and uv.
Enable private vulnerability reporting under Settings → Code security.
