#!/bin/sh
# Builds playground/yan.wasm for wasm32-wasip1; worker.js provides the WASI calls in the browser.
# Needs the wasm32-wasip1 target and a clang with the WebAssembly backend; core-math is C and
# compiles against the wasi-libc headers.
set -eu
cd "$(dirname "$0")/.."
WASI_INCLUDE=${WASI_INCLUDE:-/usr/include/wasm32-wasi}
# wasm has only round-to-nearest and no exception flags; wasi-libc's fenv.h leaves these out.
export CFLAGS_wasm32_wasip1="-isystem $WASI_INCLUDE \
  -DFE_DOWNWARD=0x400 -DFE_UPWARD=0x800 -DFE_TOWARDZERO=0xc00 \
  -DFE_INVALID=1 -DFE_DIVBYZERO=4 -DFE_OVERFLOW=8 -DFE_UNDERFLOW=16 -DFE_INEXACT=32"
export RUSTFLAGS="-C link-arg=-zstack-size=33554432"
# Archive core-math with llvm-ar: macOS ar writes no symbol index for wasm objects, so the link
# would miss every cr_* function.
rustup component add llvm-tools >/dev/null
export AR_wasm32_wasip1=${AR_wasm32_wasip1:-"$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin/llvm-ar"}
cargo build --release -p yan-playground --target wasm32-wasip1
cp target/wasm32-wasip1/release/yan_playground.wasm playground/yan.wasm
