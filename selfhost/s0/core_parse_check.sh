#!/usr/bin/env bash
# AST parity of selfhost/s0/core_parse.yn with crates/yan-core/src/parser.rs (#1505).
# Usage: selfhost/s0/core_parse_check.sh   (needs target/release/yan, libyan_rt.a and a release build of the example)
set -euo pipefail
files=$(find conformance/yir selfhost/s0/lex_edge selfhost/s0/parse_edge -name '*.yir' | sort)
tmp=$(mktemp -d)
# `use .sibling` is E017 (R1067 is not implemented), so the program is one file: main, lexer, parser.
cat selfhost/s0/core_parse_main.yn selfhost/s0/core_lex.yn selfhost/s0/core_parse.yn > "$tmp/core_parse.yn"
cargo build --release --locked -p yan-core --example parse_dump
target/release/examples/parse_dump $files > "$tmp/rust.out"
target/release/yan run "$tmp/core_parse.yn" -- $files > "$tmp/interp.out"
target/release/yan build "$tmp/core_parse.yn" -o "$tmp/core_parse"
YAN_REPORT_LIVE=1 "$tmp/core_parse" $files > "$tmp/native.out" 2> "$tmp/native.err"
grep -q '^live: 0' "$tmp/native.err"
cmp "$tmp/rust.out" "$tmp/interp.out"
cmp "$tmp/rust.out" "$tmp/native.out"
echo "parser parity: $(wc -l < "$tmp/rust.out") lines, interpreter and native identical to Rust"
