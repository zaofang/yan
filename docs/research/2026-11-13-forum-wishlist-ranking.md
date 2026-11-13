# What language communities ask for, ranked for Yan

Study, 2026-11-13, against main at `7f4486c`. Exploratory (spec freeze #1025): no rows, no RFC.
Companion to the [Yan Meta syntax proposal](2026-11-13-yan-meta-syntax-proposal.md).

## 0. Summary

Five sweeps covered forums, issue trackers and surveys for about 35 languages: systems (Rust, C++,
Zig, Go, Carbon), managed (Swift, Kotlin, C#, Java, Dart, TypeScript, JavaScript), scripting (Python,
Ruby, PHP, Elixir, Julia), functional and research (Haskell, OCaml, Scala, F#, Lean, Koka, Unison,
Roc, Flix, Effekt; r/ProgrammingLanguages, Hacker News, recent POPL/PLDI themes), and data, array
and AI-era communities (Mojo, APL/BQN/Uiua, PyTorch/JAX, PRQL/Malloy/Gel, marimo, LLM-language
threads). About 210 raw requests deduplicate to 70 themes.

Three findings:

1. **Yan already has most of what mainstream users ask for most loudly.** Sum types (Go #19412,
   1,045 votes; Kotlin 45%; C#'s longest-running request), pattern matching, typed errors,
   structured concurrency, data-race safety, a pipeline operator, units, named and shape-checked
   array axes, capabilities, deterministic replay, content-addressed items, typed holes and
   auto-active verification are all in the spec. These are the honeypot: section 2.
2. **The open gaps with the best value for cost** are compile speed through hash-keyed incremental
   queries, user notations with reader lenses, live programming on item hashes, size arithmetic,
   rank polymorphism, effect handlers with one-shot resume and proof tactics. Every one of them
   already has a research row or a design note; none needs a new direction. Ranked in section 3.
3. **The pushback is consistent across communities**: no runtime code in the compiler (Swift macro
   build times, Dart cancelled macros in January 2025, Zig and Go refuse them), no second syntax for
   the same thing (Go kept `if err != nil`, 2,007 votes to 223; Python deferred `?.`; JavaScript
   users list cognitive overload as their second pain at 10.6%), heavy annotation syntax for
   effects and captures loses users (OCaml typed effects "failed to gather support"; Scala capture
   syntax), and invented syntax for LLMs hurts models. Section 4 turns these into rules.

The recommendation, best only (section 5): build the top seven gaps in the order given, through Yan
Meta's lab where they change syntax or the core, and decline specialization, higher-kinded types,
full dependent types and procedural macros.

## 1. Method and scoring

Each sweep returned items with evidence (votes, survey shares, thread size) and a status in the
asking language. Items were merged by meaning, then checked against the spec (section headings, the
26.6 research table, the 26.2 non-goals) and classified:

- **Have**: specified for Yan 1.0 (spec section given).
- **Research**: a 26.6 research row exists (row given).
- **Gap**: neither.
- **Decline**: conflicts with a law or non-goal, or the forum evidence says it hurts.

Scores, 1 to 5, are judgment informed by the evidence:

| Score | Meaning |
| --- | --- |
| Demand (D) | how many communities ask, and how strongly (votes, survey share) |
| Add (A) | how much Yan gains: new programs made possible, bugs removed, users drawn |
| Novelty (N) | 5 = no shipping language has it |
| Difficulty (X) | 5 = core semantics, checker and both backends; 1 = a formatter rule |

**Priority = 2A + D + N - X.** Add counts double because Yan is not chasing popularity; difficulty
is subtracted once because the project already accepts hard work for real gains.

Home says where the feature should land first: **Yan** (core, with Yan and Yan Agent spellings),
**Yan Meta** (lab first, under `@lab`, graduating per R777), **std**, or **tooling**.

Caveats: GitHub reaction counts came from the search API on 2026-11-13; several survey charts are
images, so some shares are approximate; items marked "unverified" by the sweeps are noted.

## 2. What Yan already has (the honeypot)

| Theme | Asked by (evidence) | Yan |
| --- | --- | --- |
| Sum types, exhaustive match | Go #19412 1,045 votes; Kotlin survey 45%; C# #75; Mojo; HN | core types (3), match |
| Pattern matching | JS 20.1% (State of JS 2025); Swift `is case` 57 replies; C++ P1371 | core, all surfaces |
| Typed errors, no exceptions | Swift typed throws 88 replies; Kotlin rich errors KEEP-0441; Zig #2647 310 votes | failure types (8.1) |
| Null safety, `??`, `?.` | Python PEP 505 revival; Kotlin; HN | `Option`; Yan Meta `??`, `.?f` |
| Pipeline operator | JS 26.0%; PHP 8.5 passed 33 to 7; R 4.1 | Yan Meta `\|>` |
| Partial application | PHP RFC 33 to 0 | Yan Meta holes and sections |
| Collection literals | Kotlin 32% | all surfaces |
| Context parameters | Kotlin 30%, stable in 2.4 | capabilities as declared context (7) |
| Structured concurrency, no colouring | Java Loom; Swift roadmap 167 replies; HN | `par`, cancel scopes (9) |
| Data-race safety | Swift 182-reply approachability thread | exclusivity, Send/Sync (4, 6) |
| Ownership, noncopyable, in-place | Swift ownership 268 replies; OxCaml modes; Hylo, Vale | ownership and views (4) |
| Partial borrowing | Rust rfcs#1215, 149 votes | projections and views (4.4, 4.5) |
| Value types | Java Valhalla JEP 401 | value semantics by default |
| Keyword and default arguments | Rust rfcs#323, 943 votes | named arguments, defaults |
| Generators | Rust #43122, 328 votes; Zig #23446 | generators (10.5) |
| Reflection, derive, serialization | C++26 P2996; Swift 177-reply serialization thread; Mojo | stage-0 reflection, `derived`, `Wire` (10.4) |
| Compile-time evaluation | Zig; Mojo | stage 0 (10.4) |
| Arenas | Go #51317, 396 votes | allocators and arenas (5.5) |
| Embed files | Go #35950, 819 votes | package-scoped `embed` at stage 0 |
| 128-bit integers, SIMD | Go #9455 278 votes; Go survey 37% of SIMD users | i128, SIMD (11.3) |
| Units of measure | Python typing thread; F#; Julia | units (11.4) |
| Shape-checked, named tensor axes | PEP 646; PyTorch named tensors; jaxtyping | shape types with labelled axes (11.2) |
| Automatic differentiation | JAX, Julia, Mojo | AD (11.5) |
| GPU kernels in the host language | Mojo | `@kernel`, `gpu.launch` (11, RFC 0061) |
| Typed tables and queries | sqlx, jOOQ, LINQ; PRQL; Malloy | tables (11.9); Yan Query Language study |
| Capability sandbox for agent code | agent-builder surveys | capabilities, ceilings (7, 19.2) |
| Deterministic replay | AgentOS; Temporal | record and replay (7.6) |
| Content-addressed code | Unison; HN 165-comment thread | item hashes (3.6) |
| Verified AI-generated code | vericoding benchmark (arXiv 2509.22908) | auto-active verification (8.7) plus the agent loop (21) |
| Familiar syntax plus strong types for LLMs | HN GlyphLang thread pushback | Yan Agent (13, 14) |
| Typed holes | Hazel; Lean | typed holes (21.2) |
| One fast official checker, good errors | Python typing survey: 33.3% hit checker inconsistencies; Haskell survey | one checker; diagnostics and patch protocol (18) |
| Editions, stability | Haskell: 27% broken by upgrades; C++ epochs | editions (20.4), compatibility promise (20.1) |
| Package manager | C++ survey top theme | registry and lockfile (19) |
| Strict evaluation | Haskell survey | strict by design |
| Stable formatter, no options | Haskell, Gleam | `yan fmt`, no options (18.5) |

Reading: a programmer from almost any community finds their top wish already answered. The pitch
writes itself, but only with working demos; spec rows alone draw nobody.

## 3. Gaps and research items, ranked

| # | Feature | Asked by (evidence) | Status | Home | D | A | N | X | P | Feasible |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | Fast incremental compilation by item hash | Rust surveys: top pain; Haskell "make GHC 10x faster"; Julia TTFX ~41%; Swift macro thread; C++ modules | R528 (queries) | tooling | 5 | 5 | 2 | 4 | 13 | yes |
| 2 | User notations, DSL islands, reader lenses | Rust internals grammar-for-macros; sqlx/rsx users; Lean; HN | proposal; R526, R527 | Yan Meta | 3 | 4 | 5 | 3 | 13 | yes, as proposed |
| 3 | Live programming and hot reload on item hashes | Zig #68 hot swapping 96 votes; Rust REPL rfcs#655 348 votes; Hazel; Future of Coding; marimo | gap (REPL is M3; R529 synthesis) | tooling | 3 | 4 | 5 | 4 | 12 | yes, after M3 |
| 4 | Size arithmetic in types | Rust generic const exprs: top 2025 survey want, #76560 297 votes; Zig ranged ints #3806 | R781 | Yan Meta lab, then Yan | 4 | 4 | 3 | 4 | 11 | yes (Presburger, decidable) |
| 5 | Rank polymorphism | APL, BQN, J, Uiua; NumPy broadcasting; PEP 646 | R780 | Yan Meta lab, then Yan | 3 | 4 | 4 | 4 | 11 | yes (Remora, Dex) |
| 6 | Effect handlers, one-shot resume | OCaml typed effects thread; Koka, Effekt, Flix, Unison; HN effects threads | R522 | Yan Meta lab, then Yan | 4 | 4 | 3 | 4 | 11 | yes; keep annotation light |
| 7 | Proof tactics and a vericoding loop | vericoding research; Lean users; Dafny/Verus | R784 | Yan Meta, agent loop | 3 | 4 | 4 | 4 | 11 | yes |
| 8 | Reactive cells and signals | JS Signals 29.5%; marimo reactive notebooks; Observable 14.1% | R528 (`Cell[T]`) | std | 4 | 3 | 3 | 3 | 10 | yes |
| 9 | Anonymous unions and error unions | Kotlin 45% (union for errors); F# FS-1092; TypeScript | gap | Yan Meta, needs core positional sums | 4 | 3 | 2 | 3 | 9 | yes, needs a hashing rule (K2) |
| 10 | Information-flow labels | agent security; Jif research | R523 | Yan Meta lab | 2 | 3 | 5 | 4 | 9 | yes; a sixth fact |
| 11 | Structured templates for SQL, HTML, shell | Python t-strings (PEP 750, shipped 3.14); Java string templates withdrawn | partial (interpolation; islands) | std plus Yan Meta islands | 3 | 3 | 2 | 2 | 9 | yes |
| 12 | Row-polymorphic and anonymous records | Rust structural records PR#2584 262 votes; Haskell records; Roc, Koka | R785 | Yan Meta lab | 3 | 3 | 2 | 3 | 8 | yes |
| 13 | Variadic generics | Rust rfcs#376 241 votes; PEP 646 | gap (stage-0 tuples cover part) | Yan | 3 | 3 | 2 | 3 | 8 | yes, via stage 0 |
| 14 | Ranged integer types | Zig #3806 105 votes; Ada | gap (contracts cover) | Yan Meta notation over contracts | 2 | 3 | 2 | 2 | 8 | yes |
| 15 | Code mobility by hash across a cluster | Unison Cloud | partial (actors, `uses cluster`) | Yan | 2 | 3 | 4 | 4 | 8 | later |
| 16 | Word-to-glyph formatter input | Uiua (HN 334 points) | partial (word twins) | Yan Meta tooling | 2 | 2 | 3 | 1 | 8 | yes, cheap |
| 17 | Mapped and conditional types | Python typing 2025 survey top request | partial (stage-0 reifiers) | Yan, via reifiers | 4 | 2 | 1 | 3 | 6 | covered by reifiers |
| 18 | Graded types | Granule, Idris 2 | R524 | Yan Meta lab | 1 | 2 | 4 | 4 | 5 | later |
| 19 | GADTs | Haskell, OCaml | R786 | Yan Meta lab | 2 | 2 | 2 | 4 | 4 | later |
| 20 | Specialization (overlapping impls) | Rust #31844 455 votes | gap | none | 3 | 2 | 1 | 5 | 3 | decline: unsound in Rust after 9 years |
| 21 | Higher-kinded types | Rust rfcs#324 390 votes | non-goal (no GATs, 26.2) | none | 3 | 2 | 1 | 4 | 4 | decline: readability for agents and newcomers |
| 22 | Full dependent types | Haskell (minor topic), Idris, Lean | gap | none | 2 | 2 | 2 | 5 | 1 | decline: contracts and size arithmetic cover the uses asked for |
| 23 | Procedural macros | Swift, Rust, Gleam (asked); Dart cancelled | non-goal | none | 3 | 1 | 1 | 4 | 2 | decline (proposal D1) |

Two items scored high on demand but are excluded: lazy imports and runtime type enforcement solve
problems of interpreted, gradually typed languages that Yan does not have.

## 4. Rules the pushback gives

| Pushback | Evidence | Rule for Yan |
| --- | --- | --- |
| Compiler-executed user code | Swift 2 to 4 times slower builds, 33-minute build; Dart macros cancelled Jan 2025; Zig, Go refuse | No user code at parse, format or IDE time (proposal D1, D3) |
| Two spellings in one surface | Go 2,007 to 223 for `if err != nil`; Python PEP 505 and PEP 736 rejected; JS cognitive overload 10.6% | Yan and Yan Agent keep one spelling per form; extra spellings live in Yan Meta with spelling notes |
| Heavy effect and capture syntax | OCaml typed effects lacked support; Scala capture-checking syntax objections | Effects reuse the `uses` and `\|-` capability mark; no new row syntax in Yan or Yan Agent |
| Typed effects feel like checked exceptions | OCaml thread | Infer effect sets inside a package; spell them only on public headers, where capabilities already appear (R210) |
| Complexity growth | Rust and C++ surveys; Haskell upgrades broke 27% | New core features enter through Yan Meta's lab and graduate with evidence (R777) |
| Invented syntax for LLMs | HN GlyphLang thread: token savings shrank from 45% to about 35%, models write familiar syntax better | Yan Agent stays Python-shaped; Yan Meta notations are never the agent surface |
| Unsound or stalled power features | Rust specialization stalled; PHP erased generics rejected 7 to 19 | Admit only features with a decidable, checkable model |

## 5. Recommendation

Best only. Build these seven, in this order, each through the existing research row or proposal.

1. **Hash-keyed incremental compilation (R528).** The top pain in every compiled-language survey.
   Yan's item hashes make it simpler here than anywhere else. It is a "make it fast" item, so it
   follows M1 and the native refusals work.
2. **Notations, DSL islands and reader lenses (Yan Meta proposal).** The novelty hook, built to
   avoid every measured macro complaint.
3. **Live programming on item hashes.** Hot reload and always-running evaluation keyed by item hash,
   with typed holes (21.2) and replay (7.6). After the REPL (M3). New to mainstream languages.
4. **Size arithmetic (R781)** and 5. **rank polymorphism (R780)**, together: they answer Rust's top
   type-system want and the array-language crowd at once, and make Yan the language where tensor
   shapes are checked for real.
6. **One-shot effect handlers (R522)**, spelled with the existing capability marks, inferred inside
   a package.
7. **Proof tactics (R784) with a vericoding loop** in the agent toolchain: verified AI-written code
   is the strongest AI-era argument for Yan.

Decline specialization, higher-kinded types, full dependent types and procedural macros.

Everything else in section 3 is worth doing when its row's turn comes, but none of it changes who
comes to Yan.

## 6. Open questions

- **Q1** Should incremental compilation (1) be pulled ahead of other "fast" work, given it is the
  most-cited pain in the sweeps?
- **Q2** Rank polymorphism and size arithmetic touch Yan Core; do they enter through Yan Meta's lab
  (R777) or straight into a core RFC?
- **Q3** Is live programming (3) a tooling feature only, or does it need a spec section for hot
  reload semantics (state migration between item versions)?
- **Q4** Anonymous unions (9) need a hashing rule for positional sums under K2; worth a study?

## Sources

Systems: Go #19412 https://github.com/golang/go/issues/19412; Go #32825 https://github.com/golang/go/issues/32825
and https://go.dev/blog/error-syntax; Go #35950, #51317, #9455; Go survey 2024 H2
https://go.dev/blog/survey2024-h2-results; Rust rfcs#323 https://github.com/rust-lang/rfcs/issues/323;
Rust #31844 https://github.com/rust-lang/rust/issues/31844; Rust #76560
https://github.com/rust-lang/rust/issues/76560; rfcs#324, rfcs#376, rfcs#655, rfcs#1215, PR#2584;
State of Rust 2025 https://blog.rust-lang.org/2026/03/02/2025-State-Of-Rust-Survey-results; Zig #16270,
#3806, #68, #23446, #2647 https://github.com/ziglang/zig/issues; ISO C++ survey 2026
https://isocpp.org/files/papers/CppDevSurvey-2026-summary.pdf.

Managed: Kotlin survey 2021 https://blog.jetbrains.com/kotlin/2021/12/kotlin-features-survey-2021-results/;
Kotlin features https://kotlinlang.org/docs/kotlin-language-features-and-proposals.html; C# 15 unions
https://devblogs.microsoft.com/dotnet/csharp-15-union-types/; State of JS 2025
https://2025.stateofjs.com/en-US/features/; Swift Evolution top
https://forums.swift.org/c/evolution/18/l/top; Swift macro build times
https://forums.swift.org/t/compilation-extremely-slow-since-macros-adoption/67921; TypeScript #202
https://github.com/microsoft/TypeScript/issues/202; Valhalla
https://en.wikipedia.org/wiki/Project_Valhalla_(Java_language); Dart macros cancellation
https://dart.googlesource.com/macros/+/refs/heads/main/README.md.

Scripting: Python typing surveys
https://engineering.fb.com/2026/10/27/developer-tools/python-typing-survey-2026-ai-tools-checker-landscape/
and https://engineering.fb.com/2025/12/22/developer-tools/python-typing-survey-2025-code-quality-flexibility-typing-adoption/;
PEP 810 https://discuss.python.org/t/pep-810-explicit-lazy-imports/104131; PEP 505 revival
https://discuss.python.org/t/revisiting-pep-505-none-aware-operators/74568; PEP 736
https://peps.python.org/pep-0736; PEP 638 https://discuss.python.org/t/pep-638-syntactic-macros/5352;
PHP pipe https://wiki.php.net/rfc/pipe-operator-v3; PHP partial application
https://discourse.thephp.foundation/t/php-dev-vote-partial-function-application-v2/4821; PHP generics
vote https://discourse.thephp.foundation/t/php-dev-vote-rfc-bound-erased-generic-types/5554; Elixir 1.20
https://elixir-lang.org/blog/2026/06/03/elixir-v1-20-0-released; Julia survey 2025
https://julialang.org/assets/2025-julia-user-developer-survey.pdf.

Functional and research: Haskell survey answers
https://blog.haskell.org/what-would-you-see-changed-in-haskell/; State of Haskell 2025
https://discourse.haskell.org/t/state-of-haskell-2025-results/13755; OCaml typed effects
https://discuss.ocaml.org/t/whats-the-status-of-typed-effects/18439; OCaml priorities
https://discuss.ocaml.org/t/next-priority-for-ocaml/12561/14; Jane Street locality
https://blog.janestreet.com/oxidizing-ocaml-locality/; Scala capture checking
https://contributors.scala-lang.org/t/experimental-capture-checking-new-syntax-for-explicit-capture-polymorphism/7095;
Modal effect types https://arxiv.org/pdf/2407.11816; Ask HN nice features
https://hn.nuxt.dev/item/42200056; Gleam survey https://gleam.run/news/developer-survey-2024-results.

Data and AI-era: PEP 646 and tensor typing
https://discuss.pytorch.org/t/feedback-on-pep-646-relevant-to-tensor-typing/111791; vericoding
https://arxiv.org/pdf/2509.22908; GlyphLang HN https://hn.nuxt.dev/item/46571166; marimo
https://marimo.io/blog/introducing-marimo; Uiua HN https://news.ycombinator.com/item?id=37673127; Mojo
wishlist https://github.com/modularml/mojo/discussions/1490.
