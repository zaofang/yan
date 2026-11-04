# Bootstrapping roadmap: a self-hosted Yan compiler

Study and roadmap, 2026-11-04. Docs only: no spec rows, no conformance edits, no diagnostic codes, no
compiler changes. Items are labelled D1, D2 and so on; none carries a row number. Everything here is
non-normative until an RFC and accepted rows say otherwise.

Reading of the brief ("Bootstrapping roadmap"): a staged plan for writing the Yan compiler in Yan,
from today's Rust implementation (R215) to a compiler that builds itself. Two other readings were
possible and are covered only where they touch this one: bootstrapping the toolchain install (`yan
toolchain`, `yan self update`, 19.6), and bootstrapping the library in Yan (std is already 58 `.yag`
sources in `crates/yan-std`, which is stage A below in all but name). If either was meant, say so and
this doc is rescoped.

Read against main `8a49ea1`: spec 22.1, 22.2, 23, 19.5, 19.6, 26.1, 26.2, `docs/architecture.md`,
`docs/reports/2026-11-04-progress.md`, `docs/studies/build-system.md`.

## 1. Answer

Self-hosting conflicts with a decided row. R215 and 22.2 say the compiler and tools MUST be Rust, and
26.1 gives the reason: one codebase, no port. A self-hosted compiler is a port. So the first decision
(D1) is whether to reopen R215 at all, and when.

Recommendation: keep R215 for 1.0. Do not self-host before RC. Before RC, do only the work that is
needed anyway or that measures the product: finish the Rust prerequisites (they are all M1b and M2a
items), and write two small Yan programs that dogfood the compiler's own domain (stage A). After the
1.0 freeze, self-host in stages along the seams the architecture already has, front end first, with
the Rust implementation kept as the stage0 seed and as the diverse compiler for trusting-trust checks.

The seam that makes this tractable is Yan Core text. It has a parser, a printer, a validator and a
BLAKE3 item hash today. A Yan-written front end that emits `.yir` byte-identical to the Rust front end,
with equal item hashes, on the whole conformance corpus, is a complete and cheap oracle. The back end
can stay Rust behind that text boundary for as long as needed.

## 2. Why bother, and why not now

For:

- **Largest real Yan program.** A compiler is the program agents would write most of. It tests the
  M1 bet (agents write Yan Agent well) on 30 to 40 thousand lines of non-toy code, which no benchmark
  does.
- **Determinism pays off.** Law 7 and 19.5 already demand bit-reproducible builds. A fixed-point check
  (stage2 equals stage3, byte for byte) is the natural end-to-end test of that promise.
- **Capability-safe compiler.** A Yan compiler can declare its capabilities in its header: read
  sources, write outputs, no `net`, no `proc` except the linker. Capability ceilings (19.2) then make
  "the compiler cannot phone home" a checked fact, which a Rust compiler cannot offer.
- **Ecosystem signal.** Go, Zig, Rust, OCaml and others treat self-hosting as a maturity marker.

Against, now:

- **R215's rationale still holds before 1.0.** Every accepted row would have to land twice while the
  spec churns (446 rows in five days per the progress report). One part-time lead writes all the
  conformance; a second implementation doubles the code that must pass it.
- **Prerequisites are not built.** AOT native builds, drops, `Pool`/`Handle`, FFI and the C profile are
  M1b and M2a. A compiler running on the interpreter would be too slow to be its own test.
- **The M1 gate is unmet** (R567 rerun: Sonnet 66, Haiku 50, below typed Python). If it fails, Yan
  Agent moves closer to Python, and a compiler written in today's Yan Agent would be rewritten.
- **Dependencies are Rust crates.** salsa, rowan, Cranelift and blake3 have no Yan equivalent. Cranelift
  has no C API; LLVM has one.

## 3. Prior art (short)

| Project | Seed | Lesson for Yan |
| --- | --- | --- |
| Go 1.5 | C compiler translated to Go mechanically, then Go 1.4 binary as the seed | keep an old binary as the seed; translation beats rewrite when the source is stable |
| Rust | OCaml compiler, later each release builds with the previous beta; mrustc as a C++ second path | release-N-1 seed chain; a second implementation enables diverse double compiling |
| Zig 0.11 | the self-hosted compiler checked in as a WASM blob plus a small C WASM interpreter | the seed can be a portable IR snapshot run by a tiny interpreter |
| OCaml | bytecode `boot/` snapshot committed and run by a C bytecode interpreter | same pattern: commit IR, keep the interpreter small and auditable |
| GNU Guix, bootstrappable.org | full-source bootstrap from a few hundred bytes of hex | the end state for trusting trust; optional and late |
| Wheeler, DDC (2009) | compile the compiler with a second, independent compiler and compare | needs two implementations; the Rust one can be that second one forever |

