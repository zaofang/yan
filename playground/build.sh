#!/bin/sh
# Builds playground/yong.wasm. Needs the wasm32-unknown-unknown target, a clang with the
# WebAssembly backend and wasi-libc headers (core-math is C).
set -eu
cd "$(dirname "$0")/.."
WASI_INCLUDE=${WASI_INCLUDE:-/usr/include/wasm32-wasi}
# wasm has only round-to-nearest and no exception flags; wasi-libc's fenv.h leaves these out.
export CFLAGS_wasm32_unknown_unknown="-isystem $WASI_INCLUDE -D__wasi__ \
  -DFE_DOWNWARD=0x400 -DFE_UPWARD=0x800 -DFE_TOWARDZERO=0xc00 \
  -DFE_INVALID=1 -DFE_DIVBYZERO=4 -DFE_OVERFLOW=8 -DFE_UNDERFLOW=16 -DFE_INEXACT=32"
# The C fenv and stdio calls left undefined become imports the page stubs (worker.js).
export RUSTFLAGS="-C link-arg=-zstack-size=33554432 -C link-arg=--allow-undefined"
cargo build --release -p yong-playground --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/yong_playground.wasm playground/yong.wasm
