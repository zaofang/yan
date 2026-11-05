# Architecture overview

Read against main `1f2571a` (2026-11-04). Spec sections are cited as numbers (for example 3.6), change
rows as R numbers. This describes what is built; where the spec plans more, the gap is named.

## 1. The shape: one core, many surfaces

Yan is one language core, **Yan Core** (`.yir`), with several surface syntaxes that all lower to it:

- **Yan Agent** (`.yag`, crate `yan-yag`): the surface models write. Python-shaped (law 10: with types
  and contracts removed it should be valid Python 3.12), with a lenient reader that repairs common
  model mistakes (normalizations N01 to N32).
- **Yan** (`.yn`, crate `yan-yn`): the surface people read and write. Strict, one printing.
- **Yan Meta** (`.ymt`, RFC 0001/0029): dense research surface with spelling notes; print-only view
  in M3, reader in M4. Not built.
- **Niche surfaces** ("Yan &lt;job>", RFC 0002, R568): query, shell, schema, binary format and others.
  Each must satisfy rules N1 to N7 (16.7). None built.

The design laws (section 2) that shape the code most:

1. One semantics: a surface adds syntax, never behavior.
4. Five facts visible at the use site: mutation of a passed variable, failure, comptime, concurrency,
   capability use.
5. Nothing implicit at run time: no null, exceptions, hidden I/O, hidden heap copies, no runtime.
6. Local reasoning: an item checks from its body plus the signatures it references.
7. Deterministic by default.
8. Canonical text: one printing per surface.

## 2. Pipeline

```
  .yag / .yn text
      | lex, parse (surface crate), lenient reader for .yag
  surface AST  ----------------------------- print (canonical printer, `yan fmt`)
      | abstract / lower
  HIR (yan-hir) ------------------------------ raise (shared, back up to any surface: `yan view --as`)
      | elaborate (yan-elab): names, inference, capabilities, move/copy, A-normal form
  Yan Core (yan-core) -- item hash (BLAKE3), text form, validator + checker (yan-check)
      |                         \
      | yan-opt (Yan Core -> Yan Core)   yan-interp: register VM over checked Yan Core
      |   monomorphization (R772)         (the executable definition of the abstract machine, 6.1)
  yan-mir: per-function CFG, scalar locals, registry ids, decision trees
      |
  yan-cg-cranelift: JIT (debug code, M1b); LLVM for release is M2
```

Every step is meant to be reversible (RFC 0004 D1): text, lossless tree, surface AST, HIR (with trivia
and spelling notes), checked Yan Core. Down is parse, abstract, elaborate; up is print, resugar,
raise. The HIR is the only pivot between surfaces: Yan Agent and Yan never translate to each other
directly. Since #165 the HIR is surface-neutral (Yan type names, `Record`, no Yan-only forms).

Layering is one-way (R558, R560): base crates (`yan-diag`, `yan-names`) then surface crates, then
`yan-hir` and `yan-elab`, then `yan-core`, then back ends. `yan-elab` depends on no surface crate;
`yan-db` parses std sources so the elaborator need not.

Drivers sit on the **query database** (`yan-db`, salsa): `yan check`, `run`, `hash`, `patch`, `mcp`
ask it for a module's front end, elaborated core, diagnostics and items, keyed by file and item hash.
`yan-ide` is the analysis API shared by the CLI, MCP and the browser playground.

## 3. Yan Core

- **Form.** A-normal form: every intermediate value is named; control flow uses blocks and join
  points (`jp`). Records, sums, newtypes, traits, impls, externs, tests and contracts are items
  (`yan-core::ast::ItemKind`). Fall-through after a jump continues after the jumping statement (R764).
- **Text.** Yan Core has a text syntax with a parser and a canonical printer (`.yir`, 43 conformance
  cases). It is the interchange format between front and back ends and the format of `yan view --as
  yir`.
