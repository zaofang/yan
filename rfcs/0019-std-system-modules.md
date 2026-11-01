# 0019: std.bits, std.rand, std.env, std.log, std.proc and std.cli

- Status: draft
- Research: none; this RFC is the proposal (Wave 3 of the std build plan)
- Rows: filled in when accepted

## Summary

These are the last six Wave 3 modules that do not wait on `derive(Wire)`. The 17.3 table and the
7.1 capability table name each one in a line, and none has an API yet. This RFC fixes the API of all
six so the implementation can land one module per PR.

Three rules apply throughout:
- Every operation that reads the host goes through the module's capability (7.1).
- Every capability has a deterministic test handler.
- Every derived operation is defined in terms of the capability's primitive operations, so a seeded
  stream or a captured log is the same under every implementation.

Deferred, each to its own later proposal:
- `derive(Config)`, which needs the toml codec and so `derive(Wire)`.
- `env.system_tzdb()`, which comes with the host tzdb reader.
- `proc.spawn` and piped stdio, which wait for `Reader` and `Writer`.
- A JSON-lines log handler, which waits for `derive(Wire)`.

## Decisions

### D1: `std.bits` functions

`std.bits` is in the `core` tier and needs no capability. Its functions take any fixed-width integer
type or `int`, and work on the two's-complement bit pattern.

| Function | Result |
| --- | --- |
| `bits.count_ones(x)`, `bits.count_zeros(x)` | `int` |
| `bits.leading_zeros(x)`, `bits.trailing_zeros(x)` | `int`; the width when `x` is 0 |
| `bits.bit_width(x)` | `int`, the width minus `leading_zeros` |
| `bits.rotate_left(x, n)`, `bits.rotate_right(x, n)` | same type as `x`; `n` is taken modulo the width |
| `bits.swap_bytes(x)`, `bits.reverse_bits(x)` | same type as `x` |
| `bits.is_power_of_two(x)` | `bool`; false for 0 and for negative values |
| `bits.wrapping_shl(x, n)` | same type as `x`; shifted-out bits are discarded |

`bigint` gets `bits.count_ones` and `bits.bit_width` (Python's `int.bit_count` and
`int.bit_length`, which use the magnitude) and nothing else, since it has no fixed width.

The functions are called through the module, as `bits.count_ones(x)`, following `math`. Methods on
the integer types were weighed and rejected, because `math` set the module-call convention.

Recommendation: accept.

### D2: bit operators

3.4 already says that `& | ^ ~ << >>` work on integer types through `Bits`, and that a shift by at
least the width panics with `overflow`. The compiler only has `& | ^` on `int`. This item settles
the remaining cases.

For fixed-width integers and `int`:
- `>>` is arithmetic on signed types and logical on unsigned types.
- `<<` panics with `overflow` when it shifts out a bit that differs from the result's sign bit. So
  `x << n` is `x * 2**n` exactly or panics, which matches checked arithmetic. `bits.wrapping_shl`
  discards those bits instead.
- The shift amount is any integer type. A negative amount, or one at least the width, panics with
  `overflow`.
- `~x` is the bitwise complement.

For `bigint`, `& | ^ ~` act on the infinite two's-complement form, as in Python. `<<` and `>>` never
overflow, and `>>` floors. A negative shift amount panics with `overflow`.

Rejected: Rust's rule, where `<<` silently drops bits. A Python author writes `1 << 70` and expects
the exact value, so a panic that names `bigint` is the safer answer for a checked `int`.

Recommendation: accept.

### D3: `rand` streams and handlers

The handlers are:
- `seeded(seed: u64, algorithm: Algorithm = ChaCha8())`. This is the test default, with seed 0
  unless `yong test --seed` gives another.
- `os()`. This is the default under `yong run`, and it reads the operating system's CSPRNG.

`type Algorithm = ChaCha8() | Pcg64Dxsm()` is declared in `std.rand`.

Each algorithm has one fixed stream per seed, versioned with the module (20.5):
- **ChaCha8.** The key is four SplitMix64 outputs from `seed`, written little-endian. The nonce and
  the counter start at 0. `next_u64` takes the block words in order, two at a time, low word first.
  These are the `rand_chacha` 0.3 words for `ChaCha8Rng::from_seed(key)`.
- **Pcg64Dxsm.** This is NumPy's `PCG64DXSM`. The 128-bit state and increment come from four
  SplitMix64 outputs from `seed`, and the increment is forced odd.

The interpreter's selftests pin the first outputs of both algorithms for seeds 0 and 42.

Recommendation: accept.

### D4: derived `rand` operations

These are module functions with `uses rand`. Each one is defined only through `next_u64`, so every
handler, including a user's, gives the same values for the same stream.

