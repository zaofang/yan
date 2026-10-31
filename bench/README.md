# Benchmarks

The benchmark set of spec 23 (R389) lives here. Each directory under `programs/` holds one program in
Yong text (`main.yong`), typed Python (`main.py`) and Rust (`main.rs`). All three use the same
algorithm and input size and assert the same result, so a wrong answer fails the run instead of
producing a fast time. Zig and C versions join when the M2a gate needs them.

```sh
cargo build --release -p yong-cli
uv run python bench/run.py            # all programs, 5 runs each after a warmup
uv run python bench/run.py fib sieve  # a subset
uv run python bench/run.py --json bench.json
```

Times are median wall times minus the median time of the same language's `startup/` program.
Rust is built with `-C opt-level=3 -C overflow-checks=on`, matching the gate's "overflow checks on".
Below about a millisecond the Rust time is timer noise, and the report marks those ratios as lower
bounds.

## Programs

| Program | Exercises | Size |
|---|---|---|
| `fib` | non-tail calls, `decreases` | fib(27) |
| `loop` | a counted loop with checked integer ops | 1,000,000 iterations |
| `collatz` | `become` tail calls, `requires` on every call | starts 1 to 10,000 |
| `mandel` | `f64` arithmetic, strict floats | 100 x 100 grid, 100 iterations |
| `sieve` | `List[bool]` push, index read and write | primes up to 10,000 |
| `ring` | a 64-slot ring buffer over `List[i64]` | 500,000 rounds |

Sizes are set so each program takes one to two seconds under the M0 interpreter. Raise them when
native code generation lands, since the Rust times are near the timer floor today.

## M0 baseline

Linux x86_64 (4 vCPU Xeon, 2.1 GHz), CPython 3.14.6, rustc 1.97.1, `yong` at 9bc606d, 5 runs.

| program | yong ms | python ms | rust ms | yong/python | yong/rust |
|---|---:|---:|---:|---:|---:|
| collatz | 1893.3 | 62.2 | 2.2 | 30.4x | 852x |
| fib | 752.8 | 27.1 | 2.3 | 27.8x | 333x |
| loop | 828.0 | 41.6 | 1.1 | 19.9x | 756x |
| mandel | 503.2 | 18.3 | 1.5 | 27.5x | 331x |
| ring | 1361.9 | 72.2 | 0.4 | 18.9x | > 1362x |
| sieve | 1444.8 | 2.3 | 0.1 | 639.8x | > 1445x |

Geometric mean, yong/python: 42.1x. Without `sieve`, the interpreter runs scalar code 19x to 30x
slower than CPython. `sieve` is quadratic: the interpreter clones the whole list on every index read
and on every `inout` argument.
