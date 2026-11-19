#!/usr/bin/env bash
# Printer parity of selfhost/s0/core_print.yn with crates/yan-core/src/printer.rs (#1505).
# Usage: selfhost/s0/core_print_check.sh   (needs target/release/yan, libyan_rt.a and a release build of the example)
set -euo pipefail
files=$(find conformance/yir selfhost/s0/lex_edge selfhost/s0/parse_edge -name '*.yir' | sort)
tmp=$(mktemp -d)
# `use .sibling` is E017 (R1067 is not implemented), so the program is one file: main, lexer, parser, printer.
cat selfhost/s0/core_print_main.yn selfhost/s0/core_lex.yn selfhost/s0/core_parse.yn selfhost/s0/core_print.yn > "$tmp/core_print.yn"
cargo build --release --locked -p yan-core --example print_dump
target/release/examples/print_dump $files > "$tmp/rust.out"
target/release/yan run "$tmp/core_print.yn" -- $files > "$tmp/interp.out"
target/release/yan build "$tmp/core_print.yn" -o "$tmp/core_print"
YAN_REPORT_LIVE=1 "$tmp/core_print" $files > "$tmp/native.out" 2> "$tmp/native.err"
grep -q '^live: 0' "$tmp/native.err"
cmp "$tmp/rust.out" "$tmp/interp.out"
cmp "$tmp/rust.out" "$tmp/native.out"
echo "printer parity: $(wc -l < "$tmp/rust.out") lines, interpreter and native identical to Rust"