| Function | Definition |
| --- | --- |
| `rand.next_f64() -> float` | `(next_u64() >> 11) * 2**-53`, in `[0, 1)` (the existing trait operation, now pinned) |
| `rand.below(n: int) -> int` | uniform in `[0, n)` by Lemire's method on 64 bits; panics with `value` when `n <= 0` |
| `rand.range(lo: int, hi: int) -> int` | `lo + below(hi - lo)`; panics with `value` when `hi <= lo` |
| `rand.bool() -> bool` | the top bit of `next_u64()` |
| `rand.choice(xs: list[T]) -> T` | `xs[below(len(xs))]`; panics with `bounds` when `xs` is empty |
| `rand.shuffle(var xs: list[T])` | Fisher-Yates from the last index down, swapping `i` with `below(i + 1)` |

Rejected: Python's `randint(a, b)` with an inclusive end, because `range` matches `range()`.

Recommendation: accept.

### D5: `rand` splits by task path

7.1 makes `rand` a split capability. Branch `k` of a `par` (counting from 0) gets a handler seeded
with `splitmix64(parent_seed ^ splitmix64(k + 1))`, using the same algorithm. The parent's stream is
unchanged. So results do not depend on scheduling, and adding a branch does not shift the others.
`os()` splits into independent `os()` handlers. `seeded(seed, Pcg64Dxsm())` is not splittable, so
using it in two branches is E402, as 7.1 says.

Recommendation: accept.

### D6: `std.env` operations

| Operation | Meaning |
| --- | --- |
| `env.args() -> list[str]` | the program's arguments, without the program name: `yong run f.xin -- a b` gives `["a", "b"]` |
| `env.var(name: str) -> str \| None` | a variable; an unset variable or one that is not valid Unicode is `None` |
| `env.vars() -> dict[str, str]` | every valid-Unicode variable, sorted by name |
| `env.secret(name: str) -> Secret[str] \| None` | `env.var` wrapped in a `Secret` |
| `env.local_zone() -> TimeZone` | as R603 |

There is no `set_var`. The environment is a shared capability, and mutating it while other tasks
read it is the race that made Rust mark `set_var` unsafe.

The test handler is `fake_env(args: list[str] = [], vars: dict[str, str] = {}, zone: TimeZone =
TimeZone.utc())`. The default `env` handler under `yong test` is `fake_env()`.

The 17.3 table changes `secret(name) -> Secret[str]` to return `Secret[str] | None`.

Recommendation: accept.

### D7: `Secret[T]`

`Secret[T]` is a built-in opaque type in `std.env`, until generic classes let std declare it.
- `Secret(x)` wraps a value, and `s.expose() -> T` unwraps it.
- `show(s)` prints `Secret(***)`.
- `==` is constant-time for `Secret[str]` and `Secret[bytes]`.
- It is not `Ord`, `Hash` or `Wire`, as 17.4 requires.

Zeroizing on drop and the hashed replay form arrive with code generation and replay. The interpreter
does neither, and the gap is logged in SPEC_QUESTIONS.

Recommendation: accept.

### D8: log events and fields

A log event is written `log.info(msg, key=value, ...)`, and the same holds for `debug`, `warn` and
`error`.
- Every keyword argument is a field, and any `Show` type is allowed.
- A field records its name and `show(value)`, in call order. Typed field values come with
  `derive(Wire)`.
- `type Level = Debug() | Info() | Warn() | Error()` is ordered in that order.

The trait operation underneath is `event(level: Level, msg: str, fields: list[(str, str)])`, and
the four calls lower to it. A user handler implements only `event`.

Recommendation: accept.

### D9: log spans are handlers

`log.span(name: str, key=value, ...) -> Log` returns a child handler, and it is used with the
existing `with` form:

```
with log = log.span("load", file=path):
    log.info("parsed", rows=n)
```

Every event under the child carries the span path, which is `name` joined to the parent's with `/`,
and the span's fields before its own. A span emits no event itself. OpenTelemetry start and end
times come with the `time`-aware OTel handler in `@yong/otel`.

`par` branches inherit the current span. Their output is committed in source order at the join,
as 7.1 already says.

Recommendation: accept.

### D10: log handlers and the text format

Events print in logfmt: `level=info msg="parsed" span=load file=a.csv rows=3`.
- A value is quoted when it is empty or holds a space, `=` or `"`.
- Inside quotes, `\"`, `\\` and `\n` are escaped.

The handlers are:
- `log_stderr(level: Level = Info())` is the default under `yong run`. It writes to stderr, and
  each line starts with `ts=` and the host time as RFC 3339 UTC to the microsecond.
- `log_stdout(level: Level = Debug())` writes to stdout with no timestamp, so output is
  deterministic.
- `discard_log()` drops every event.

