## 22. Implementation plan

This section is normative where it says MUST; elsewhere it is the recommended path. Deviations SHOULD be recorded in `SPEC_QUESTIONS.md` with the reason. The Yong Technical Implementation Spec (https://claude.ai/artifact/JqBVcrhH5heYx7F4BjkmYF) sits under this section and adds no language behavior.

### 22.1 Two implementations

Yong has one implementation, written in Rust with the crate layout below (R215, chosen 2026-10-31 in place of a Python reference followed by a port). M0 and M1 run on the interpreter, whose checked mode defines the abstract machine (6.1); Cranelift debug code generation follows in M1b. The conformance runner is implementation-independent and is the oracle: for every conformance and evaluation program, the interpreter and every code generator MUST produce the same result, diagnostic codes included. For Jian programs, the output of `yong export --python` run under CPython 3.12 is an independent second check. Performance targets apply to native code.

### 22.2 Crates

The compiler and tools MUST be written in Rust (stable toolchain) as one Cargo workspace:

| Crate | Responsibility | Key dependencies |
| --- | --- | --- |
| `yong-db` | query database, item hashing, file inputs, stage-0 query cache | `salsa`, `blake3` |
| `yong-syntax` | shared lexer infrastructure, lossless syntax trees, trivia | `rowan` |
| `yong-jian` | Jian lexer (INDENT/DEDENT), parser, lenient normalizer, canonical printer, exported grammars |  |
| `yong-wen` | Wen lexer, parser and printer (M3) |  |
| `yong-hir` | name resolution, surface-independent high-level IR |  |
| `yong-elab` | type inference (bidirectional, local inside items), desugaring, capability passing, copy and move insertion, lowering to Yong |  |
| `yong-core` | Yong IR data types, text parser and printer, IR validator |  |
| `yong-check` | ownership, exclusivity, views, auto traits, exhaustiveness, bounds solver, unit and shape checking |  |
| `yong-prove` | verification-condition generation and SMT-LIB2 subprocess client (M4) |  |
| `yong-opt` | monomorphization, comptime specialization, ownership passes, fusion, handler resolution, query-plan specialization |  |
| `yong-interp` | interpreter over checked Yong: stage 0 (bytecode), REPL, `yong run` in M1, checked UB mode, `explore` |  |
| `yong-cg-cranelift` | debug code generation (M1b) | `cranelift-*` |
| `yong-cg-llvm` | release code generation (M2) | `inkwell` |
| `yong-rt` | std support: alloc handlers, `pool` scheduler, blocking platform I/O, record and replay handlers, panic-abort hook | mimalloc, a heartbeat scheduler for pool (R481); corosensei, io-uring only in `std.evented` |
| `yong-std` | standard library sources (`.jian`) plus intrinsics | `arrow`, `regex-automata`, `rustls` with `aws-lc-rs`, `jiff`, ICU4X segmenter, a CORE-MATH-style libm |
| `yong-ffi` | `yong ffi import`, `yong bind`, the C profile | libclang (import only) |
| `yong-py` | Python export and import (M1); CPython embedding and extensions (M2) | `pyo3` |
| `yong-pkg` | resolver, lockfile, registry client, log verification, approvals, semver, SBOM | `sigstore` |
| `yong-diag` | diagnostic records and the three renderings |  |
| `yong-mcp`, `yong-lsp` | agent server, language server | `lsp-server` |
| `yong-cli` | the `yong` binary | `clap` |

Other dependencies: `opentelemetry` (spans), `gimli` (DWARF), LibAFL (fuzzing).

### 22.3 Front end and passes

- **Front end.** Hand-written lexers and recursive-descent parsers, with precedence climbing for expressions. Parsers MUST produce lossless syntax trees (every byte, including comments and whitespace, is recoverable), because the formatter, the lenient reader and patch application depend on them. Parse errors MUST recover at item boundaries, so one broken item never hides diagnostics in others.
- **Elaboration order** per item: resolve names, check the signature, infer and check the body bidirectionally, insert capability arguments, insert copies and moves by last-use analysis, and lower to Yong. Yong is then validated (every `let` typed, every argument's convention explicit) before any checking pass runs. A validator failure is an internal error (I-code), never a user diagnostic.
- **Check order** on Yong: exhaustiveness, then ownership, exclusivity and views, then auto traits, then bounds and refinements, then units and shapes, then contracts (at the selected level).
- **Optimization** runs the ownership passes of 5.3 after monomorphization and comptime specialization.

### 22.4 Linking and runtime support

- **Linking.** M1 uses the system linker. M2 bundles lld (`lld-link` on Windows with MinGW or UCRT import libraries) and target libc stubs, which make cross-compilation work without a platform toolchain. C sources compile with the toolchain's pinned clang (19.5).
- **No language runtime.** `yong-rt` holds std's support code only. Ordinary values carry no header; `Rc` and `Arc` keep their count in their block. Generated code has no landing pads, only table-only abort pads at extern call sites; the interpreter aborts the same way.

### 22.5 Testing strategy

- Snapshot tests for the parser, printer, elaborator and Yong output.
- Fuzzing of both parsers and of the patch applier, with the property "never panics; round-trip holds on accepted input".
- Differential testing: every conformance program runs on the interpreter and on each code generator, and outputs MUST match exactly. This also exercises the determinism guarantee.
- Property tests for the round-trip laws (16.5).
- Every unsafe block in std passes under `--ub`; concurrency primitives pass under `--explore`.

### 22.6 Working rules for the implementing agent

1. Follow the milestones in order. Do not start a milestone until the previous one's acceptance criteria pass.
2. Propose each feature's conformance tests (section 24) for the spec owner's approval before implementing it. Approved tests, conformance files and the spec are read-only to agents: CI rejects any agent patch that touches `tests/`, `conformance/` or `spec/` (CODEOWNERS plus a path guard). Agents keep their own non-gating tests in `src/**/selftest/` (SQ28).
3. Keep `SPEC_QUESTIONS.md` current; never resolve an ambiguity silently.
4. Keep the compiler's own diagnostics in the 18.2 format from the first commit, because the agent loop depends on them.
