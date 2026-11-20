## 22. Implementation plan

This section is normative where it says MUST; elsewhere it is the recommended path. Deviations SHOULD be recorded in `SPEC_QUESTIONS.md` with the reason. The Yan Technical Implementation Spec sits under this section and adds no language behavior.

### 22.1 Two implementations

The 0.1.0 toolchain is written in Yan (R966, RFC 0057), which replaces R215. Until then Yan Core has one implementation, bootstrap stage 0, written in Rust with the crate layout below; it bootstraps the Yan compiler, is the oracle every port is compared against, and is frozen when the fixpoint gate of 22.7 passes. In this section, 22.7 and 23, stage 0 to stage 3 name bootstrap compilers; the stage 0 of 10.4 is compile-time evaluation. M0 and M1 run on the interpreter, whose checked mode defines the abstract machine (6.1); Cranelift debug code generation follows in M1b. The conformance runner is implementation-independent and is the oracle: for every conformance and evaluation program, the interpreter and every code generator MUST produce the same result, diagnostic codes included. For Yan Agent programs, the output of `yan export --python` run under CPython 3.12 is an independent second check. Performance targets apply to native code.

**Formal core model (R680).** `formal/` holds a Lean 4 model of Yan-K, the fragment of Yan Core text named in RFC 0022: a reader, an executable semantics, and from phase 1 on the proofs of the formal core study. It stays outside the toolchain-language rule (R966): it never ships, no crate depends on it, and building the toolchain never needs Lean. `formal/drt.py` runs the `conformance/yir/run` cases and generated Yan-K programs on both the interpreter and the model and compares output, exit status and panic kind; a difference is a bug in one of them or a gap in this spec. Proofs start after the M1 gate.

### 22.2 Crates

Stage 0 (22.1) MUST be written in Rust (stable toolchain) as one Cargo workspace. The self-hosted compiler (22.7) ports the same crates and keeps their responsibilities and layering:

| Crate | Responsibility | Key dependencies |
| --- | --- | --- |
| `yan-db` | query database, item hashing, file inputs, stage-0 query cache | `salsa`, `blake3` |
| `yan-syntax` | shared lexer infrastructure, lossless syntax trees, trivia | `rowan` |
| `yan-yag` | Yan Agent lexer (INDENT/DEDENT), parser, lenient normalizer, canonical printer, exported grammars |  |
| `yan-yn` | Yan lexer, parser and printer (M3) |  |
| `yan-names` | word-list and foreign names, the projection table (16.1 to 16.3) (R560) |  |
| `yan-surface` | the `Surface` trait (parse, abstract, lower, subset, resugar, print, optional lenient reader, grammar export, message rendering), the trivia side table, the round-trip law harness (R560) |  |
| `yan-ymt` | Yan Meta lexer, parser, printer and spelling notes; print-only view in M3, reader in M4 (R560, R563) |  |
| `yan-hir` | name resolution, the surface-neutral high-level IR every surface lowers to, and the shared raise from Yan (R558, R559) |  |
| `yan-elab` | type inference (bidirectional, local inside items), desugaring, capability passing, copy and move insertion, lowering to Yan Core |  |
| `yan-core` | Yan IR data types, text parser and printer, IR validator |  |
| `yan-check` | ownership, exclusivity, views, auto traits, exhaustiveness, bounds solver, unit and shape checking |  |
| `yan-prove` | verification-condition generation and SMT-LIB2 subprocess client (M4) |  |
| `yan-opt` | Yan to Yan Core: monomorphization, comptime specialization, ownership passes, fusion, handler resolution, query-plan specialization (R771, R772) |  |
| `yan-mir` | mid-level IR for code generation from optimized Yan Core: per-instance control-flow graphs, layouts, drop glue, borrow lowering, decision trees, intrinsic ids (R560, R773) |  |
| `yan-interp` | interpreter over checked Yan Core: compile-time evaluation (bytecode, 10.4), REPL, `yan run` in M1, checked UB mode, `explore` |  |
| `yan-cg-cranelift` | debug code generation (M1b) | `cranelift-*` |
| `yan-cg-llvm` | release code generation, written in Yan over printed MIR (M2, R1049) | LLVM C API |
| `yan-rt` | std support: alloc handlers, `pool` scheduler, blocking platform I/O, record and replay handlers, panic-abort hook | mimalloc, a heartbeat scheduler for pool (R481); corosensei, io-uring only in `std.evented` |
| `yan-std` | standard library sources (`.yag` and `.yn`, in `core/`, `alloc/` and `std/`) plus the intrinsic registry (R561, R562) | `arrow`, `regex-automata`, `rustls` with `aws-lc-rs`, `jiff`, ICU4X segmenter, a CORE-MATH-style libm |
| `yan-ffi` | `yan ffi import`, `yan bind`, the C profile | libclang (import only) |
| `yan-py` | Python export and import (M1); CPython embedding and extensions (M2) | `pyo3` |
| `yan-pkg` | resolver, lockfile, registry client, log verification, approvals, semver, SBOM | `sigstore` |
| `yan-diag` | diagnostic records and the three renderings |  |
| `yan-ide` | analysis API (diagnostics, hover, inlay hints, lenses, watch expressions, view-as) shared by `yan-cli`, `yan-mcp` and `yan-lsp` (R560) |  |
| `yan-mcp`, `yan-lsp` | agent server, language server | `lsp-server` |
| `yan-cli` | the `yan` binary | `clap` |

