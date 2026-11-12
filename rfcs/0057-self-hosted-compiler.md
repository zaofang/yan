# 0057: Self-hosted compiler; Rust as the frozen stage 0

- Status: accepted
- Research: Self-hosted Yan compiler and formal-proof plan (2026-11-09, read against 82f6a33); [Bootstrapping roadmap](../docs/studies/bootstrapping.md) (2026-11-04). Tracker #851, stages #852 to #857, this RFC #858.
- Rows: R966 to R970

## Summary

R215 says Yan has one implementation, written in Rust, and 22.2 says the compiler and tools MUST be
Rust. The lead decided on 2026-11-09 that 1.0 ships a toolchain written in Yan. This RFC replaces R215:
the Rust workspace becomes stage 0, the bootstrap compiler and the oracle, and is frozen once the Yan
compiler reaches a fixpoint. The port runs in six gated stages (S0 to S5), each of which must reproduce
stage 0's output byte for byte before the next starts. It also records the lead's four answers on the
plan: LLVM through its C API is the 1.0 code generator, with a Yan-written backend replacing Cranelift
after 1.0; the compiler is written in Yan (`.yn`), which pulls the Yan front end forward from M3;
formal phases keep waiting for the M1 gate and S2 waits for phases 1 and 2; the optimizer (#760) and
package manager (#764) continue in Rust and are ported in S2 and S5.

The 2026-11-04 study recommended keeping R215 for 1.0 (its D1). That recommendation is superseded; its
D9 (retire the Rust tree except a Cranelift shim) is superseded by D1 and D3 below. Its D7 (two seeds:
the Rust tree and a committed `.yir` snapshot) stands.

## Decisions

### D1: implementation language

The 1.0 toolchain (compiler, interpreter, runtime and tools) is written in Yan. The Rust workspace of
22.2 is stage 0: it bootstraps stage 1, serves as the oracle every stage is compared against, and is
frozen at the commit where S4 passes. After the freeze it takes no features and only those fixes needed
to keep building the seed. Non-Yan code allowed in the shipped toolchain: LLVM, called through its C
API; a thin C shim over libc where Yan's FFI cannot express a call; the pinned clang and lld of 22.4.
The Lean model under `formal/` (R680) stays outside the toolchain and outside this rule.

Options weighed: keep R215 for 1.0 and self-host after RC (the study's D1); self-host for 1.0
(decided). The interim relaxation of 2026-11-01 (non-Rust parts where they buy speed) is absorbed: it
applied to stage 0 and still does until the freeze.

- Decision: self-host for 1.0, Rust as frozen stage 0 (lead, 2026-11-09)
- Row: R966

### D2: stages and gates

Each stage ports one layer of 22.2 and passes its gate before the next starts. The corpus is every
file under `conformance/`, `crates/yan-std/lib/` and `examples/`. Yan Core text is canonical, so every
middle-end gate is a text diff.

| Stage | Ports | Gate |
| --- | --- | --- |
| S0 readiness | nothing | native AOT runs a compiler-shaped program (read files, lex, build maps, print); #758 closed for the features the compiler uses; the difftest allowlist is empty for them; a callgrind baseline for lexing a large file |
| S1 front end | `yan-diag`, `yan-names`, `yan-surface`, the lexers, parsers and printers of Yan, Yan Agent and Yan Core text, the validator | on the corpus, stage-1 parse-print output and diagnostic codes equal stage 0's |
| S2 middle end | `yan-hir`, `yan-elab`, `yan-check`, `yan-flow`, `yan-opt` | on the corpus, printed Yan Core after elaboration and after `yan-opt` equals stage 0's; every conformance fail case gives the same codes |
| S3 back end and runtime | `yan-mir`, the LLVM code generator (D3), `yan-rt` | `tools.difftest` clean with three backends: interpreter, stage-0 native, stage-1 native |
| S4 fixpoint | none | stage 0 builds stage 1, stage 1 builds stage 2, stage 2 builds stage 3; stage 2 and stage 3 binaries are bit-identical and stage 2 passes the full conformance suite; `yan` ships from stage 2 and stage 0 freezes |
| S5 tools | `yan-cli`, `yan-ide`, `yan-mcp`, `yan-lsp`, `yan-pkg`, `yan-interp`, `yan-db` | each tool's conformance and selftest results equal stage 0's; the interpreter keeps its role as the abstract machine of 6.1 |

S5 may overlap S4; 1.0 needs both. Diverse double compiling uses the frozen Rust tree as the second
compiler (study D7).

- Decision: as the plan states (lead, 2026-11-09)
- Row: R967

### D3: code generation route

Cranelift has no C API, so the Yan compiler cannot call it. For 1.0 the Yan compiler generates code
through LLVM's C API, for debug builds (`-O0`) and release builds alike. Cranelift stays in stage 0
only. After 1.0 a debug backend written in Yan replaces it for fast debug builds and the JIT. Options
weighed: a C-ABI shim over Cranelift (the study's D6 and D9); a Yan backend before 1.0; LLVM alone for
1.0 (decided).

- Decision: LLVM C API for 1.0, Yan-written backend after 1.0 (lead, 2026-11-09, #855)
- Row: R968

### D4: implementation surface

The compiler is written in Yan (`.yn`), not Yan Agent. The Yan lexer, parser, printer and the Yan
lowering to HIR move from M3 to M2a, ahead of S1. Projections, the Yan Meta view and the rest of M3
stay where they are. Until the M3 randomized comparison reports, compiler sources avoid the contested
forms it covers, so its result never forces a rewrite of the compiler.

- Decision: Yan (`.yn`), `yan-yn` pulled forward (lead, 2026-11-09)
- Row: R969

### D5: ordering against proofs and the second pass

Formal phases 1 to 4 (#425) keep waiting for the M1 gate (#411). S2 starts only after phases 1 and 2
land, so the checker rules the port copies are the proved ones. Second-pass items that make stage 0 a
better oracle (#754, #756, #758, #762) come before S1. The optimizer work (#760) and the package
manager crate (#764) continue in Rust and are ported in S2 and S5. Stage 0 keeps growing until the
S4 freeze; every stage-0 crate is ported.

- Decision: as stated (lead, 2026-11-09)
- Row: R970

## Spec changes

- 1: the pipeline diagram names the 1.0 code generators.
- 18.1: `yan run` and `yan build` name LLVM as the shipped backend; Cranelift is stage 0's.
- 22.1: rewritten for R966.
- 22.2: the Rust requirement applies to stage 0.
- 22.7 (new): self-hosting, R966 to R970.
- 23: M2a takes the Yan front end and S0; M3 takes S1 to S3; RC takes S4 and S5.
- 26.1: the implementation-language row.
