## 23. Milestones and acceptance criteria

A milestone is complete only when every criterion below passes in CI. Performance figures are targets. The M1 gate is the project's central bet, and its result decides whether the later milestones proceed unchanged. Dates and staffing are in the 1.0 Roadmap.

### P0 · Setup

- Repositories, the Cargo workspace scaffold, CI on the self-hosted runner, and the path guard that rejects an agent patch touching `tests/`, `conformance/` or `spec/`.
- No blocking `SPEC_QUESTIONS.md` entries.
- Evaluation tasks, typed-Python baselines and held-out tests sealed. The imports of the sealed baselines are scanned, and every Python module used by at least 1% of them needs a Yan Agent equivalent by M1.

### M0 · Yan core

- `yan-core` data types; the text parser and printer round-trip every `conformance/yir` file.
- The validator rejects every malformed-IR test with an internal-error code.
- Type checker for Yan, plus ownership, exclusivity and views (section 4), capabilities (section 7) and failure (section 8).
- Exhaustiveness checker and bounds solver for the decidable fragment.
- Interpreter runs checked Yan Core, with panics, `par` (sequential execution is acceptable) and contracts at the `check` level.
- Core semantics: UTF-8 strings and `char`, integer conversion rules, associated types, join points, drop order, error injection and context, contract semantics.
- Diagnostics in the 18.2 format from the start.
- Hash-key independence: every conformance test runs under two hash keys and the outputs are identical.
- **Probe (R566).** Runs as soon as the Yan Agent lexer, parser, lenient reader, elaborator and interpreter pass the conformance cases they cover. Two model families write Yan Agent for 50 development tasks (never the sealed set), zero-shot and 5-shot. Recorded: the share that parses after lenient reading (target at least 95%), the share that checks, the diagnostics hit, and tokens against typed Python, split as 21.6 says. Until the probe reports, no row changes Yan Agent syntax (13, 14.3, 14.4); afterwards a row that changes it cites a probe or M1 measurement. Rows for std, Yan Core, Yan and Yan Meta are not held.

### M1 · Yan Agent and the agent toolchain

- Yan Agent lexer, parser, lenient normalizations N01 to N40, canonical printer.
- Elaborator to Yan Core, including inference, capability passing, copy, share and move insertion, narrowing and fail-transparent closures.
- Standard library: the M1 row of the std roadmap, including prelude, text, list, dict, set, iter, collections, rc, slices with bisect, fmt, regex, csv, encoding, path, cli, `proc.run`, rand, bigint, bits, `Secret`, log, io, fs, json, math, test; Temporal-style time types.
- `yan check` with lints, `run` (including scripts), `test` with verdict caching, `fmt`, `sig`, `patch`, `undo`, `explain`, `fix`, `export --python`, `import --python`, `mcp`, `mcp schema`, `add`, `remove`, `update`, `tree`, `why`, `vendor`, `x`, `schema gen`, `toolchain`, `skill`.
- Agent features: spec gate, typed holes, autofix, `pack_context`, `search_type`, `query` over the `toolchain` capability; the grammar exported for llguidance, XGrammar and GBNF.
- Safety: `Wire` limits, `Secret`, `proc`, `crypto_rand`, capability ceilings, test isolation, lexical safety, E312, spec freeze, protected targets, the untrusted-content channel.
- Golden programs G1 to G4 run on the interpreter and, exported, under CPython 3.12 with identical output.
- The registry design (section 19) written and reviewed.
- **Benchmark harness.** Translate a public benchmark (MultiPL-E HumanEval and MBPP, or LiveCodeBench) into Yan Agent and into typed Python with identical hidden tests. Run at least two model families in the same agent loop, with and without constrained decoding. Record pass@1, total tokens per solved task (prompt, reasoning, output and repairs), repair rounds, the failure mix and the cheating rate (21.6).
- **Gate (R567):** Yan Agent's pass@1 is at least typed Python's, and both token measures of 21.6 are lower: its body and repair tokens per solved task than typed Python's, and its spec-phase tokens than those of the pytest cases the same model writes for the same task. The cheating rate and the capability violations the checker caught are reported beside the gate. If the gate fails, record the failure mix, move Yan Agent closer to Python, and apply the fluency plan of 21.6 before continuing.

### M1b · Native code

Order: the JIT lands first, then ahead-of-time (AOT) output. M1b is complete only when both pass.

- JIT: the Cranelift backend runs every `run` test with output identical to the interpreter.
- AOT: layouts (section 5.2) and drop glue (section 5.4) for native values, the runtime as a static library, object emission from the Cranelift backend, and `yan build` linking with the system linker. Every `run` test built with `yan build` matches the interpreter in stdout and exit code, and the live-allocation report is zero at exit.

### M2 · Systems layer, performance and library

M2a (systems layer and performance; gate: Rust parity):

