# 0065: Technology picks for the surfaces: shared reader infrastructure, agent tooling, `std.sim`, and design bases for four niche surfaces

- Status: draft
- Research: [Yan surfaces: technology research](https://claude.ai/artifact/CZWDhwoMky8RN3rKEAs1eQ); issue #947, tracked by #449. Items: #948 to #957; the rest of the study is in tracker #958.
- Rows: filled in when accepted

## Summary

The surface study ranked ten technologies by capability gained per unit of cost across the sixteen
surfaces. Six live in shared infrastructure, `yan mcp` or std, so they can start before 1.0 without
breaking R550. Four are design bases for niche surfaces that R550 keeps after 1.0; accepting them now
fixes the starting point of each surface's own RFC without opening the surface.

Every link below was opened or resolved on 2026-11-10 (arXiv abstract, crates.io record, DOI
metadata, or the project's own page). Links the study had from memory and that could not be resolved
are not cited.

## Decisions

### D1: grammar export for constrained decoding (#948)

Add `yan grammar --as <surface> --format lark|gbnf`, which generates the grammar of a surface's
canonical form from its reader and prints it in a format constrained-decoding engines accept.
Engines: [XGrammar](https://arxiv.org/abs/2411.15100) (MLSys 2025) and
[llguidance](https://github.com/guidance-ai/llguidance). Yan Agent first.

Options: (a) generated from the reader; (b) a hand-written grammar file per surface; (c) not now.
(b) is a second representation of the grammar (practice 12). (c) leaves the parse-repair turns of the
agent loop (21) in place. Recommended (a).

- Decision:
- Row:

### D2: one lossless CST under every reader (#949)

Readers build a lossless red-green tree ([rowan](https://github.com/rust-analyzer/rowan), as in
rust-analyzer) that holds trivia and the spelling notes of 16.5. N3 one-printing and sugar recovery
read that tree. New lowering goes in a new module (practice 5).

Options: (a) now, for plain Yan and Yan Agent; (b) when the first niche reader is written; (c) never,
each reader keeps its own trivia. (a) costs most now but is cheapest overall: two readers to move
instead of fifteen, and it removes a duplicated representation today. Recommended (a).

- Decision:
- Row:

### D3: N7 stated as lens laws (#950)

Restate N7 (16.5) as the lens law pair: `S(elaborate(F)) == F` for canonical F is PutGet-shaped and
`elaborate(S(W)) == elaborate(W)` is GetPut-shaped, after Foster et al.,
[Combinators for bidirectional tree transformations](https://doi.org/10.1145/1232420.1232424)
(TOPLAS 2007) and [Boomerang](https://doi.org/10.1145/1328897.1328487) (POPL 2008).
`tools/conform.py --laws` (R564) gets one generic, seeded property test parameterised by surface.

No runtime library. The meaning of N7 does not change; the row gains the names and the test
obligation. Recommended: accept.

- Decision:
- Row:

### D4: typed-hole context tool in `yan mcp` (#951)

A new 21.4 tool `hole_context`: input a hole (R212) and an optional `max_tokens`; output the expected
type, the bindings in scope with their types, and the definitions of the types they mention, ranked
and cut to the budget. After Blinn et al.,
[Statically Contextualizing Large Language Models with Typed Holes](https://arxiv.org/abs/2409.00921)
(OOPSLA 2024). Recommended: accept.

- Decision:
- Row:

### D5: OS-enforced capability ceilings on child processes (#952)

`uses` stops at exec. `proc.run` and `proc.spawn` (7, R692) translate the caller's rooted handles into
an OS sandbox before exec: [Landlock](https://docs.kernel.org/userspace-api/landlock.html) on Linux
5.13 and later, and process handles become
[pidfds](https://man7.org/linux/man-pages/man2/pidfd_open.2.html). Capsicum on FreeBSD is the second
target. The macOS sandbox profile API is deprecated.

The decision is what happens where the OS cannot enforce the ceiling:

- (a) Fail loud: `proc` refuses to start a child with a narrower ceiling than the process unless the
  caller passes an explicit unenforced opt-in, which is visible in the capability manifest.
- (b) Run with a warning.
- (c) Enforce only where available and say nothing.

(c) is a silent fallback (practice 13). (b) lets a test pass on one OS and leak on another.
Recommended (a).

- Decision:
- Row:

### D6: PCT scheduling and trace validation in `std.sim` (#953)

Two additions to 17.7:

1. A PCT scheduler, after Burckhardt et al.,
   [A randomized scheduler with probabilistic guarantees of finding bugs](https://doi.org/10.1145/1736020.1736040)
   (ASPLOS 2010): seeded, with `d` priority change points, and a stated lower bound on the chance of
   hitting a bug of depth `d`.
2. Trace validation: a recorded `std.sim` trace is checked against a state-machine spec, after
   Cirstea, Kuppe, Merz et al.,
   [Validating Traces of Distributed Programs Against TLA+ Specifications](https://arxiv.org/abs/2404.16075).
   The spec shape follows [P](https://p-org.github.io/P/): communicating state machines, as Yan actors
   are. This is the base Yan Spec (R548) later builds on.

Recommended: accept both; PCT first, since the actor work (#929) can use it at once.

- Decision:
- Row:

### D7: egglog for rewriting attributes (#954)

[egglog](https://arxiv.org/abs/2304.04332) (Zhang et al., PLDI 2023; crate `egglog`) is the engine
for R783 and a candidate for yan-opt rewrites, replacing the `egg` already in tree there. A fixed rule
schedule and node limit keep it deterministic; hitting the limit is a diagnostic. Yan Meta research
track; no work before its gate. Recommended: accept as the R783 engine choice.

- Decision:
- Row:

### D8 to D10: design bases for three niche surfaces (#955, #956, #957)

These record the starting point of each surface's RFC. None opens a surface before 1.0 (R550).

- D8, Yan Binary Format: [EverParse/3D](https://project-everest.github.io/everparse/) constructs
  (dependent records, validators that never read out of bounds, double-fetch freedom) as the core
  forms, lowered to `View[u8]` records with refinements and `pre`; a [Kaitai Struct](https://kaitai.io/)
  `.ksy` importer after the reader.
- D9, Yan Law: the [Catala](https://catala-lang.org/) default calculus
  ([ICFP 2021](https://doi.org/10.1145/3473582)) as the semantics of R549; lowering versus an R568
  engine is left to the surface RFC.
- D10, Yan Workflow: [Temporal](https://docs.temporal.io/workflows)-style event-history replay for
  `@durable`, with determinism proved by law 7 and workflow versions pinned by item hash.

Options: (a) add one sentence to each surface's entry in 16.7 naming its basis; (b) keep them in this
RFC only and let each surface RFC decide; (c) reject. (a) makes the basis visible where a surface RFC
starts and costs three sentences. Recommended (a).

- Decision:
- Row:

## Spec changes

When accepted: 18 (D1 the `yan grammar` command); 14.3 (D1 grammar generated from the reader); 21.4
(D4 `hole_context`); 16.5 (D2 lossless tree for N3, D3 lens form of N7 and the test
obligation in R564); 7 (D5 `proc` ceiling and the unenforced opt-in); 17.7 (D6); 16.6 (D7 under R783);
16.7 (D8 to D10).