Yan Core text plays the role of Zig's WASM blob and OCaml's `boot/`: content-addressed, readable, and
run by an interpreter the project already has.

## 4. The stages

Each stage has a gate that is a conformance run plus a differential check. Stage letters are not
milestones; the right column places them against 23.

| Stage | Built in Yan | Still Rust | Gate | Earliest |
| --- | --- | --- | --- | --- |
| Pre | nothing | everything | prerequisites of section 5 met | M1b, M2a |
| A | dogfood: `.yir` reader, printer, validator; `.yag` canonical printer | everything shipped | byte-identical output to `yan-core` and `yan fmt` on all `conformance/yir` and `.yag` cases | after M1 gate and M1b AOT |
| B | Yan Agent front end: lexer, parser, lenient reader, printer | HIR, elab, check, back ends | `yan fmt` output and lenient-reader diagnostics identical on every `.yag` case | after RC |
| C | HIR, elaborator, `yan-check`; emits `.yir` | interp, opt, mir, codegen | `.yir` text and item hashes identical to Rust on the whole corpus; diagnostic codes identical | after RC |
| D | `yan-opt`, `yan-mir`; codegen libraries through FFI | Cranelift and LLVM, the runtime support crate | every `run` case identical to the interpreter; release binary works | 1.x |
| E | the driver; stage1 builds stage2 builds stage3 | the Rust tree as seed and DDC partner | stage2 and stage3 bit-identical on Linux and macOS; DDC with the Rust stage0 matches | 1.x |
| F | optional: interpreter, `yan.lock` tooling, LSP | per D9 | per D9 | later |

Notes on each:

- **A** is the only stage before 1.0. It ships nothing; it lives under `examples/` or a `selfhost/`
  workspace, runs in CI as an ordinary program, and reports its numbers (lines, tokens per solved
  task, agent repair rounds, compile and run time against the Rust crate it mirrors). It doubles as a
  benchmark for the M1 claim on real code. It touches no `tests/` or `conformance/` files: it reads
  the corpus as input.
- **B** is small (about 5 thousand Rust lines in `yan-yag`) and has a byte-level oracle in `yan fmt`.
  The Rust driver calls it as a separate process over text, so there is no FFI and no shared memory.
- **C** is most of the work (`yan-elab` alone is 13.6 thousand lines). The output is `.yir` text, so
  the composition stays UNIX-shaped: `yan-fe file.yag > file.yir`, and the Rust back end reads it.
  Item hashes give a per-item diff when outputs disagree.
- **D** keeps code generation libraries foreign, as Rust keeps LLVM. LLVM is reached through its C
  API; Cranelift needs a thin C-ABI shim crate, or the back end moves to LLVM only for the self-hosted
  path (D6).
- **E** is the fixed point. Stage0 is the Rust `yan`. Stage1 is the Yan compiler built by stage0.
  Stage2 is built by stage1, stage3 by stage2. Stage2 equal to stage3 is the reproducibility test;
  stage1 built by stage0 and by the previous Yan release, then each building the compiler, is
  diverse double compiling (section 7).

## 5. Prerequisites (from the Rust side)

All of these are already in 23 for M1b or M2a; none is new work. The table only says which stage
needs which.

| Need | Where in spec | Status (architecture.md) | Needed by |
| --- | --- | --- | --- |
| AOT build with the system linker | 23 M1b | not built | A |
| Drops, no leaks of `Str` and `List` | RFC 0028, R770 | not built | A (B for long runs) |
| `fs`, `cli`, `proc.run`, `io`, `text` byte access, `Map`, `Set` | 17, 23 M1 | mostly built | A |
| Sum types, match, generics, traits, closures | 3, 10 | built | A |
| Arenas: `Pool` and `Handle` (or `Rc`) for trees and graphs | 17, 23 M2a | `rc` missing | B |
| `std.hash` stable hash, BLAKE3 through `std.crypto` | 17 | not built | C |
| Monomorphization and ownership passes, for speed | 23 M2a | partial in `yan-opt` | C |
| FFI and the C profile | 12, 23 M2a | not built | D |
| `par` with codegen independent of thread count | 19.5 | interpreter only | E |
| Reproducible builds, `--verify` | 19.5, 23 M2b | specified | E |

Two language questions surface only for a compiler and are worth noting early, without changing
anything now: a compiler wants interning and cheap shared immutable trees (today `Rc`, 17), and it
wants an incremental query engine, which in Rust is salsa. The first is covered by `Pool`/`Handle`.
The second is not; stage C can start as a batch compiler and leave incrementality to the Rust LSP
path (D5).

## 6. Conformance as the bootstrap gate