- Monomorphization, comptime specialization, ownership passes, fusion, static handler resolution.
- LLVM backend; release builds; Windows and WebAssembly targets.
- Yan lexer, parser, printer and lowering to HIR, pulled forward from M3 (R969); the S0 readiness gate of 22.7 (R967).
- Systems layer: `unsafe`, `Ptr`, `RawCell`, layout control, SIMD `Vec`, atomics and the memory model (section 6), `yan test --ub` and `--explore` on native builds (their interpreter forms are M1, R378), FFI and the C profile (section 12), `core`, `alloc` and `std` tiers and platform `freestanding`, with a freestanding binary in the conformance suite that passes the embedded profile checks and prints its stack bound (R320, R321, R324, R325). The rest of the embedded profile (R322, R323, R326) is M4.
- Concurrency: `pool` scheduler, `par concurrent`, cancellation and drain, `std.evented` (io\_uring with epoll fallback, kqueue, IOCP) as a preview.
- Determinism tests pass, and record and replay works for every built-in capability.
- Benchmark set (R389), checked into `bench/` with equivalent Rust, Zig and C programs: numeric kernels, table queries, a JSON service, a zero-copy parser, a graph over `Pool` and `Handle` (arena Ref is M4), and a ring buffer. Gate: geometric mean at parity or better with Rust and within 5% of C and Zig, with overflow checks and strict floats on; each opt-out (wrapping ops, `@fast_math`, `@unordered`) reported separately. Release builds use whole-program ThinLTO by default; `yan build --pgo profile.yprof` applies a profile whose hash is pinned in `yan.lock`.

M2b (library, registry and tools):

- Full standard library of section 17, including `table` over Arrow, `array`, units, `net`, `dns`, `tls`, `http`, `sim`, actors, decimal and money, the correctly rounded libm, and CPython embedding and extensions.
- Registry and supply chain built to section 19; bundled lld and pinned clang; reproducible builds with `--verify`.
- DWARF, native debugging with checkpoints, profiling, native coverage and fuzzing, and mutation testing (the interpreter debugger, replay, coverage, fuzzing, --ub, --explore and yan doc are M1, R378); `yan semver`, `publish`, `audit`, `approve`, `sbom`, `rebuild`.

### M3 · Yan

- Self-hosting stages S1 to S3 of 22.7 (R967).
- Projections in both directions; all round-trip laws that apply pass on the whole suite, and `conform.py --laws` gates CI (R564).
- The print-only Yan Meta view, `yan view --as ymt` (16.6, R563).
- Prose diagnostics; language server; Yan tree-sitter grammar; `yan repl` (R990).
- Yan surface: grapheme-aware display, date literals, `select`, generators, deadline blocks; HTTP server; localizable diagnostics.

### RC · Release candidate

- The 1.0 edition is named; the freeze list (20.2) takes effect; the release train (20.7) starts.
- Self-hosting stages S4 and S5 of 22.7 pass; `yan` ships from stage 2 and stage 0 is frozen (R966, R967).

**Docs gate (R319).** Every fence in the tour, book, guides and std passes, and std `pub` doc coverage is 100%. The doc model and links land in M0; `yan doc`, the fence runner and doc lints in M1; the wasm interpreter in M1b; the surface toggle, tour and playground in M3, the book in 1.x (R990).

### 1.x · After 1.0

Ships under the release train of 20.7, not before the 1.0 edition (R991).

- Reactive notebooks (`yan nb`) and live values computed on examples; depth-lens inlay hints; the item-level merge driver (`yan merge-driver`).
- The full book, beyond the tour, by-example pages and std reference of R319.
- Randomized comparison (at least 30 participants) on contested Yan syntax: implicit-subject lambdas and the typed implicit subject in frame expressions, grid literals, unit suffixes, `\(x)` interpolation, and the `&x` mark. It reports after 1.0 (R969).

### M4 · Depth

- `prove` level with Z3 or cvc5 through SMT-LIB2; counterexamples in V401; proof cache keyed by hashes.
- L4 to L6 remainder: arena `Ref`, proof attributes, reflection and attribute functions at full scope.
- Reverse-mode autodiff preview; `std.evented` handlers and `@yan/quic` considered for stabilization.
- Remote scheduling with `RemoteError` failure types and `Wire` checking.
- CHERI targets; a native Tree Borrows sanitizer (candidate).
- Stretch: GPU code generation for `par for` bodies with `uses none`, through MLIR and LLVM to Apple Metal first, then NVIDIA, AMD and Vulkan (R327 to R329), with `@fast_math` allowed under `nondet`.
- WASM components (`wasm32-wasip2`, capabilities as WASI interfaces, platforms as WIT worlds), with `yan bind --lang js` writing an npm package through jco, published with npm trusted publishing (R641).
- Yan Meta (16.6, R516): reader, printer with spelling notes and lenient reader (the print-only view is M3, R563); the grammar moves from RFC 0001 into section 16.6 first, and every Yan conformance program round-trips through Yan Meta by item hash.
- Niche surfaces (16.7, R536 to R550) are post-1.0 and follow the order of R550; none is part of any milestone above. Durable timers (R552) ship with `@yan/durable`.
