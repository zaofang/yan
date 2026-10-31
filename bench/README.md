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
| `sieve` | `List[bool]` push, index read and write | primes up to 500,000 |
| `ring` | a 64-slot ring buffer over `List[i64]` | 500,000 rounds |

Sizes are set so each program takes one to two seconds under the M0 interpreter. Raise them when
native code generation lands, since the Rust times are near the timer floor today.

## M0 baseline

Linux x86_64 (4 vCPU Xeon, 2.1 GHz), CPython 3.14.6, rustc 1.97.1, `yong` with #16 (lists read
by reference), 11 runs.

| program | yong ms | python ms | rust ms | yong/python | yong/rust |
|---|---:|---:|---:|---:|---:|
| collatz | 2032.6 | 46.7 | 1.6 | 43.5x | 1243x |
| fib | 674.6 | 12.1 | 1.6 | 55.7x | 435x |
| loop | 774.1 | 30.8 | 0.9 | 25.1x | > 774x |
| mandel | 512.1 | 17.1 | 1.1 | 30.0x | 470x |
| ring | 1122.8 | 36.8 | 0.3 | 30.5x | > 1123x |
| sieve | 1311.8 | 25.8 | 2.1 | 50.8x | 628x |

Geometric mean, yong/python: 37.6x. Python times here are tens of milliseconds over a ~20 ms startup,
so they move by up to 2x between sessions on a shared host; read the Python ratios as 20x to 60x.