**Layering (R558, R560).** Dependencies run one way: base crates (`yan-diag`, `yan-syntax`, `yan-names`), then the surface kit and the surface crates, then `yan-hir` and `yan-elab`, then `yan-core`, `yan-check` and `yan-prove`, then `yan-opt` and `yan-mir`, then the interpreter and code generators, with `yan-rt` and `yan-std` beside them, and the tools on top. A surface crate depends on nothing below `yan-hir`; a niche surface engine (R568) is a separate `yan-<name>-engine` crate at the runtime layer, beside `yan-rt`, and its items reach the HIR only as extern signatures (R576); and `yan-elab` depends on no surface crate; std sources are loaded by `yan-db`. The interpreter runs checked Yan Core; the code generators start from `yan-mir`.

Other dependencies: `opentelemetry` (spans), `gimli` (DWARF), LibAFL (fuzzing).

### 22.3 Front end and passes

- **Front end.** Hand-written lexers and recursive-descent parsers, with precedence climbing for expressions. Parsers MUST produce lossless syntax trees (every byte, including comments and whitespace, is recoverable), because the formatter, the lenient reader and patch application depend on them. Parse errors MUST recover at item boundaries, so one broken item never hides diagnostics in others.
- **Elaboration order** per item: resolve names, check the signature, infer and check the body bidirectionally, insert capability arguments, insert copies and moves by last-use analysis, and lower to Yan Core. Yan is then validated (every `let` typed, every argument's convention explicit) before any checking pass runs. A validator failure is an internal error (I-code), never a user diagnostic.
- **Check order** on Yan: exhaustiveness, then ownership, exclusivity and views, then auto traits, then bounds and refinements, then units and shapes, then contracts (at the selected level).
- **Optimization (R771, R773).** `yan-opt` runs monomorphization and comptime specialization, static handler resolution, the ownership passes of 5.3, then fusion and the rest. Every pass reads and writes valid Yan Core: the validator accepts it and `yan-check` accepts it in post-optimization mode (5.3 drop rules added, nothing else changed); a failing output is an internal error naming the pass. Optimized Yan Core prints with `yan view --as yir --opt` and runs with `yan run --opt`; `yan run` interprets checked Yan Core by default. Item identity stays on checked Yan Core (3.6): optimized items are cached by the checked item's hash, the instance key (R772) and the `yan-opt` version. `yan-mir` reads only optimized Yan Core and rejects a type parameter with an internal error. Native builds always run monomorphization, handler resolution and drop elaboration, at every optimization level.
- **Differential law (R771).** For every conformance program, the interpreter on checked Yan Core, the interpreter on optimized Yan Core and native code agree on output, failure and panic kind. The law harness (R564) runs the second leg in CI.

### 22.4 Linking and runtime support

- **Linking.** M1 uses the system linker. M2 bundles lld (`lld-link` on Windows with MinGW or UCRT import libraries) and target libc stubs, which make cross-compilation work without a platform toolchain. C sources compile with the toolchain's pinned clang (19.5).
- **Intrinsics (R561).** Each std intrinsic has one registry entry in `yan-std` (id, signature, stage, purity) and is resolved to its id during elaboration. The interpreter and every code generator bind the same id to the same implementation in `yan-rt`: the interpreter by a direct call, native code through a C-ABI symbol or an inline lowering. Nothing dispatches on intrinsic names at run time.
- **MIR boundary (R1048).** `yan-mir` is the last IR the compiler owns. Every decision with a semantic effect (instances, drops, layout, niches, the convention of each parameter, checks, panic kinds) is made at or above MIR and is visible in printed MIR; a code generator only translates it. LLVM is reached through one package, whose `ffi/` declares `lib llvm` (12.2); where LLVM's C API lacks a call, a C shim supplies it. Linking runs the bundled lld as a subprocess of the driver. `yan view --as mir F` prints that text (R1113).
- **No language runtime.** `yan-rt` holds std's support code only. Ordinary values carry no header; `Rc` and `Arc` keep their count in their block. Generated code has no landing pads, only table-only abort pads at extern call sites; the interpreter aborts the same way.

### 22.5 Testing strategy

- Snapshot tests for the parser, printer, elaborator and Yan output.
- Fuzzing of both parsers and of the patch applier, with the property "never panics; round-trip holds on accepted input".
- Differential testing: every conformance program runs on the interpreter and on each code generator, and outputs MUST match exactly. This also exercises the determinism guarantee.
- Property tests for the round-trip laws (16.5).
- Every unsafe block in std passes under `--ub`; concurrency primitives pass under `--explore`.

### 22.6 Working rules for the implementing agent

1. Follow the milestones in order. Do not start a milestone until the previous one's acceptance criteria pass.
2. Propose each feature's conformance tests (section 24) for the spec owner's approval before implementing it. Approved tests, conformance files and the spec are read-only to agents: CI rejects any agent patch that touches `tests/`, `conformance/` or `spec/` (CODEOWNERS plus a path guard). Agents keep their own non-gating tests in `src/**/selftest/` (SQ28).
3. Keep `SPEC_QUESTIONS.md` current; never resolve an ambiguity silently.
4. Keep the compiler's own diagnostics in the 18.2 format from the first commit, because the agent loop depends on them.

### 22.7 Self-hosting (RFC 0057)

**Stages (R967).** The Yan compiler is ported from stage 0 in six stages. Each passes its gate before the next starts. The corpus is every file under `conformance/`, `crates/yan-std/lib/` and `examples/`; Yan Core text is canonical, so middle-end gates are text diffs.

| Stage | Ports | Gate |
| --- | --- | --- |
| S0 readiness | nothing | native AOT runs a compiler-shaped program (read files, lex, build maps, print); every feature the compiler uses runs natively with no difftest allowlist entry; a callgrind baseline for lexing a large file |
| S1 front end | `yan-diag`, `yan-names`, `yan-surface`, the lexers, parsers and printers of Yan, Yan Agent and Yan Core text, the validator | stage-1 parse-print output and diagnostic codes equal stage 0's on the corpus |
| S2 middle end | the query engine and the compile-time evaluator (R1047), `yan-hir`, `yan-elab`, `yan-check`, `yan-flow`, `yan-opt` | printed Yan Core after elaboration and after `yan-opt` equals stage 0's on the corpus; every conformance fail case gives the same codes |
| S3 back end and runtime | `yan-mir`, the LLVM code generator, `yan-rt` | `tools.difftest` clean on the interpreter, stage-0 native and stage-1 native |
| S4 fixpoint | nothing | stage 0 builds stage 1, stage 1 builds stage 2, stage 2 builds stage 3; stage 2 and stage 3 are bit-identical and stage 2 passes the full conformance suite; `yan` ships from stage 2 and stage 0 freezes |
| S5 tools | `yan-cli`, `yan-ide`, `yan-mcp`, `yan-lsp`, `yan-pkg`, the rest of `yan-interp`, `yan-machine`, persistence for the query engine | each tool's results equal stage 0's; the interpreter stays the abstract machine of 6.1 |

S5 may overlap S4; 0.1.0 needs both. The frozen Rust tree and a committed `.yir` snapshot of stage 2 are the bootstrap seeds; the Rust tree is also the second compiler for diverse double compiling. After the freeze stage 0 takes only the fixes needed to keep building the seed.

**Entry gate (R1043).** S1 starts only when S0 passes and `tools.difftest` reports no native refusal.

**Readiness and subset (R1044).** S0 also requires each of the following with a `.yn` run case, native output equal to the interpreter's and no live allocation at exit: recursive sum types with moves out of a matched value; index-based trees; `Map` and `Set` interning; `Bytes`, `char`, `Path` and `Fs`; generic sums and `Result` propagation with an `inout` sink; traits, closures and module constants; a `lib` call from an `ffi` module; `par` joined in input order. Compiler sources use only the features listed in `selfhost/SUBSET.md`, which a lint checks; a feature is listed only with its run case. The subset excludes `Rc`, `Arc`, actors, channels, `Mutex`, atomics and `nondet`, and allows `unsafe`, `Ptr` and extern blocks only in the LLVM package's `ffi/` and in the runtime. Std is ported to `.yn` before S1.

**Freeze by layer (R1045).** A stage's comparison job is advisory until it has been green on 20 consecutive main commits, then blocking. From then on the stage-0 crates it covers take only fixes, each with the same fix in the Yan port in one PR, and new features for that layer are written in Yan only.

**Compiler structure (R1046).** The diagnostics catalogue, the names and projection tables, the intrinsic registry and the exported grammars are data files read by both compilers. Every pass is `uses none` apart from `alloc`; only the driver holds `fs`, `io`, `env` and `proc`. Diagnostics are sorted by file, span and code before rendering. The compiler is sequential until S4 passes; after it, `par` results are joined in input order.

**S2 contents (R1047).** S2 opens with a query engine (memoized pure queries keyed by item hash, with early cutoff) and the bytecode evaluator of 10.4. No `yan-elab` code is ported before both pass. Persistence, the remote cache and the rest of `yan-interp` are S5.

**Comparisons (R1051).** Gates compare bytes of text with stage 0 on the corpus: at S1 tokens, the lossless tree, printed source in each surface, the Yan Core text round trip and diagnostics; at S2 printed HIR, checked and optimized Yan Core, instance keys, item hashes, diagnostics and compile-time step counts; at S3 printed MIR, then program behaviour. Diagnostics are compared in full (code, span, arguments, rendered text). From S3 the stage-2 build MUST be bit-identical across two builds, across 1 thread and all cores, across checkout paths with different `TZ`, `LANG` and user, and across Linux and macOS hosts for one target.

**Prerequisites (R1052).** Before S1: moving a part out of an owned aggregate drops the rest, a value bound to `_` is dropped, and a CI job runs the compiler over the corpus with no live allocation at exit and a clean memory-checker run; every later gate requires that job. Before S3: `--ub` reports a use after free.

**Performance (R1054).** At S4, stage 2 uses at most 2.0 times stage 0's callgrind instruction count for `yan check` of the corpus and for building the compiler sources (time in LLVM excluded), and at most 2.0 times its peak memory; at RC the limits are 1.25, 1.25 and 1.5. No stage before S4 is gated on speed.

**Shipped languages (R966).** The shipped toolchain is Yan source except LLVM, called through its C API, a thin C shim over libc or LLVM where Yan's FFI cannot express a call, and the pinned clang and lld of 22.4.

**Engines (R1050).** A std engine behind an intrinsic id is Yan code or a pinned C library reached through `lib`. Until each is replaced, the engines built from Rust crates ship in one static library with a C ABI, `yan-rt-engines`, outside stage 0 and linked like any `lib`, with one tracked replacement per engine.

**Code generation (R968).** The Yan compiler generates code through LLVM's C API for debug (`-O0`) and release builds. Cranelift stays in stage 0 only; where this spec names Cranelift it means stage 0's backend. After 0.1.0 a debug backend written in Yan replaces it for fast debug builds and the JIT.

**One LLVM generator (R1049).** The LLVM code generator is written once, in Yan, and reads printed MIR. It may start when S0 passes. Until S3 it is stage 0's release backend, run as a subprocess, and its gate is the differential law (R771) with LLVM native code as a third leg; at S3 it is linked into the Yan compiler.

**Surface (R969).** The compiler is written in Yan (`.yn`). The Yan lexer, parser, printer and lowering to HIR are M2a work (23). Until the randomized comparison reports (after 0.1.0, R991), compiler sources avoid the contested forms it covers.

**Ordering (R970).** S2 starts only after formal phases 1 and 2 (22.1) land, and those start after the M1 gate. A stage-0 crate keeps growing until its layer freezes (R1045), the optimizer and the package manager included, and every stage-0 crate is ported.