22.1 already says every implementation must produce the same result as the interpreter, diagnostic
codes included, on every conformance program. A self-hosted front end is one more implementation
under the same rule. Concretely:

1. The gate for a stage is `conform.py --milestone M1` (or the milestone that stage covers) run with
   the self-hosted component substituted, plus a text diff of that component's output against the
   Rust one.
2. A difference is a bug in one of them or a gap in the spec, the same rule R680 sets for the Lean
   model. It is filed in `SPEC_QUESTIONS.md` when the spec is silent.
3. The self-hosted path is added to CI only as a separate job, so a self-hosting failure never blocks
   a Rust PR until the lead promotes it (D8).
4. Diagnostics: codes must match exactly; message text may differ until stage C, where it must match
   byte for byte so `yan fix` and the agent loop see one toolchain.

The Lean model (R680) stays a third, independent check on the Yan-K fragment. Three implementations
of the core (Rust interpreter, Yan front end feeding it, Lean model) make disagreements cheap to
localize.

## 7. Reproducibility, trusting trust and supply chain

- **Seed.** The Rust tree, pinned and vendored under cargo-vet (19.6), is the stage0 seed. Rust's own
  bootstrap (and mrustc) is outside Yan's scope; the project trusts a pinned `rustc` as it does now.
- **Alternative seed.** At stage E the project can commit the stage1 compiler as `.yir` text with its
  item hashes, run by the Rust interpreter (or a small C interpreter, Zig-style). That removes the
  need to build the whole Rust tree to bootstrap, at the cost of a large reviewed blob (D7).
- **Fixed point.** Stage2 and stage3 must be bit-identical. Law 7 and 19.5 make this a direct test,
  and it is the cheapest proof that the compiler itself has no hidden nondeterminism (hash iteration
  order, thread scheduling, timestamps).
- **Diverse double compiling.** Build stage1 twice, once with the Rust stage0 and once with the
  previous Yan release; use each to build the compiler; the two results must be bit-identical. As
  long as the Rust tree is kept buildable, Yan has a permanent, genuinely independent DDC partner,
  which is rare.
- **Capability ceiling on the compiler.** The self-hosted compiler declares its capabilities; a
  release rejects any compiler build whose computed capability set grows (for example gains `net`).
  This is a checked version of the "compiler does not reach the network" policy most toolchains
  enforce only by sandboxing.
- **Release flow.** 19.6 already requires a matching independent rebuild before `yan self update`.
  The fixed point and DDC checks join that list at stage E, not before.

## 8. Ordering against milestones and open RFCs

- **M1 gate first.** Stage A starts only after the R567 gate result is known; if Yan Agent changes,
  stage A code changes with it, which is a small loss at stage A size and a large one later.
- **M1b and M2a deliver the prerequisites.** No reordering is proposed; this roadmap adds no work to
  them. The one ordering preference: drops (R770) and the AOT build before stage A, which the
  progress report already ranks at positions 2 and 6.
- **M2b and M3 are unaffected.** The self-hosted front end covers Yan Agent and Yan Core only. Yan
  (`.yn`), Yan Meta and niche surfaces stay Rust until stage F, since the HIR is shared and the Rust
  `.yn` front end can feed the Yan elaborator later (D4).
- **Open RFCs.** RFC 0030 (nested optionals), 0031 (forum batch), 0032 (net) and the studies of
  2026-11-04 (actor send, call-once, extractors, lint sets, relaxed floats, shield) change what the
  front end accepts. Each is cheaper to land once, in Rust, before stage B. That is the main argument
  for starting B only after the RC freeze (20.2): from then on, the language changes only by edition
  and the two front ends move together slowly.

## 9. What to cut

| Cut | Why |
| --- | --- |
| Self-hosting from 1.0 entirely | R215, spec churn, unmet gate |
| A Yan-written code generator | keep Cranelift and LLVM as foreign libraries; a new backend is a separate project |
| A Yan-written salsa | batch compilation first; incrementality stays in the Rust IDE path |
| Self-hosting the interpreter | it is the executable definition of the abstract machine (6.1); one copy is safer |
| Self-hosting LSP, playground, notebooks | no correctness value, large UI surface |
| Self-hosted `.yn`, `.ymt`, niche front ends | the HIR is the pivot; Rust front ends can feed the Yan elaborator |
| Full-source bootstrap (Guix-style) | optional end state; DDC with Rust gives most of the assurance |
| Mechanical Rust-to-Yan translation tooling | the Rust code is small enough to rewrite, and a translator is a third compiler to maintain |

## 10. Effort (estimate)