- **Item identity (3.6, R519).** Each item's hash is BLAKE3 over its canonical Yan Core text. Locals,
  closure parameters, labels and join points are named by binding order, references to other items
  are replaced by their hashes, and recursive groups hash as one, so renaming a local changes no hash. Formatting, comments and item order do not change hashes (law 4, 175/175 non-skipped cases
  pass). Hashes key the query database, the test verdict cache and `yan review`.
- **Trivia side table (16.5, `yan-surface`).** Everything needed to print a module byte for byte that
  is not in the hash: comments, blank lines, spelling notes, origin marks for inserted copies and moves
  (R565).

## 4. Types and checking

- **Inference** (`yan-elab`): bidirectional and local to an item (law 6). Lambda parameter types come
  from the expected type, else from the first call (#45). Generic functions with trait bounds (#28);
  no GATs (v0.3 R12).
- **Ownership** (section 4, v0.3 R1, R2): unique ownership, explicit `copy`, `Rc`/`Arc` as library
  types, second-class views (`View[T]`, `Span`) that cannot escape (E414). Since #169 the elaborator
  writes `move` at a value's last use and `copy` when it is used later, inside a loop or from a
  borrowed parameter. E403 flags a mutation that silently changed a copy.
- **`inout`**: parameters mutated by the callee; in MIR they come back as extra results. Writes back
  on failure (R765).
- **Failure** (section 8): typed fail unions (`T!E`, `Result`), `try`, `?` (E310 where the function
  cannot fail), panics always abort (v0.3 R5), integer overflow always checked (R6), allocation
  failure aborts with `try_` siblings (R7).
- **Bounds solver** (8.6, `yan-elab/bounds.rs`): proves index bounds over a decidable fragment
  (linear facts, list lengths, guards split into cases, `!=` and disjunctions, inferred loop
  invariants from RFC 0027). Unknown is W820, refuted is E501. 22/22 solver cases pass.
- **`yan-check`**: validator and type checker over Yan Core. It reports only what it can prove from
  one module; an unknown type never produces a mismatch. The full section 4 checker is not here yet.
- **Unsupported constructs** are reported as E017 and checking continues (R649), after an early bug
  where `check` exited 0 on them.

## 5. Capabilities and determinism

- **Capabilities are parameters** (section 7): a function lists what it uses (`uses fs, clock`), and
  the elaborator passes the capability values explicitly (capability passing). A capability is
  marked at each use site (law 4). Remote calls are `uses cluster` (R400). Std capability operations
  are declared with `@capability` and bound by registry id.
- **Handlers.** I/O, time, randomness and scheduling are handlers (v0.3 R3, R4): `par` lowers to a
  `sched` handler (sequential, seeded, pool); tests swap in fakes (`fake_fs`, `fake_proc`).
- **Determinism** (law 7): results depend only on arguments and capability results. `std.rand` uses
  seeded ChaCha8 and Pcg64Dxsm streams split by task path inside `par`, so a parallel run equals the
  sequential one. `par` runs sequentially today; `pool` is M2a.
- **Security model** (RFC 0026): no OS escape today because the interpreter has no host fs/net/proc
  handler and externs never run. Findings F1 to F8 (path handling in MCP, contract-name shadowing,
  closure capture of capabilities) were fixed in #119.
- **Supply chain.** Packages have no install or build scripts, and no surface has syntactic macros
  (section 19, 26.2). Building a dependency therefore runs no code from it: stage-0 evaluation is
  hermetic and uses no capabilities, so a malicious package can act only through the capabilities a
  caller passes it at run time, where they are visible in its signatures. This is a security property,
  not only a simplicity choice, and a proposal that adds either feature must answer for it.

## 6. Std and the intrinsic registry

- **Tiers** (R562): `lib/core` (no allocation), `lib/alloc`, `lib/std` (host). `yan-std/build.rs`
  fails the build if a module references a higher tier.
- **Sources are Yan Agent** (`.yag`, 40 files). Native bodies are `@intrinsic def` declarations.
- **Registry** (R561): `build.rs` line-scans std for `@intrinsic` and `@capability` and generates one
  id per entry. The interpreter (`yan-interp/bind.rs`) and native code (`yan-rt`) bind by id, never by
  name, so both back ends share one list. A selftest checks the scan against the real parser.
- **Generic std bodies** are lowered at their instance (#30).
- `yan-rt` holds plain-Rust kernels: text, list, sort (SIMD `sort_unstable` for primitive ints), math
  on CORE-MATH (correctly rounded, exact rounding modes), regex, RNGs.

## 7. Back ends

- **Interpreter** (`yan-interp`): compiles Yan Core to register bytecode (#37) with fast paths for
  branches, calls and division (#38); evaluates stage 0 (comptime, #72); prints `panic(kind): message`
  with a stack trace; runs `yan test` with `ex` lines as tests. It is the oracle every other back end
  must match.
- **Optimizer** (`yan-opt`, RFC 0028): Yan Core in, Yan Core out, so every pass is visible with
  `yan view` and checkable by the differential law (optimized and unoptimized runs agree).
  Monomorphization (#170) turns each reached generic instance into an `inst fn id[i64]` item. Drop
  elaboration (R770, drops only in optimized Yan Core) is accepted but not built.
- **MIR** (`yan-mir`): one CFG per function over typed scalar locals. Records, tuples and
  non-recursive sums are split into scalar slots (a sum is a tag slot followed by every variant's
  slots). `match` lowers to Maranget decision trees (#114). `Str` and `List` are pointers to `yan-rt`
  blocks that are never freed yet.
- **Cranelift** (`yan-cg-cranelift`): JIT with lazy stubs (a function compiles on first call, #117).
  No AOT object output or linking yet.
- **Leak counter**: `yan run --report-live` prints `live: N` to stderr at exit, N being the
  `yan-rt` blocks (`Str`, `List`, closure captures) native code took and has not freed
  (`yan_rt::heap::live_blocks`). Stdout is unchanged. The interpreter always reports `live: 0`
  (Rust owns its values). Not printed when the program panics or exits from inside native code.

## 8. Agent toolchain

- **Patch protocol** (18.3, `yan patch`): item-addressed edits (add, replace, delete) with base
  checks, atomicity and P001 to P006 refusals; P006 protects contracts and `@test_support` items.
- **MCP server** (`yan mcp`, in `yan-cli`): `check`, `check_patch`, `apply_patch`, `autofix`, `sig`,
  `spec_check`, `run_tests`.
- **Reference agent loop** (`yan agent`, RFC 0014): two phases. Spec phase: the model writes the
  signature, contracts and `ex` examples; the spec gate checks them (E619, `post` names, R767). Body
  phase: the model writes the body under a frozen spec; holes (`...`) elaborate and run as
  `unreachable`. Models are reached through a `cmd:` adapter; the toolchain has no HTTPS.
- **`yan review`** (RFC 0023): a review report over item hashes between two revisions, with the test
  verdict cache.
- **Diagnostics** (`yan-diag`, section 18, 25): one record, three renderings (line, prose, JSON),
  second-site labels, mechanical fixes, near-name suggestions (R651), `yan explain`.

## 9. Conformance and verification model

- **The developer owns the gates.** Agents never edit `tests/` or `conformance/`; `tools/guard.py`
  rejects such PRs in CI. Conformance changes ship in Antonio's own PRs.
- **Suite** (section 24): 544 cases in `conformance/` by surface and tool (`yir`, `yag`, `yn`,
  `solver`, `patch`, `pkg`, `semver`, `ub`, `golden`). Each file carries headers: spec, rows,
  milestone, flags, expect (code at line:col), stdout, exit. `tools/conform.py` is independent of the
  implementation and treats a tool crash as a failure.
- **Milestone gating**: each case has a milestone; CI runs `--milestone M0`.
- **Round-trip laws** (`conform.py --laws`, R564): law 1 (canonical Yan Agent prints back from its
  Yan Core byte for byte), law 3 (a Yan module inside the Yan Agent subset elaborates as its
  projection does), law 4 (hashes ignore formatting, comments, order).
- **Formal model** (`formal/`, RFC 0022): Lean 4 model of Yan-K (sequential Yan Core with ownership
  and failure) with an executable semantics and a reader, checked in CI against the interpreter on
  fixed cases and generated programs (`formal/drt.py`). Proofs come after M1. Phase 0 already found a
  double-rounding bug in `int / int` (#129).
- **Spec process**: `spec/` is Markdown per section; `spec/rows.toml` registers every change row;
  `tools/rows.py` allocates numbers, renders Appendix A and checks citations. RFCs label items D1, D2
  until accepted.

## 10. Patterns and paradigms

- **Multi-level IR with one semantic core** (in the line of Rust's HIR/MIR and MLIR's progressive
  lowering, but with a single dialect at the top).
- **Content-addressed code** (Unison-style item hashes), with names kept in the hash except locals.
- **Capability-passing effects** (object-capability security; handlers in the style of algebraic
  effects, without resumable continuations).
- **Mutable value semantics** for ownership (unique ownership, explicit copy, second-class views,
  `inout`), not a lifetime-annotated borrow checker.
- **Interpreter as oracle, differential testing everywhere**: interp vs Cranelift, interp vs Lean,
  unoptimized vs optimized, surface vs projection.
- **Generated registries over hand-kept tables** (intrinsic ids, Appendix A rows).
- **Agent-first tooling**: lenient reading in, canonical printing out, item-addressed patches,
  spec-before-body.

## 11. Difficulties, changes, pivots, cancellations and setbacks

Chronological, from git history, RFCs and project notes.

| Date | Event | Kind |
| --- | --- | --- |
| 2026-10-30 | v0.3 turned the language into a systems language with no runtime (R1 to R14): unique ownership, panics abort, checked overflow, core/alloc/std. | pivot |
| 2026-10-30 to 31 | Project renamed Ben/Fan, then Ming/Jian, then Yong/Jian/Wen in two days (#3, #4). | rename |
| 2026-10-31 | Python reference implementation and its Rust port (M1b) cancelled; Rust only (R215). Saves most of the old months 21 to 30. Relaxed on 2026-11-01: non-Rust parts allowed where they buy speed. | cancel |
| 2026-10-31 | Fei (third, dense surface) dropped from the spec; rows R210 to R239 freed. Revived on 2026-11-01 as a post-1.0 research track (RFC 0001), later renamed Chang, then Yan Meta. | cancel, then revive |
| 2026-10-31 | Yue, a token-minimal surface, measured 12% fewer tokens (o200k) than typed Python on 15 dry-run tasks. Proposed, never raced against Yan Agent. | open |
| 2026-10-31 | Tree-walking interpreter 43x slower than CPython on the M0 bench's collatz; replaced by a register VM (#37). Bench not rerun since. | setback, fixed |
| 2026-11-01 | Spec moved from a Claude Doc (rev 517) into `spec/` with a row registry (#44). | process change |
| 2026-11-01 | Lanting rename (R586, #74, #87): Jian became Xin, Fei became Chang, niche surfaces renamed after 兰亭集序. Brand kits Jian and Lanting archived. | rename |
| 2026-11-01 | Probe blocked: no API keys, OpenAI host blocked. Jian syntax rows put on hold until the probe ran. | setback |
| 2026-11-01 | Name-matched std binding dropped for an id registry (R561, #62 to #71). | redesign |
| 2026-11-01 | Studies flagged that the HIR was still shaped by the agent surface; fixed by #165 on 11-03. | debt, fixed |
| 2026-11-03 | Probe ran on Sonnet 5.5 only: first-reply parse 14% zero-shot; green 5/50. Root causes were the primer (wrong `ex` placement), `out` vs `result` in `post`, and elaborator gaps. After fixes (#153, #155, #158 to #162): green 26/50 and 29/50; tokens still 1.02x to 1.29x Python. | setback, partly fixed |
| 2026-11-03 | Security study found a critical path escape in MCP and an agent cheat via prelude shadowing; fixed (#119). | setback, fixed |
| 2026-11-03 | `elab` emitted bare `let ys = xs` for lists with no `move`; drops never generated. RFC 0028 decided both; move/copy built (#169), drops not yet. | debt |
| 2026-11-03 to 04 | Yong to Yan rename (R774, R775, #166, #168): Wen became plain Yan (`.yn`), Xin became Yan Agent (`.yag`), Yong Core became Yan Core (`.yir`). The conformance half needed Antonio's own PR. All 17 surface seals redrawn on one grid. | rename |
| 2026-11-04 | `std.list.push` replaced by `append` across code, tests and std (#172 to #174). | API change |

Recurring difficulties:

- **Renames cost real work.** Four project names and three names per surface in six days; each
  rename touched code, conformance (Antonio-only), brand and docs, and left stale branches the
  proxy cannot delete.
- **Agents cannot touch the gates.** This is deliberate, but every conformance change waits for
  Antonio, so features can merge with their suite still failing.
- **Counts can lie.** Unsupported constructs once counted as passes; law 1 now exposes 72 failures
  that plain conformance does not.
- **One model family.** R566 asks for two; OpenAI is off for now, so the gate evidence is thin.

## 12. Deviation from the original spec (measured, with estimates marked)

Baseline: `spec/archive/v0.3.md`, the frozen v0.3 draft of 2026-10-30.

| Measure | v0.3 | Now | Change |
| --- | --- | --- | --- |
| Spec size (bytes, section files only) | 229,412 | 465,808 | +103% |
| Highest change row | R259 | R810 | +551 numbers |
| Rows registered in `rows.toml` | (219 of v0.3's survive) | 665 at the time of the baseline | 446 rows newer than v0.3 |
| RFCs | 0 | 32 | |
| Named surfaces | 3 (Wen, Jian, Yong) | 2 built, 1 research, 17 named niche "Yan &lt;job>" | |

Text overlap (measured with a crude method, so an estimate of meaning): sentences over 40 characters
were normalized (lower case, names and R numbers masked, code spans removed) and matched. Of 1,642
v0.3 sentences, 298 survive verbatim and 238 more at 80% similarity or better: **about 33% of v0.3's
text survives**. Of the current spec's 2,878 sentences, **about 81% are new**. Much of the churn is
rewording, renames and the split into sections, so the semantic deviation is smaller than the text
deviation.

Semantic judgement (estimate):

- **Kept from v0.3**: the one-core law, the systems-language direction (no runtime, unique
  ownership, panic abort, checked overflow, core/alloc/std), capabilities as parameters, handlers,
  determinism, the five facts, the M1 gate as the central bet, the milestone order.
- **Changed**: implementation strategy (Rust only, no port), the third surface (dropped, then
  revived as research), every name, the architecture (HIR pivot, registry, optimizer as Yan Core to
  Yan Core, positional locals in hashes, origin marks as trivia), the gate's definition (split into
  body+repair tokens and spec-phase tokens, R567), niche surfaces with their own semantics (R568), a
  Lean formal model admitted (R680).
- **Added**: 446 rows covering std (time, path, encoding, rand, cli, proc, log, env), the agent
  loop and review, error-message rules, packaging, security, formal core, loop invariants, drops.
- **Estimate**: the core semantics of v0.3 hold at roughly 80% to 90%; the plan around it (how it is
  built, named and gated) has changed by more than half.
