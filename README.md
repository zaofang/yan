# Yong

Yong is a programming language with one core and two surfaces.

- **Jian**: a typed, Python-shaped syntax optimized for LLM authors, measured in total tokens per solved task.
- **Wen**: the human syntax.

Both compile to the same Yong core, which is fast, concurrent, deterministic and capability-safe.
The implementation starts as a Python reference and is ported to Rust.

The developer owns the spec, tests and gates; agents write the code and never edit tests.
Authoritative documents: Spec v0.2 and the 1.0 Development Roadmap.

## Layout

| Path | Owner | Notes |
|---|---|---|
| `spec/`, `conformance/`, `tests/` | Antonio | Agents propose changes in a separate PR and never edit these. CI rejects agent PRs that touch them. |
| `packages/` | agents | Added as crates are built. uv workspace, one package per Rust crate in spec section 18, each a subpackage of the `yong` namespace. |
| `tools/conform.py` | agents | Conformance runner (spec 20.1), independent of the implementation. |
| `tools/guard.py` | Antonio (CODEOWNERS) | Path guard used by CI. |
| `SPEC_QUESTIONS.md` | Antonio | Spec ambiguities; code cites them with `SPEC-Q`. |

Held-out evaluation tests live in a separate private repository that agents cannot read.

## Development

```sh
uv sync
uv run pytest -q tools/selftest
uv run python tools/conform.py                                 # no executors yet
uv run python tools/conform.py --executor py-interp="uv run yong"  # once yong-cli exists
```

An executor is any command that implements `CMD check --format json FILE` (prints a JSON array of
spec 16.1 diagnostic records) and `CMD run FILE`.

## Licensing

| What | License |
|---|---|
| Code: compiler, toolchain, std, runtime support, Python reference, conformance suite | [Apache-2.0 WITH LLVM-exception](LICENSE) |
| Spec text, docs, tour, book | [CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/) |
| Grammars, normative tables, spec code samples | CC-BY-4.0, or Apache-2.0 WITH LLVM-exception |
| Doc examples, `yong new` templates, published eval task statements | [0BSD](https://opensource.org/license/0bsd) |

Programs compiled with Yong belong to their authors. Std and runtime code embedded in them falls under the
LLVM exception, so binaries need no Yong notice.

Copyright the Yong authors. "Yong" and the 永 logo are trademarks claimed by the project lead; the trademark policy will be
published before the repository goes public.

Contributions: see [CONTRIBUTING.md](CONTRIBUTING.md) (DCO sign-off, AI disclosure),
[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) and [SECURITY.md](SECURITY.md).

## Required repository settings

CI runs on a self-hosted runner (no hosted Actions minutes): register one under Settings → Actions → Runners.


Branch protection on `main`: require a pull request, require review from Code Owners, and require
the `guard`, `canary`, `check` and `conformance` status checks.
Enable private vulnerability reporting under Settings → Code security.
