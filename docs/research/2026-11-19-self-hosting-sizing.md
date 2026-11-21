# Self-hosting sizing: what remains before the compiler is written in Yan

Source: `zaofang/yan` at e558a19, 2026-11-19. Plan of record: RFC 0057, RFC 0082 (decided 2026-11-17), tracker #851. No spec rows, no codes. Nothing here is implemented.

Every size is an **estimate**. Line counts of Rust are measured (`*.rs` outside `tests/` dirs, so inline `#[cfg(test)]` is included and Rust figures are upper bounds). Yan figures and thread counts follow from the assumptions below.

## Assumptions

- A1: a Yan port is 0.7 times the Rust line count (no lifetimes, no borrow plumbing, but no macros either). Not measured; the first port slice should replace it.
- A2: one thread lands about 1,200 lines of port with its equality check. Basis: PR #1652 landed seven readiness programs (about 300 lines) and surfaced four blockers in one thread, so ports of existing semantics should be larger per thread than new-feature work, but blocker fixes are not counted in A2.
- A3: up to 3 parallel lanes on disjoint paths (project rule). A lane takes one thread at a time.
- A4: each stage's blockers are fixed in stage 0 by threads of their own; these are counted in the "S0 fixes" row, not in the stage's port threads.

## The two meanings of "when"

