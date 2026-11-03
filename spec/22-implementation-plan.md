## 22. Implementation plan

This section is normative where it says MUST; elsewhere it is the recommended path. Deviations SHOULD be recorded in `SPEC_QUESTIONS.md` with the reason. The Yan Technical Implementation Spec (https://claude.ai/artifact/JqBVcrhH5heYx7F4BjkmYF) sits under this section and adds no language behavior.

### 22.1 Two implementations

Yan Core has one implementation, written in Rust with the crate layout below (R215, chosen 2026-10-31 in place of a Python reference followed by a port). M0 and M1 run on the interpreter, whose checked mode defines the abstract machine (6.1); Cranelift debug code generation follows in M1b. The conformance runner is implementation-independent and is the oracle: for every conformance and evaluation program, the interpreter and every code generator MUST produce the same result, diagnostic codes included. For Yan Agent programs, the output of `yan export --python` run under CPython 3.12 is an independent second check. Performance targets apply to native code.

**Formal core model (R680).** `formal/` holds a Lean 4 model of Yan-K, the fragment of Yan Core text named in RFC 0022: a reader, an executable semantics, and from phase 1 on the proofs of the formal core study. It is the one exception to R215: it never ships, no crate depends on it, and building the toolchain never needs Lean. `formal/drt.py` runs the `conformance/yir/run` cases and generated Yan-K programs on both the interpreter and the model and compares output, exit status and panic kind; a difference is a bug in one of them or a gap in this spec. Proofs start after the M1 gate.

### 22.2 Crates

The compiler and tools MUST be written in Rust (stable toolchain) as one Cargo workspace:

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
| `yan-interp` | interpreter over checked Yan Core: stage 0 (bytecode), REPL, `yan run` in M1, checked UB mode, `explore` |  |
| `yan-cg-cranelift` | debug code generation (M1b) | `cranelift-*` |
| `yan-cg-llvm` | release code generation (M2) | `inkwell` |
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
