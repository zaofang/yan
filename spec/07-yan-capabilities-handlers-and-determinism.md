## 7. Yan: capabilities, handlers and determinism

Effects are capabilities: second-class values passed as explicit parameters in Yan and implicitly by the surfaces. There are no effect variables, effects are statically safe, and common handlers compile to direct calls.

### 7.1 Built-in capabilities

| Name | Interface | Operations (minimum) |
| --- | --- | --- |
| `io` | `Io` | `print`, `eprint`, `read_line`; `split(n)` (4.3) |
| `fs` | `Fs` | `read`, `write`, `append`, `list_dir`, `exists`, `remove`; completion-shaped (9.7); attenuated by `fs.only` and `fs.sub` (17.6, R674) |
| `tmp` | `Tmp` | a scoped scratch directory removed at scope exit |
| `net` | `Net` | `tcp_connect`, `tcp_listen`, `udp_bind`, `resolve`; completion-shaped with deadlines (9.7); attenuated by `net.only` (17.6); the TCP slice and `mem_net` are R789 to R791 |
| `time` | `Time` | clock reads only: `now`, `monotonic` (returns `MonoInstant`, includes suspended time), `sleep`, `sleep_until(MonoInstant)` |
| `rand` | `Rand` | `next_u64`, `next_f64`, `state() -> Option[RandState]` (R823); handlers `seeded(seed, algorithm=ChaCha8())` (default in tests with seed 0; the streams are R683, the split R685; `seeded(seed, Pcg64Dxsm())` is not splittable, so using it in two branches is E402; R377) and `os` |
| `crypto_rand` | `CryptoRand` | `fill_bytes` (in place), `random_bytes`; a CSPRNG that tests may replace and release builds may not |
| `log` | `Log` | structured key-value events at `debug`, `info`, `warn`, `error` over one `event(level, msg, fields)` operation, plus spans as child handlers (R688, R689); `@splittable` |
| `metrics` | `Metrics` | counters, gauges, histograms; `@splittable` |
| `env` | `Env` | `args`, `var`, `vars`, `secret` (returns `Secret[str] \| None`), `local_zone`, `system_tzdb`; no `set_var` (R686); attenuated by `env.only` (R674) |
| `proc` | `Proc` | `run(Command)` taking argv, never a shell string (R692); `spawn(Command)` with piped stdio follows `Reader` and `Writer` (R693); there is no shell helper (R448); attenuated by `proc.only` (R674) |
| `signals` | `Signals` | a stream of OS signals; SIGINT and SIGTERM also drain `main`'s root scope (9.4) |
| `py` | `Py` | `call`, `import_module` (Python bridge, section 17) |
| `fault` | `Fault` | `maybe(name) -> bool`; handlers `never` (default, compiles away) and `sim` (seeded) |
| `toolchain` | `Toolchain` | read-only queries over the query database (section 21.4); never bound in release builds (E318) |
| `nondet` | none | marks scheduler-visible nondeterminism, shared mutable state across tasks, `@unordered`, `@fast_math` and observing hash values |
| `shield` | none | marks code that opens a `shield` block (9.4, R812); like `nondet`, a marker with no operations |
| `alloc` | `Alloc` | `alloc`, `realloc`, `free`; ambient (5.5) |
| `sched` | `Sched` | task placement for `par`, `par for`, `par_fold` and actors |
| `cluster` | `Cluster` | remote spawn and calls between nodes (9.6.1, R400) |
| `gpu` | `Gpu` | device memory and kernel launch (17.1, R327, R377) |
| `critical_section` | `CriticalSection` | interrupt-masked read-modify-write on freestanding targets (17.1, R377). |

`embed` and `diag` are stage-0 only (10.4), and `abort` is a platform setting (17.1), so none of them is a runtime capability.

**User capabilities.** A trait marked `@capability` may appear in `uses`. It binds under the snake\_case word list of its name: `@capability trait HttpClient` is `uses http_client` (section 16). `uses cache: Kv` in Yan and `uses Kv as cache` in Yan Agent (R461) bind it under another name. Capability sets (R431): type Svc = uses net, log, time names a set of capabilities; a set name may appear in any uses list, in both surfaces, and expands in place. Sets may include sets, a cycle is E251, and yan sig shows sets expanded.

**Sharing classes (R342).** Every capability has one sharing class, which fixes how branches that may run at once use it (4.3, 9.2). `sched`, like `alloc`, is ambient.

| Class | Capabilities | In two concurrent branches |
| --- | --- | --- |
| ambient | `alloc`, `sched` | never written in `uses`; always allowed |
| split | `rand` (task-path streams, 7.7), `fault`, `io`, `log`, `metrics` | each branch gets its own handle; `io`, `log` and `metrics` output is committed in source order at the join |
| shared | `net`, `fs`, `tmp`, `time`, `env`, `proc`, `signals`, `crypto_rand`, `py`, `toolchain`, cluster (9.6.1), gpu, critical\_section (R377), foreign-library capabilities | `let` access; operations may run at once, and their results are capability results that `yan replay` reproduces |
| exclusive | user capabilities by default | `inout` access; use in two branches is E402 |

