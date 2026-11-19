# Recipes wrap the commands .github/workflows/ci.yml runs; change both together.
# Full conformance, difftest and sweeps are CI-only (long runs): `just conform` runs the gate on a release build.

set shell := ["bash", "-euo", "pipefail", "-c"]

yan := justfile_directory() / "target/release/yan"

default:
    @just --list

# Everything CI runs except the long conformance sweeps.
ci: check lint-rust test deny audit lint-workflows

# Python tooling checks (ci job `check`).
check:
    uv sync --locked
    uv run ruff check .
    uv run ruff format --check .
    uv run mypy tools
    uv run pytest -q tools/selftest
    uv run python tools/rows.py check
    uv run python -m tools.lint_reports
    uv run python tools/conform.py --coverage conformance/COVERAGE.md
    uv run python -m tools.names_lint
    uv run python -m tools.line_ratchet

lint: lint-rust lint-workflows

lint-rust:
    cargo fmt --all --check
    cargo clippy --locked --all-targets -- -D warnings

# nextest builds only test targets; yan-cli's build tests need libyan_rt.a. nextest does not run doctests.
test:
    cargo build --locked -p yan-rt
    cargo nextest run --locked --no-fail-fast
    cargo test --locked --doc

deny:
    cargo deny --locked check

audit:
    cargo audit

lint-workflows:
    actionlint
    uvx zizmor@1.32.0 --offline --no-progress --config .github/zizmor.yml .github/workflows

# The conformance gate (ci job `rust`); the rest of its sweeps run on CI only.
conform:
    uv sync --locked
    uv run python tools/third_party.py --check
    cargo build --locked --release -p yan-cli
    uv run python tools/conform.py --json conformance-summary.json --expected-failures tools/conform_xfail.toml --executor interp={{ yan }}
