# 0082: Self-hosted compiler design: subset, boundaries, trust and gates

- Status: accepted (lead, 2026-11-17: recommendation taken on every item)
- Research: this document (design, read against main `bbedc4c`, 2026-11-17); RFC 0057 (accepted,
  stages S0 to S5); [Bootstrapping roadmap](../docs/studies/bootstrapping.md) (2026-11-04); plan of
  2026-11-09 (#851); audits #1500 (native ownership) and #1502 (capability soundness). Tracker #851.
- Rows: R1043 to R1054

## Summary

RFC 0057 decided that 0.1.0 ships a toolchain written in Yan, with the Rust workspace as the frozen
stage 0, six gated stages and LLVM through its C API. It does not say what the compiler may be written
with, where the Yan-owned pipeline ends and LLVM begins, how the two implementations are kept equal
while both change, what the bootstrap trusts, or how fast the result must be. This RFC fills those in.
It changes four things in RFC 0057 and section 22.7, each a separate decision below: an entry gate for
S1 (already decided on #1026), a progressive freeze of ported crates (D4), the query engine and the
compile-time evaluator moving from S5 to S2 (D6), and the LLVM code generator being written once, in
Yan, over a canonical MIR text, startable after S0 (D7, D8). Everything else refines the accepted plan.

Each decision gives the long-term recommendation first and interim steps separately.

Sizes, from `crates/` at `bbedc4c` (Rust lines, tests included): S1 crates about 23 thousand
(`yan-yn` 9.3, `yan-yag` 6.1, `yan-core` 4.9, `yan-diag` 1.1, `yan-ebnf` 0.1.0, `yan-cst`, `yan-names`,
`yan-surface`); S2 about 46 thousand (`yan-elab` 32.8, `yan-hir` 4.8, `yan-check` 3.1, `yan-opt` 2.9,
`yan-db` 1.5, `yan-flow` 0.6); S3 about 34 thousand (`yan-mir` 15.7, `yan-rt` 14.4, `yan-cg-cranelift`
4.2 as the size guide for the LLVM generator); S5 about 34 thousand (`yan-interp` 15.0, `yan-cli`
13.7, `yan-machine` 3.8, `yan-ide` 1.4). About 137 thousand in all.

## Decisions

### D1: entry gate for S1

S1 starts only when S0 passes and `tools.difftest` reports zero native refusals (#758; 98 of 234
programs on `61e6a66`, 2026-11-12). Decided by the lead on 2026-11-12 (#1026); the RFC was deferred by
the spec freeze, which CASE WITH ROW has since replaced. This item only writes the decision into 22.7.

- Decision: S1 entry gate as decided 2026-11-12 (#1026), written (lead, 2026-11-17)
- Row: R1043

### D2: S0 is widened to cover what a compiler does

The S0 program (`selfhost/s0/lex.yag`, 66 lines) exercises file reads, lists of records, dict updates
and string iteration. A compiler also needs the features below, and none is proven on native yet. S0
passes when each has a `.yn` run case, runs natively with interpreter-equal output, zero live
allocations at exit and no difftest allowlist entry:

1. Recursive sum types and exhaustive `match` that moves fields out of a by-value scrutinee (#1500
   finding 3 leaks here today; `examples/real/bst.yag` leaks 42 blocks).
2. Index-based trees: `Pool` and `Handle` (or a list plus a `u32` newtype) with generic functions over
   them; a lossless syntax tree is stored this way (D5).
3. `Map` and `Set` with string and newtype keys, insertion-ordered iteration, interning tables.
4. `Bytes` and byte views for lexing (#1041), `char` classification, string building, `Path` and `Fs`
   directory walks (#1044).
5. Generic sum types and `Result` propagation through several layers; a diagnostics sink passed as
   `inout`.
6. Traits with generic dispatch, closures passed to higher-order functions, module constants.
7. An `ffi` module calling a C library through `lib` (spike: about 20 LLVM-C functions, emit and link
   an object for one function; see D7).
8. `par` over a list with results joined in input order (used after the fixpoint only, D5).

The second S0 program is a `.yn` parser and printer for Yan Core text over the `conformance/yir`
corpus (55 files): it is compiler-shaped, has an exact oracle in stage 0, and is reused by S1.

Long term: this list is the S0 gate. Interim: `lex.yag` stays as the smoke test.

- Decision: as recommended (lead, 2026-11-17)
- Row: R1044

### D3: the bootstrap subset

Compiler sources are written in a fixed subset of Yan (`.yn`), recorded in `selfhost/SUBSET.md` and
enforced by a lint in CI. A feature enters the subset only when it has a run case on `conformance/yn`,
runs on native without a refusal, and is clean under the `native-live` job (D11). The subset:

- In: records, sums, newtypes, generics, traits, closures, `match`, `Result` and `fail`, `inout`,
  views, `List`, `Map`, `Set`, `Str`, `Bytes`, `Pool` and `Handle`, `Arena` and `Ref` once native
  supports them, module constants, `par` with ordered joins.
- Out everywhere: `Rc` and `Arc` in compiler data (trees are index-based, so the Send and Sync gaps of
  #1502 items 4 and 9 cannot reach the compiler), actors, channels, `Mutex`, atomics, `nondet`, floats
  outside constant folding of user programs, reflection beyond the std derives, the contested forms of
  R969, and anything `tools.difftest` allowlists.
- Out except in named modules: `unsafe`, `Ptr` and extern blocks, allowed only in the LLVM binding
  package's `ffi/` directory and in the runtime (D9).

The subset is a ratchet in one direction during the port: it grows by a PR that adds the case and the
lint entry together, and never because a port needed a feature that is not yet proven.

Until the std port to `.yn` (#1027, decided 2026-11-12) is done, the compiler may import `.yag` std
modules; the std port is ordered before S1 so the compiler and std share one surface from the start.

- Decision: as recommended (lead, 2026-11-17)
- Row: R1044

### D4: port order and the dual-maintenance window

From the first ported crate to the S4 freeze every change to a ported layer must be made twice. RFC
0057 D5 lets stage 0 keep growing until the freeze, which makes that window as wide as possible.

Recommended, long term: a progressive freeze. When a stage's equality gate has been green on 20
consecutive main commits it becomes blocking (study D8), and from then on the Rust crates it covers
take only fixes, each landing with the same fix in the Yan port in one PR. New features for a frozen
layer are written in Yan only and reach stage 0 not at all. Stage 0 stays the oracle for the corpus as
it stood at each layer's freeze; after that the conformance suite is the oracle (D10).

Options weighed: (a) stage 0 grows until S4 (RFC 0057 D5), simplest to state, doubles every feature
PR for months; (b) progressive freeze per stage (recommended); (c) feature freeze of all of stage 0
at S1 start, shortest window but stops language work until S4.

Port order inside a stage is bottom-up along the 22.2 layering, one crate per PR series, each with its
own equality check so a red gate names one crate. The `yan-elab` port follows the module split that
practice 5 already requires (`lower.rs` is not ported as one file).

- Decision: option (b), progressive freeze (lead, 2026-11-17)
- Row: R1045

### D5: shape of the compiler in Yan

One fact, one representation (practice 12) decides most of this.

- **Shared tables are data, not ported code.** The diagnostics catalogue, the names and projection
  tables, the intrinsic registry and the exported grammars are single files read by both compilers
  (generated into Rust by `build.rs`, read at stage 0 of compile time or embedded by the Yan compiler).
  Diagnostic text parity then follows from code parity; no second copy of a message exists.
- **Trees.** Syntax trees are lossless and index-based: one token arena and one node arena per file,
  `u32` handles, trivia in a side table (as R560). HIR, Yan Core and MIR use recursive sums for
  expressions and index tables for items. No `Rc`.
- **Names and spans.** Identifiers are interned to `u32`; a span is a file id and two `u32` byte
  offsets, computed once in the lexer and never recomputed.
- **Purity.** Every pass is a function with `uses none` (plus `alloc`). Only the driver holds `fs`,
  `io`, `env` and `proc`. The capability checker therefore proves that no pass reads a clock, the
  environment, a random source or the file system, which is the determinism argument of D10 made by
  the language itself.
- **Diagnostics.** Passes append to a diagnostics sink; the driver sorts by file, span and code before
  rendering. An internal error is an I-code panic that names the pass (22.3); an unsupported construct
  is a diagnostic, never a fallback (practice 13).
- **Concurrency.** Sequential until S4 passes. After the fixpoint, `par` over files (front end) and
  over items and instances (middle and back end), with results joined in input order, so output is
  independent of thread count (19.5). No actors or channels in the compiler.

- Decision: as recommended (lead, 2026-11-17)
- Row: R1046

### D6: query engine and compile-time evaluator move to S2

RFC 0057 ports `yan-db` and `yan-interp` in S5. Two things need them earlier:

1. `yan-opt` runs compile-time specialization, and the S2 gate compares printed Yan Core after
   `yan-opt` on a corpus that uses `comptime`. The Yan compiler needs a compile-time evaluator at S2,
   and stage 1 cannot borrow stage 0's.
2. Elaboration, checking and optimization in stage 0 are written against queries keyed by item hash.
   Porting them as batch code and rewriting them for queries in S5 is a second representation of the
   pipeline, and the lead's answer to study D5 was a Yan query engine at the middle-end stage.

Recommended, long term: S2 opens with two pieces, before any `yan-elab` code is ported: a query engine
in Yan (inputs, memoized pure queries keyed by item hash, early cutoff; no persistence yet), and the
bytecode evaluator for checked Yan Core with the step counting of 10.4. S5 keeps the rest of
`yan-interp` (REPL, `--ub`, `explore`, `yan run`) and adds persistence and the remote cache to the
query engine. S1 stays batch: its passes are per-file pure functions and need no engine.

Interim: none; a batch S2 would be thrown away.

- Decision: as recommended (lead, 2026-11-17)
- Row: R1047

### D7: the boundary between the Yan pipeline and LLVM

`yan-mir` is the last IR Yan owns. It is already specified as fully lowered: per-instance control-flow
graphs, layouts, drop glue, borrow lowering, decision trees and intrinsic ids (R773). The rule:

- Every decision with a semantic effect (monomorphization, ownership passes, drop placement, layout,
  niche choice, calling convention per parameter, overflow and bounds checks, panic kinds) is made at
  or above MIR, in Yan code, and is visible in printed MIR.
- The LLVM code generator is a translation of MIR to LLVM IR and nothing else. It makes no choice a
  second backend would have to copy. LLVM runs a pinned pass pipeline per optimization level.
- MIR gets a canonical text form with a parser, a printer and a validator, as Yan Core has. Stage 0
  prints it (`yan view --as mir`, `yan build --emit mir`). It does not exist today.
- LLVM is reached through one package, the only compiler package with an `ffi/` directory. It declares
  `lib llvm` (12.2), so LLVM is a capability named in the package ceiling and in `allow_ffi`. The
  binding is a typed layer over the LLVM-C functions the generator uses (about 150); it is hand-written
  at first and regenerated by `yan ffi import` once `yan-ffi` exists. Where LLVM-C lacks a call, a C
  shim compiled by the pinned clang supplies it; R966's shim wording is widened from libc to LLVM.
- Linking runs the bundled lld as a subprocess of the driver. Objects are written to files; the
  compiler passes hold no `proc`.

Options weighed for the boundary: (a) the LLVM-C builder API in process (recommended, and what R968
says); (b) print textual LLVM IR and run the pinned `clang`, which needs no FFI but ties the compiler
to an unstable text format and adds a parse per module; (c) write bitcode directly, which is a large
format to own. Option (b) is not taken even as an interim step, since it would be a second emitter.

- Decision: as recommended (lead, 2026-11-17)
- Row: R1048

### D8: one LLVM code generator, written in Yan, startable after S0

Section 22.2 lists `yan-cg-llvm` as a Rust crate over `inkwell` (M2); it has not been started. Building
it in Rust and again in Yan for S3 writes the largest backend twice.

Recommended, long term: the LLVM code generator is written once, in Yan, as a program that reads MIR
text (D7) and writes objects. Until S3 it is stage 0's release backend, run as a subprocess by
`yan build --release`; at S3 the same package is linked into the Yan compiler and takes MIR in memory.
The `inkwell` crate is dropped from 22.2. This changes RFC 0057's "each stage passes its gate before
the next starts" for this one work package: it starts when S0 passes, in parallel with S1, on paths
disjoint from the front end. Its gate until S3 is the differential law with a third leg (interpreter,
Cranelift native, LLVM native agree on the conformance run cases).

What this buys: the LLVM FFI risk is met first instead of last; the M2 benchmark gate (R389) keeps its
date because release code generation no longer waits for S3; the code generator is the first large
Yan program and tests the subset (D3) before the front-end port depends on it.

Options weighed: (a) as RFC 0057, LLVM generator in S3 only, with the M2 benchmark gate either moved
after S3 or served by a throwaway Rust backend; (b) Yan generator over MIR text after S0
(recommended); (c) Rust `inkwell` backend now and a Yan port at S3.

Interim under (b): S0 item 7 (D2) is the spike that proves the binding approach before the package
starts.

- Decision: option (b), Yan generator over MIR text after S0 (lead, 2026-11-17)
- Row: R1049

### D9: runtime and third-party engines

R966 allows only LLVM, a C shim and the pinned clang and lld as non-Yan code in the shipped toolchain.
`yan-rt` today builds on Rust crates that implement std: `regex`, `jiff`, `num-bigint`, `core-math`
and `libm`, `unicode-segmentation`, `blake3`, `serde_json`, `getrandom`; 22.2 adds mimalloc, `rustls`,
`arrow` and ICU4X. A frozen Rust tree cannot keep supplying them, and RFC 0057 does not say what does.

Recommended, long term: every engine behind an intrinsic id is Yan code or a pinned C library reached
through `lib`, decided per engine: Yan for those that define observable semantics the spec fixes (big
integers, regex matching, text segmentation, JSON, the libm subset), a pinned C library where an
audited implementation matters more than the language (TLS, the allocator, BLAKE3).

Interim for 0.1.0: the engines not yet replaced stay in one Rust-built static library with a C ABI,
`yan-rt-engines`, outside the frozen stage 0, linked like any `lib`. R966 names it as a dated
exception with one issue per engine. The runtime core (allocation handlers, the `pool` scheduler,
blocking I/O, panic and abort, record and replay) is ported to Yan in S3 as planned; it is the only
compiler-adjacent code outside the LLVM binding that may use `unsafe`.

Options weighed: (a) port every engine before 0.1.0 (months of work off the critical path, and a new
regex or TLS engine is a security risk of its own); (b) the engines library as a dated exception
(recommended interim); (c) C libraries for all of them.

- Decision: long-term rule with option (b) as the 0.1.0 interim (lead, 2026-11-17)
- Row: R1050

### D10: equality, determinism and conformance

**Per-stage equality (refines the gates of R967).** All comparisons are byte comparisons of text.

| Stage | Compared with stage 0 on the corpus |
| --- | --- |
| S1 | tokens, lossless tree dump, printed source in each surface, Yan Core text round trip, diagnostics as JSON (code, span, arguments) |
| S2 | printed HIR, checked Yan Core, optimized Yan Core, instance keys, item hashes, diagnostics as JSON, compile-time step counts |
| S3 | printed MIR; then program behaviour by `tools.difftest` on interpreter, stage-0 native and stage-1 native |
| S4 | stage 2 and stage 3 bit-identical; stage 2 passes the whole conformance suite |

Diagnostics are compared in full (span, arguments, rendered text) from S1 on, not codes only; D5's
shared catalogue makes the text free. The MIR text comparison is new and turns S3 from a behavioural
gate into a text gate plus a behavioural one.

**Harness.** `tools/selfhost.py` builds stage N, runs one stage's comparison and prints the first
differing file per crate. The conformance runner takes the compiler binary as an argument, so the same
suite runs against stage 0, 1 and 2. CI runs the comparisons on every PR; a stage's job is advisory
until it has been green on 20 consecutive main commits, then blocking (study D8).

**Determinism checks**, each a CI job from S3 and a release check from S4:

1. Double build: `yan build --verify` on the compiler itself.
2. Thread count: the stage-2 build with 1 thread and with all cores is bit-identical.
3. Location and environment: two checkouts at different paths, different `TZ`, `LANG` and user, are
   bit-identical (remapped paths, 19.5).
4. Host: stage 2 for one target cross-built on Linux and on macOS is bit-identical (nightly; interim:
   Linux only until a macOS runner exists).
5. No source of nondeterminism is reachable: the compiler packages' computed capability sets contain
   no `nondet`, clock, random or `net` capability (D12).

LLVM is the one component Yan cannot check this way: its version and source hash are pinned, code
generation runs one module per codegen unit with a partition that does not depend on thread count,
and checks 1 to 4 cover its output.

**After the freeze.** Stage 0 no longer defines new behaviour. The oracles are the conformance suite
(owned by the lead, independent of both compilers), the differential law across the Yan interpreter
and native code, and the Lean model for Yan-K. Every bug found by an equality gate during the port is
filed as a proposed `conform/*` case (practice 2), so the suite ends the port stronger than the Rust
tree's selftests.

- Decision: as recommended (lead, 2026-11-17)
- Row: R1051

### D11: ownership and soundness prerequisites

The compiler is the largest Yan program and the first long-running one; it is exposed to exactly the
bug classes of #1500 and #1502.

From #1500 (native ownership):
- Finding 1 (use after free through unchecked moves) is fixed by #1501. Its generated ownership grid
  (matrix item 4) is an S0 requirement, because the compiler uses every consuming position in it.
- Finding 3 (moving one field out leaks the rest) and finding 4 (`_` binding never dropped) are fixed
  before S1. Compiler code is mostly `match` on by-value sums.
- Matrix item 1, the `native-live` CI job, covers the compiler: every stage's gate requires zero live
  allocations at exit and a clean valgrind or ASAN run of the compiler over the corpus. Finding 5
  (locals live when `main` fails) is fixed so the check also holds on failing runs.
- Finding 2 (`--ub` machine panics instead of reporting) is fixed before S3, when the runtime and the
  LLVM binding bring `unsafe` into the compiler; from S3 the compiler runs under `--ub` nightly.

From #1502 (capability soundness), the items that D5's purity argument and D12's ceiling rest on are
S0 requirements: capability laundering through arguments and closures (items 1 and 6, fixed), `nondet`
captured in a closure and run in a `uses none` callee (item 11), E709 for a `Mutex` inside a record
(item 10), a `lib` named like a built-in capability (item 14), and `ffi` modules in `.yag` not counted
(item 18). The Send and Sync items (4, 5, 7, 9) do not gate the port, because the subset bans `Rc`,
`Arc`, channels and actors (D3); they gate the post-fixpoint `par` work of D5.

- Decision: as recommended (lead, 2026-11-17)
- Row: R1052

### D12: trust and security of the bootstrap

**Seeds.** As RFC 0057: the frozen Rust tree and a committed Yan Core snapshot of stage 2. This RFC
adds how they are kept honest.

- The Rust seed is pinned to a commit and a `rustc` version, vendored, and built in CI on every
  release, forever. It is the only fully independent compiler Yan will have.
- The snapshot is generated by CI from stage 2, never edited by hand (the path guard rejects any other
  author), carries the item hashes, and is verified in the same job: the frozen stage-0 interpreter
  runs the snapshot to compile the compiler sources, and the result must equal stage 2 bit for bit.
- Diverse double compiling on every release: compiler sources built by the Rust seed and by the
  previous Yan release; each result builds the sources again; the two second-generation binaries must
  be bit-identical. A difference blocks the release.

**Capability ceiling of the toolchain.** Each toolchain package declares its ceiling in its manifest:
the compiler passes none; the driver `fs`, `io`, `env`, `proc`; the LLVM package `llvm`; the package
manager adds `net`. A committed table holds the computed capability set per package, and a release
check fails when any set grows (the lead's answer to study D11). `proc` in the driver is limited to
the bundled lld and clang by path.

**What stays trusted.** The pinned `rustc` (for the seed), LLVM, clang and lld at pinned source
hashes, the engines of D9, the host kernel and libc. The conformance expectations are owned by the
lead and reviewed independently of both compilers, which bounds what a compromised compiler could
hide. Release binaries keep the independent-rebuild requirement of 19.6; the fixpoint, DDC and ceiling
checks join it.

- Decision: as recommended (lead, 2026-11-17)
- Row: R1053

### D13: performance targets

There is no compiler-speed target in the spec. Figures are callgrind instruction counts against the
committed baseline (practice 6), each with its commit and date. Proposed:

| When | Subject | Target |
| --- | --- | --- |
| S1 to S3 | stage 1 (built by Cranelift) | recorded per stage, not gated |
| S4 | stage 2, `yan check` of the corpus | at most 2.0 times stage 0's instruction count |
| S4 | stage 2 building itself (front and middle end, LLVM excluded) | at most 2.0 times stage 0 building the same sources |
| S4 | peak memory, same runs | at most 2.0 times stage 0 |
| RC | the same three | at most 1.25, 1.25 and 1.5 times |

Stage 0 is compiled by `rustc` with LLVM and uses salsa and rowan; reaching 1.25 times means the Yan
optimizer and LLVM pipeline are doing their job, which is the point of the target. Time inside LLVM
is reported separately, since stage 0 has no LLVM path to compare with. Following the order of work,
no stage before S4 is held back for speed. The S0 baseline is 1,431,300,899 instructions for lexing
the conformance corpus (#852, 2026-11-13).

- Decision: as recommended (lead, 2026-11-17)
- Row: R1054

### D14: wording, the two meanings of "stage 0"

Section 10.4 uses "stage 0" for compile-time evaluation; 22.1 and 22.7 use it for the Rust bootstrap
compiler; 22.2 uses both in one table. Recommended: sections 18, 22 and 23 say "bootstrap stage 0"
(and stage 1, 2, 3) wherever the Rust compiler or a built compiler is meant; 10.4 keeps its term. No
name changes, only the qualifier.

- Decision: as recommended (lead, 2026-11-17)
- Row: none (wording in 22.1 and 22.2)

## Stage breakdown

Issues are sub-issues of the stage issues #852 to #857. "After" lists hard dependencies.

| Stage | Work package | After |
| --- | --- | --- |
| S0 | widened readiness list and second program (D2) | #758, #1041, #1044 |
| S0 | ownership and soundness prerequisites (D11) | #1500, #1502 |
| S0 | bootstrap subset file and lint (D3) | D2 package |
| S0 | LLVM-C binding spike (D2 item 7) | #404 |
| S0 | std port to `.yn` (#1027) | S0 program green |
| S1 | shared tables as data (D5) | none; can start now in Rust |
| S1 | equality harness, CI jobs, blocking rule (D10, D4) | S0 |
| S1 | ports: diag, names, lossless tree and lexers, parsers and printers, Yan Core text and validator (#853) | S0 gate (D1) |
| S3a | canonical MIR text in stage 0 (D7) | none; can start now in Rust |
| S3a | LLVM code generator in Yan over MIR text (D8) | S0, MIR text |
| S2 | query engine in Yan (D6) | S1 |
| S2 | compile-time evaluator in Yan (D6) | S1 |
| S2 | ports: hir, elab, check, flow, opt (#854) | the two above; formal phases 1 and 2 (R970) |
| S3 | ports: mir, runtime core; generator linked in process (#855) | S2 |
| S3 | engines library and per-engine replacement plan (D9) | lead decision |
| S4 | fixpoint, seeds, DDC, ceiling table, determinism jobs (D10, D12) (#856) | S3 |
| S4 | performance gate (D13) | S3 |
| S5 | tools, rest of the interpreter, `yan-machine`, persistence for the query engine (#857) | S4 may overlap |

Known holes this RFC does not close: `yan-machine`, `yan-flow`, `yan-cst`, `yan-ebnf`, `yan-sandbox`
and `yan-playground` exist in `crates/` and are missing from the 22.2 table, which still lists
`yan-syntax`; S2 waits on formal phases 1 and 2, which wait on the M1 gate (#411), and that chain is
the longest dependency on the path to S4.

## Spec changes

- 18.1: `yan view --as mir` and `yan build --emit mir`; `yan build --release` names the Yan code
  generator.
- 19.5, 19.6: the determinism checks of D10 and the release checks of D12.
- 22.1, 22.2: "bootstrap stage 0" wording (D14); the `yan-cg-llvm` row becomes the Yan package (D8);
  the crate table gains the missing crates.
- 22.4: MIR text (D7); linking as a driver subprocess.
- 22.7: S1 entry gate (D1); widened S0 (D2); bootstrap subset (D3); progressive freeze (D4); S2 and S5
  contents (D6); the comparison table and harness (D10); the engines exception (D9); performance gate
  (D13).
- 23: M2a takes the MIR text and the start of the code generator; RC takes the release checks.

R1043 to R1054 state gates, structure and release checks with no program-observable behaviour and are
listed in `spec/row-exemptions.toml`, beside R966 to R970. The MIR text format, `yan view --as mir`
and `yan build --emit mir` (18.1) are not written here: they get their rows with their cases in the
PR set that implements them (#1513). The 22.2 crate table's missing crates and the remaining
"stage 0" wording in 18 and 23 are left to a follow-up.
