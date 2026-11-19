#!/usr/bin/env bash
# Driver parity of selfhost/s0/core_main.yn (#1505): walks conformance/yir and the edge corpora with fs.list_dir, prints
# each file, checks print-parse-print is stable, and compares the output with the Rust printer on the same file list.
# Usage: selfhost/s0/core_main_check.sh   (needs target/release/yan, libyan_rt.a and a release build of the example)
set -euo pipefail
tmp=$(mktemp -d)
dirs="conformance/yir selfhost/s0/lex_edge selfhost/s0/parse_edge"
cat selfhost/s0/core_main.yn selfhost/s0/core_lex.yn selfhost/s0/core_parse.yn selfhost/s0/core_print.yn > "$tmp/core_main.yn"
cargo build --release --locked -p yan-core --example print_dump
target/release/yan run "$tmp/core_main.yn" -- $dirs > "$tmp/interp.out" 2> "$tmp/interp.err"
target/release/yan build "$tmp/core_main.yn" -o "$tmp/core_main"
YAN_REPORT_LIVE=1 "$tmp/core_main" $dirs > "$tmp/native.out" 2> "$tmp/native.err"
grep -q '^live: 0' "$tmp/native.err"
! grep -q Mismatch "$tmp/interp.err"
! grep -q Mismatch "$tmp/native.err"
# The walk order is the order of fs.list_dir; the Rust side is given the same list.
files=$(sed -n 's/^file //p' "$tmp/interp.out")
target/release/examples/print_dump $files > "$tmp/rust.out"
cmp "$tmp/rust.out" "$tmp/interp.out"
cmp "$tmp/rust.out" "$tmp/native.out"
echo "driver: $(echo "$files" | wc -l) files, interpreter and native identical to Rust, print-parse-print stable"
