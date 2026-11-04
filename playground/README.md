# Playground

A browser page that checks and runs a module and shows it in every surface at once: Yan Core,
Yan Agent, Yan and Yan Meta. Everything runs client side in `yan.wasm`, built from
`crates/yan-playground` over `yan-db`, `yan-ide` and `yan-interp`.

## Build and serve

Needs the `wasm32-unknown-unknown` target, a clang with the WebAssembly backend and the
wasi-libc headers (`core-math` is C).

```sh
rustup target add wasm32-unknown-unknown
# Debian/Ubuntu: apt install clang wasi-libc
# macOS: brew install llvm wasi-libc; then
#   CC_wasm32_unknown_unknown=$(brew --prefix llvm)/bin/clang \
#   WASI_INCLUDE=$(brew --prefix wasi-libc)/share/wasi-sysroot/include/wasm32-wasi
./playground/build.sh
python3 -m http.server -d playground 8000
```

Then open http://localhost:8000.

## What each pane shows

| Pane  | From Yan Agent source         | From Yan source          | From Yan Core source |
|-------|-------------------------|--------------------------|------------------|
| Yan  | elaborated module       | elaborated module        | the source       |
| Yan Agent   | `yan fmt` of the source | `view --as yag`          | `view --as yag`  |
| Yan   | no printer yet          | `yan fmt` of the source | no printer yet   |
| Yan Meta | no printer yet          | no printer yet           | no printer yet   |

"Edit this" loads a pane's text into the editor as that surface, so a round trip is two
clicks.

## Limits

- `run` uses the interpreter. Programs that read the real clock trap: wasm32 has no
  `SystemTime`. A run that takes longer than 10 s is stopped.
- core-math's C calls to `fenv.h` and stdio are stubbed in `worker.js`; wasm has only
  round-to-nearest, so results match native builds.
- `examples.js` holds programs from `conformance/` with their header comments dropped.
