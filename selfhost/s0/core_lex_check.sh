#!/usr/bin/env bash
# Token parity of selfhost/s0/core_lex.yn with crates/yan-core/src/lexer.rs (#1505).
# Usage: selfhost/s0/core_lex_check.sh   (needs target/release/yan and a release build of the example)
set -euo pipefail
files=$(find conformance/yir selfhost/s0/lex_edge -name '*.yir' | sort)
tmp=$(mktemp -d)
cargo build --release --locked -p yan-core --example lex_dump
target/release/examples/lex_dump $files > "$tmp/rust.out"
target/release/yan run selfhost/s0/core_lex.yn -- $files > "$tmp/interp.out"
target/release/yan build selfhost/s0/core_lex.yn -o "$tmp/core_lex"
YAN_REPORT_LIVE=1 "$tmp/core_lex" $files > "$tmp/native.out" 2> "$tmp/native.err"
grep -q '^live: 0' "$tmp/native.err"
cmp "$tmp/rust.out" "$tmp/interp.out"
cmp "$tmp/rust.out" "$tmp/native.out"
echo "lexer parity: $(wc -l < "$tmp/rust.out") lines, interpreter and native identical to Rust"