1. **First new compiler code written in Yan**: the LLVM code generator (RFC 0082 D8). It starts when S0 passes and MIR text exists (#1513). Earliest honest start.
2. **Compiler fully in Yan** (stage-2 fixpoint, S4 #856): the whole chain below, with the longest wait being S2 behind the formal phases.

No calendar date is given. The only observed rate is main on 2026-11-18 (53 merged commits in a day, shallow history in this clone) and PR numbers #1652 to #1733 in about a day and a half. Those are mixed small PRs and not port throughput; gates are serial, so volume does not convert to dates. Re-measure after the first S1 port slice lands.

## Stages

Rust sizes are `crates/*` lines. Yan size = A1. Threads = A2 (ports) or judgment (new work).

| Stage | Scope | Rust lines | Est. Yan lines | Est. threads | Tracked |
| --- | --- | --- | --- | --- | --- |
| S0 fixes | blockers found by readiness programs, ownership and capability soundness | n/a | n/a | 8 to 12 | #1506, #1500, #1502, #1732, #758, #1505 comments |
| S0 | Yan Core reader and printer in `.yn` over 55 `conformance/yir` files | yan-core 5,143 | 2,000 to 2,500 | 3 to 4 | #1505 |
| S0 | bootstrap subset file and lint | n/a | tool, about 300 | 1 | #1507 |
| S0 | LLVM-C binding spike from a Yan `ffi` module | n/a | about 300 | 1 to 2 | #1508 |
| S0 | std ported to `.yn`, one PR per tier | yan-std 616 plus std surface (not measured) | not measured | 4 to 6 | #1509 |
| S1 | diag, names, cst, ebnf, yag, yn parsers and printers, validator | about 24,000 | about 17,000 | 15 to 20 | #853, #1510, #1511 |
| S3a | MIR text (Rust, stage 0): printer, parser, validator | in yan-mir 17,149 | n/a (Rust, about 2,000) | 3 to 4 | #1513 |
| S3a | LLVM code generator in Yan over MIR text | new | 6,000 to 10,000 | 8 to 12 | #1514 |
| S2 | query engine and compile-time evaluator | new | 2,000 to 4,000 | 4 to 6 | #1512 |
| S2 | hir, elab, check, flow, opt | about 47,000 | about 33,000 | 30 to 40 | #854 |
| S3 | mir, runtime core, generator linked in process | mir 17,149, rt core part of 15,062 | 12,000 to 16,000 | 12 to 16 | #855 |
| S3 | runtime engines (regex, time, bigint, libm, segmentation, hashing, JSON, allocator) | in yan-rt | decided per engine, RFC 0082 D9 | 6 to 10 | #1515 |
| S4 | fixpoint, seeds, DDC, ceiling table, determinism jobs, perf gate | n/a | CI and tools | 5 to 8 | #856, #1516 |
| S5 | cli, ide, lsp, interpreter, machine, db, pkg (may overlap S4) | about 37,000 | about 26,000 | 25 to 30 | #857 |

Totals to S4, excluding S5: about 80,000 to 95,000 Yan lines, 95 to 130 threads. With S5: about 120 to 160 threads. Per-crate Rust sizes: elab 34,745, mir 17,149, interp 15,121, rt 15,062, cli 13,893, yn 9,356, yag 6,384, core 5,143, hir 4,806, machine 4,072, cranelift 3,984 (replaced, not ported), check 3,757, opt 3,062, db 2,251.

## Remaining blockers per stage

**Reader (S0, #1505).** Programs for 7 readiness items landed in #1652. Open:
- the reader itself: full `.yir` parser plus a port of the `yan-core` printer; stage 0 reflows blocks, so 0 of 55 files print identically to source and the port must reproduce the reflow (#1505 comment 2026-11-18).
- `fs.list_dir` is rejected as unsupported on this toolchain though spec 17 lists it (filed as #1737).
- `u8 as int` rejected, `b as i64` works: open question in the #1505 thread whether `u8 as int` is accepted (spec 03 says `x as T` is checked) or the reader uses `i64` indices. Antonio's call, already raised there.
- native `Set` and generic sum matched in a generic function: #1653 closed; recheck on current main pending.
- ownership blockers: #1500 (use after free and leaks, `examples/real/bst.yag` leaked 42 blocks) and #1502 (capability and FFI ceiling gaps), both under #1506; #1732 (native crash failing a record into a union).
- S1 entry gate (RFC 0082 D1): S0 passes and difftest shows zero native refusals; #758 is open (98 of 234 refused on 2026-11-12, figure stale), gate issue #1026.

**Parser and AST (S1).** Depends on the S0 gate. #1510 (shared tables as data) and #1511 (equality harness) can start earlier; #1510 is Rust-only and startable now. Risk: the lossless tree as an index-based pool (RFC 0082 D5) is proven only on small programs.

**Type checker and middle end (S2).** Longest wait. Needs S1, the query engine and compile-time evaluator (#1512), and formal phases 1 and 2, which wait on the M1 gate #411 and Lean work #425 (phases 1 to 4). Nothing sizes how long phases 1 and 2 take. This chain is the longest dependency on the path to S4 (RFC 0082 stage breakdown). `yan-elab` is 34,745 lines and ports module by module, not as one file.

**MIR (S3).** MIR text (#1513) is startable now and gates the code generator; MIR port itself waits on S2.

**Codegen (S3a, S3).** Startable after S0 plus #1513. Blocker: the LLVM-C binding spike #1508 and a Yan `ffi` module that can emit and link an object. Third-leg differential (interpreter, Cranelift, LLVM) needs the LLVM leg first.

**Driver and tools (S4, S5).** S4 needs seeds, DDC and the ceiling table (#1516). S5 (#857) holds cli, interpreter, machine and package manager; the interpreter is also needed after S4 to verify the committed snapshot (RFC 0082 D12).

## Not tracked, filed under #449

- #1737: `fs.list_dir` missing (blocks the reader's directory walk).
- #1738: per-crate port inventory and the 22.2 crate table: RFC 0082 leaves the table and which of yan-machine, yan-flow, yan-cst, yan-ebnf, yan-sandbox, yan-playground and yan-db are ported to a follow-up; the stage issues do not list crates.
- #1739: scale probe. No evidence yet that a 20,000 to 90,000 line `.yn` program builds natively in acceptable time and memory, or that the stage-0 interpreter can run it (needed by the committed-snapshot check). S0 programs are about 300 lines.

Not filed: the length of formal phases 1 and 2. The RFC already decided S2 waits for them; sizing needs the Lean owner, so it is flagged here and left to #425.

## Critical path (ordered)

1. S0 fixes: #1500 findings, `fs.list_dir`, `u8` decision, native `Set` and generic sum recheck, #1732.
2. #1505 reader and the `#1506`, `#1507` items; native refusals to zero (#758, #1026). S0 gate passes.
3. In parallel from step 1, Rust-only: #1513 MIR text, #1510 shared tables, #1508 spike.
4. S1 ports (3 lanes, about 18 threads, about 6 serial steps), with #1511 harness first.
5. S3a LLVM generator on its own lane from the S0 gate (not on the critical path unless S2 finishes first).
6. M1 gate #411, formal phases 1 and 2 (#425). Unsized; runs from now in parallel and decides whether S2 waits.
7. S2: query engine and evaluator, then hir, elab, check, flow, opt (about 14 serial steps at 3 lanes).
8. S3: mir, runtime core, generator linked in process.
9. S4: fixpoint, DDC, ceiling table, perf gate (stage 2 within 2.0 times stage 0, RFC 0082 D13).

Estimated serial depth, assuming A3, excluding step 6 wait: about 40 thread-steps. Step 6 is the swing factor.

## Risks that change the estimate

- A1 and A2 are unmeasured. Re-baseline after the first S1 port slice; a 2x error in A1 moves totals proportionally.
- Dual maintenance: until each layer's freeze (RFC 0082 D4), every feature to a ported layer lands twice. Feature growth in stage 0 during S1 and S2 adds port work not counted here.
- New native blockers appear at about 4 per readiness batch (#1505 found 4 in the first batch). Expect similar per stage.