Counted from `crates/*/src` on main: about 56 thousand lines of Rust. The self-hosted scope of stages
B and C (`yan-yag`, `yan-hir`, `yan-elab`, `yan-check`, `yan-core`, `yan-diag`, `yan-names`) is about
27 thousand lines; stage D adds `yan-opt` and `yan-mir`, about 2.7 thousand, plus FFI bindings. Yan
Agent is denser than Rust by the probe's measure, so expect fewer lines in Yan, and agent-written code
means the lead's time goes to the gates, not the port. The cost that does not shrink is review and the
conformance work for any spec gap the second implementation finds.

## 11. Decisions

- **D1. Reopen R215?** (a) Keep R215 for 1.0; self-host after RC (recommended). (b) Reopen now and
  self-host the front end during M2. (c) Never self-host; Rust is the only implementation.
- **D2. Stage A before 1.0?** Dogfood programs (`.yir` reader, printer, validator; `.yag` printer),
  outside the shipped toolchain, reported as an M1-style measurement. Recommended: yes, once the M1
  gate result is in and the AOT build exists.
- **D3. Which surface is the compiler written in?** (a) Yan Agent, since agents write the code
  (recommended). (b) Plain Yan, since people review it. The source can be viewed in either (law 1),
  so this only sets the canonical text in the repo.
- **D4. Front-end scope.** Yan Agent and Yan Core only in Yan; `.yn` and others stay Rust until stage
  F (recommended), or all surfaces at stage B.
- **D5. Incremental compilation in the self-hosted compiler.** Batch only, with the Rust query
  database kept for the IDE (recommended), or a Yan query engine at stage C.
- **D6. Code generation in the self-hosted path.** LLVM through its C API only (recommended for
  simplicity), or a C-ABI shim over Cranelift so debug builds stay fast.
- **D7. Bootstrap seed.** (a) The Rust tree, kept buildable forever, which also serves DDC
  (recommended). (b) A committed `.yir` snapshot run by an interpreter, Zig-style. (c) Both: Rust for
  DDC, snapshot for convenience.
- **D8. CI placement.** Self-hosted jobs run separately and never block Rust PRs until a stage passes
  its gate for N consecutive main commits (recommended N = 20), or they gate from day one.
- **D9. End state of the Rust tree.** (a) Kept as a maintained second implementation, the DDC
  partner and the interpreter's home (recommended). (b) Frozen at the release that first builds the
  Yan compiler, used only as seed. (c) Retired after a `.yir` seed exists.
- **D10. Diagnostic message parity.** Codes identical from stage B, text identical from stage C
  (recommended), or text identical from stage B.
- **D11. Compiler capability ceiling.** Declare and enforce the compiler's capability set as a
  release check at stage E (recommended), or leave it to sandboxing.

## 12. Lead's answers (cards, 2026-11-04)

| Item | Answer | Differs from the recommendation |
| --- | --- | --- |
| D1 | keep R215 for 1.0; self-host after RC | no |
| D2 | stage A before 1.0, after the M1 gate and the AOT build | no |
| D3 | Yan Agent is the canonical source | no |
| D4 | all surfaces at stage B | yes: `.yn`, Yan Meta and niche front ends are ported at B, not left in Rust until F |
| D5 | a Yan query engine at stage C | yes: the self-hosted compiler is incremental from C; salsa is not cut |
| D6 | LLVM through its C API only | no |
| D7 | both seeds: the Rust tree for DDC and a committed `.yir` snapshot | yes |
| D8 | self-hosted jobs block only after 20 consecutive green main commits | no |
| D9 | retire the Rust tree once a `.yir` seed exists | yes |
| D10 | codes identical from B, text from C | no |
| D11 | the compiler's capability ceiling is a release check at stage E | no |

Consequences for the sections above:

- Section 9 loses three cuts: Yan-written salsa (D5), self-hosted `.yn`/`.ymt`/niche front ends (D4)
  and, through D9, self-hosting the interpreter, since retiring the Rust tree retires `yan-interp`,
  the executable definition of the abstract machine (6.1). Stage F must then port the interpreter
  before the retirement, and its gate is the interpreter's own conformance plus agreement with the
  Lean model (R680) on Yan-K.
- D7 and D9 interact. Once the Rust tree is retired, DDC (section 7) has no independent compiler: the
  partner becomes the previous Yan release, which shares ancestry. The Lean model stays independent
  only for Yan-K. Open follow-up: keep a frozen Rust stage0 buildable for DDC only, or accept the
  weaker check after retirement.
- Stage B grows from about 5 to about 13 thousand Rust lines (`yan-yag` plus `yan-yn`), and stage C
  adds a query engine; the effort estimate in section 10 rises accordingly.

No rows are written: R215 stands for 1.0, and the first spec change (reopening R215) is due after RC.