A `@capability` trait is exclusive unless marked `@splittable` or `@shared`. `nondet` has no operations and is only a marker.

**Unconfined capabilities (R673).** `proc`, `py` and every foreign-library capability give the holder the authority of the whole process, because a child process, Python code or C code can do anything the process can. Granting one in a ceiling is reported as granting every capability, gaining one blocks like a publisher identity change (19.4), and `yan audit` lists them first. Every other built-in capability is confined: it reaches only what its handler grants, and has an attenuator (17.6).

**Foreign libraries are capabilities.** `lib zlib` in an extern block declares trait `Zlib`, capability `zlib` and a native handler that calls the linked symbols (section 12.2). Callers need `uses zlib`; tests may bind a fake.

### 7.2 Elaboration and signatures

`uses a, b` on a function becomes parameters `cap a: A, cap b: B` in Yan. Each call passes the capabilities its callee needs from the caller's scope. A capability MUST NOT be stored in data, returned, or captured by an escaping closure (E306). Calling a function whose capabilities are not in scope is E301, and its fix inserts the missing `uses` entry.

In Yan Agent every function lists its capabilities. In Yan only `pub` functions must; others are inferred within their strongly connected component and shown as inlay hints. main is exempt in Yan (R430): it need not be pub, its uses is inferred, and the inferred set must fit the package ceiling (7.4, E314). `alloc` is never written in `uses`. `uses none` forbids every capability, including `alloc`; on an extern item it is a trusted purity claim (12.2).

### 7.3 Handlers

`with name = expr { ... }` binds a handler value for the block. A handler is any value implementing the capability's trait. All built-in capability operations are tail-resumptive: they return to their caller.

- The handlers for `alloc`, `rand`, `time`, `sched` and `fault` MUST be resolvable at compile time to a concrete type, so calls monomorphize to direct calls (E307 otherwise). `with name = dyn expr` opts into one indirect call per operation, where allowed.
- Other capabilities MAY be dynamic; the compiler SHOULD monomorphize when the handler type is known.
- On hosted platforms, std binds a default handler for every built-in capability before `main` runs: system heap, OS clock, OS-seeded generator, the `pool` scheduler, blocking I/O, standard streams, `log = log_stderr(InfoLevel())` (R690), `fault = never`. On platform `freestanding` nothing is bound: the program binds every capability it uses.
- The test defaults are `time = virtual(2000-01-01T00:00Z)`, which advances only when every task is blocked, a timer is pending and no I/O is outstanding (R396), and `rand = seeded`. Binding `virtual` under the `pool` scheduler is E710, because `pool` has no quiescence point. Real-time tests bind `os` explicitly and are never cached. This is the one test handler set: in-memory fs, rand = seeded(0), virtual time, fault = never, env = fake_env(), log = log_stdout(DebugLevel()) into the test's captured output, proc = fake_proc({}), no net (a test binds `mem_net()` itself, R792), no model and sched = seeded(0) (R686, R690, R692); 14.1, 15.3, 18.4 and 18.9 refer here (R377).
- **Host handlers are unforgeable (R672).** A handler that reaches the host is built only by the platform before `main`, from the grants of a `yan_world` (12.7), or by attenuating a handle of the same capability already in scope (`with fs = fs.only(...)`). Std's host handler constructors are not nameable by user code, and binding a host handler anywhere else is E326. Handlers written in Yan (fakes, recorders, replayers, wrappers) are unrestricted, because they reach the host only through capabilities they are given.
- Release builds bind only `fault = never` (E317 otherwise).
- Control handlers are generators only (section 10.5). There are no general continuations and no fiber stacks.

### 7.4 Platforms and ceilings

`platform` in `package.yag` names a capability bundle with default handlers (`cli`, `service`, `job`, `wasm`, `freestanding`). A package's `capabilities` list is a ceiling enforced transitively (section 19.2): using a capability the package did not declare is E314, including through dependencies. Ceilings may carry `net.only` patterns and must list every foreign-library capability.

### 7.5 Time and zones

Time-zone arithmetic against the pinned tzdb is pure and needs no capability. The tzdb is the `std.tzdb` data package, pinned in `yan.lock` and part of the toolchain hash. Host or file tzdb data enters only through a capability call: `env.system_tzdb()`, `TzDb.from_tzif_dir(fs, path)` and `TzDb.from_bytes`. Deadline and timeout scopes lower to `sleep_until(MonoInstant)`.

### 7.6 Record and replay

std MUST provide a recording handler and a replay handler for every built-in capability; a program opts in by binding them. `yan run --record f.trace` logs each operation and its result, per task, in task-tree order. `yan replay f.trace` re-executes, feeding recorded results, and MUST reproduce the run exactly, by the determinism guarantee. A production mode keeps a ring buffer of the most recent trace window and writes it on panic. Each capability exposes a redaction hook applied before recording. A trace replays on the toolchain minor that wrote it (section 20.2). It also records each cancellation point reached and the winner of each timer race, so replay of cancellation is exact (R342).