Under `yong test` the default is `log_stdout(Debug())` into the test's captured output. That output
is shown when the test fails, so log lines never change a verdict.

Recommendation: accept.

### D11: `proc.Command`

`Command(program: str, args: list[str] = [])` is a value, and each builder method returns a new
`Command`:
- `.arg(s)` and `.args(xs)`.
- `.env(name, value)` and `.env_clear()`.
- `.cwd(p: P: AsPath)`.
- `.stdin(b: bytes)`, which defaults to empty.

`program` is `argv[0]`. The OS handler resolves it through `PATH` when it has no `/`, and never
runs a shell (R448). An argument is never split or expanded.

Recommendation: accept.

### D12: `proc.run` and `Output`

`proc.run(cmd: Command) -> Output fail ProcError` waits for the child and captures both output
streams as `bytes`.

`Output` has these parts:
- `status: ExitStatus`, where `ExitStatus = Exited(code: int) | Signaled(signal: int)`.
- `stdout: bytes` and `stderr: bytes`.
- `out.success() -> bool`, which is true only for `Exited(0)`.

A non-zero exit is data, not a failure, as in Python's `subprocess.run` without `check`.
`ProcError = { program: str, reason: ProcReason }`, where `ProcReason = NotFound() | Denied() |
Io(msg: str)`, covers only a child that could not start or be read. `show(e)` prints
`<program>: <reason>`.

The test handler is `fake_proc(outputs: dict[str, Output])`, keyed by `program`. An unknown program
fails with `NotFound`. The default under `yong test` is `fake_proc({})`.

Recommendation: accept.

### D13: `proc.spawn` waits for streams

`spawn(cmd) -> Child`, with piped stdio, backpressure and children owned by the spawning `par`
scope, needs the `Reader` and `Writer` types of 17.3 `io` and scope-owned handles. It gets its own
proposal after them.

Recommendation: accept the deferral.

### D14: `derive(Cli)` mapping

`@derive(Cli)` applies to a record whose fields have supported types. The rest of this item is how
fields become arguments.

| Field | Argument |
| --- | --- |
| no default | a positional argument, required, in field order |
| `list[T]`, no default | the remaining positionals; at most one, and last |
| `bool`, default `False` | a flag, `--name` |
| any other default | an option, `--name VALUE` or `--name=VALUE` |
| `T \| None`, default `None` | an optional option |
| `list[T]`, default `[]` | a repeatable option |
| a sum of unit variants | its value is the kebab-case variant name, such as `--color dark` |
| a sum whose variants are records | a subcommand, one per variant, parsed by the same rules; at most one, and last |

The rest of the mapping:
- Names are kebab-case: `max_depth` becomes `--max-depth`, and variant `DryRun` becomes `dry-run`.
- Supported value types are `str`, `int`, `float`, `bool`, `Path`, the unit sums above, and
  `T | None` and `list[T]` of those. Any other type is E269 at the derive.
- `--` ends the options.
- `-h` and `--help` are reserved.
- There are no short options. They need field decorators, which do not exist.

Help text comes from `@help(about="...", field="...", ...)` on the record, because docstrings are
hash-neutral trivia (16.5) and may not change behavior. A `@help` key that names no field is E269.

This is the clap derive's positional-by-default rule, with Typer's choice that a default makes an
option.

Recommendation: accept.

### D15: `cli.parse` and deterministic errors

There are two calls:
- `cli.parse[T: Cli]() -> T fail CliError uses env` parses `env.args()`.
- `cli.parse_args[T: Cli](args: list[str]) -> T fail CliError` is pure.

`CliError = { message: str, help: bool }`:
- `help` is true for `-h` and `--help`. Then `message` is the full help text, and the program
  prints it and exits 0.
- Otherwise `message` is one error line followed by the usage line.

The first problem in argument order is the one reported, with fixed wording:
- `unknown option '--x'`
- `missing argument <path>`
- `option '--n' needs a value`
- `invalid value 'abc' for '--n': expected int`
- `'--n' given twice`
- `unknown command 'x'`

The help text layout is also fixed:
1. `usage: <prog> [options] <positionals>`, where `<prog>` is the record name in kebab-case.
2. The `about` text.
3. The positionals.
4. The options, with each option's type, its default shown by `show`, and its `@help` text, in
   field order.

So the help text is a stable snapshot.

Recommendation: accept.

## Spec changes

- 3.4: the D2 operator rules.
- 7.1 and 7.3: the `rand` handler signatures and streams (D3, D5), and the `env`, `log` and `proc` test
  defaults (D6, D10, D12).
- 17.3: table rows for `bits`, `env`, `log`, `std.proc` and `std.cli`, and one paragraph per module
  (D1, D3 to D15).
- 17.4: the `Secret` operations (D7).
- 25: E269 gains the `derive(Cli)` cases. No new codes.