**Trace format and divergence (R305).** A trace is a header (toolchain minor, root item hashes, edition, handler bindings, seeds) followed by one stream per task, keyed by task-tree path. Each entry holds the op index, capability, operation, an argument digest and the result as canonical `Wire` bytes. Nondeterministic choices are logged only where they occur: `uses nondet` points, `explore` and `pct` decisions, cancellation points and timer races. Under `with replay(trace)`, an operation whose capability, name or argument digest differs from the next entry fails with `ReplayDiverged(task, index, expected, got)`, a typed failure, and `yan replay` prints the first differing operation with its source line. `replay(trace, live=[...])` MAY name only output-only capabilities (`log`, `metrics`), whose operations return nothing the program reads; naming any other capability is a compile error (E316, R379). `yan replay f.trace --against REV` runs the trace on another build and reports the first divergence. `yan replay f.trace --to-test` writes a proposed test (18.4, R300) that binds the trace and asserts no panic, or snapshots the output; the trace is stored under `tests/traces/`.

### 7.7 Determinism

**Guarantee (normative).** The result of any computation is a deterministic function of its arguments and of the values returned by its capability operations (SQ19). Scheduling MUST NOT be observable. Concretely:

- `par` branches share no mutable state (section 4), and their results are joined in source order. Shared mutable state needs `nondet` (6.5).
- `par for` returns results in input order. `par_fold` reduces in the fixed tree of section 9.3, determined only by the input length.
- Floating point is IEEE 754 strict: no fused multiply-add contraction, no reassociation, no flush-to-zero, unless the function uses `nondet` via `@fast_math` or `@unordered`.
- **Relaxed floats (R821).** `with fast_math:` is a lexical block, and `@fast_math` on an item means its whole body is one such block; both need `uses nondet`. Inside the block the float operators written there may be contracted into FMA, reassociated, treated as if the sign of zero did not matter, and `a / b` taken as `a * (1 / b)`; tier-F approximate math functions (11.1) may stand in for their correctly rounded counterparts. NaN and infinity keep their IEEE meaning and subnormals are kept: safe code never assumes values are finite. The scope is lexical: a called function is not relaxed and inlining never carries the relaxation into or out of the block. `fma(a, b, c)` stays strict. Flush-to-zero is outside the set on CPUs. `with unordered:` is the block form of `@unordered` with the same scope rule (R822).
- **Strict escape and lowering (R822).** Inside a relaxed scope, `with strict:` restores the strict meaning for its block. Yan Core has the statement `relaxed { ... }`, which carries the relaxation as a block, never a per-operation flag. `yan view` shades relaxed spans and `yan audit` lists them. W858 reports a relaxed block with no float operator and W859 a `with fast_math:` nested in a relaxed scope; a `with strict:` is not reported.
- Iteration order of `Map` and `Set` is insertion order, never hash order.
- Seeded randomness across tasks follows the task path: a branch's seed derives from its parent's seed, the parent's split counter and the branch index, so consecutive `par` blocks draw independent streams (R824, 17.3).
- Query results do not depend on thread count, ISA or memory budget (section 11.9).

**Scope of the strong guarantee (R342).** The bit-identical promise of law 7 holds when no stateful handler of a shared capability (7.1) is used by tasks that may run at once. When one is, results remain a deterministic function of the values its operations return, which `yan replay` reproduces, but they may differ between schedules. Buffered output of a split capability in a `par` branch cancelled by another branch's failure is dropped, never committed (9.2). Under a bounded alloc handler, try\_ results in tasks that may run at once also fall in this tier (R398).

**Determinism inputs.**

- **Hashing (R387).** The default `Hasher` is a foldhash-class hash keyed with a secret per-process seed. Each `Map` and `Set` counts probe length; past a fixed threshold it rebuilds its index once with keyed SipHash-1-3 for the rest of its life, which defeats hash flooding and is unobservable because iteration is insertion-ordered. Observing a hash value requires `nondet`. `UnorderedMap[K, V]` and `UnorderedSet[K]` store entries inline in a SwissTable; lookup, insert and remove are deterministic, and iteration requires `nondet` or goes through `sorted_items()`. `std.hash.stable` gives fixed-key, versioned hashes for content addressing. Unkeyed and fixed-seed hashers are not in std; @yan/fasthash has them (R448).
- **Math library.** `sin`, `exp`, `pow` and the rest come from the bundled correctly rounded libm, never the platform's, under the tiers of section 11.1. NaN payloads are canonicalized at `Wire` boundaries. Floats compare with IEEE `==`, so `-0.0 == 0.0` and NaN is unequal to itself. `Ord` on floats is IEEE totalOrder with `-0.0` and `0.0` identified and every NaN ordered after every number, so `sorted` is total and agrees with `==` on numbers. Floats do not implement `Hash`, so a float is never a `dict` key or `set` element (R344).
- **Pinned data.** The Unicode version is pinned per edition. The tzdb, CLDR release (R333), ISO 4217 table and `math_edition` are pinned in `yan.lock`.
- **Solver.** Proof results depend only on deterministic resource limits (section 8.7).
