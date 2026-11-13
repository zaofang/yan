# Yan Meta: syntax proposal after forum review

Proposal, 2026-11-13, against main at `9c06862`. Exploratory (spec freeze #1025): no rows, no RFC.
Supersedes the recommendation in section 7 of the
[user-defined syntax study](2026-11-13-yan-meta-syntax-extension.md); the study keeps the prior art,
models and examples. Items are D1, D2; questions Q1, Q2.

## 0. The proposal in one paragraph

Drop procedural macros entirely. Keep user syntax, but only as **declared, bidirectional notations**:
each rule both parses and prints, runs no user code, and is resolved before name resolution.
Islands carry compile-time-checked DSLs. Because every rule prints as well as parses, and Yan hashes
items by their core form with spelling as trivia, notations can also be **lenses**: a reader can view
anyone's code through their own notations without changing the file or its hash. That last part is
the bleeding edge; no shipping language does it.

## 1. What the forums say

Sources are listed at the end. Figures are as published.

### 1.1 Pushback on macros (strong, specific, about procedural macros)

| Source | What users said |
| --- | --- |
| Swift forums, "Compilation extremely slow since macros adoption" (2023 to 2025) | Incremental builds 2 to 4 times longer; one xcframework build went from about 4.3 to 33.4 minutes with one macro; teams dropped macros over build time. "I don't think it makes sense for a core language feature to take longer to compile than the actual project itself." Root cause: macros are compiled plugins on swift-syntax. |
| rust-analyzer, "IDEs and Macros" (2021) | Macros break IDEs through irreversible transformations, name resolution that must run before expansion, unbounded execution ("User-written macros can crash. IDEs must not crash.") and non-determinism. IDE-friendly design: append-only meaning, macros known up front with a distinct invocation, file-local, deterministic, bounded. |
| Rust internals, grammar information for proc macros (2024) | Users want `json!`, `rsx!` and similar custom-syntax macros to format, highlight and complete; tools cannot, because proc macros declare no grammar. Formatter maintainers refuse to run macro code during formatting. |
| Rust compiler performance survey (2025) | Derive proc-macro expansion is not cached; the team hopes new language features remove the need for some proc macros and build scripts. |
| State of Rust surveys (2024, 2025) | Compile time and resource use remain the top pain; worry that the language grows too complex persists. |
| Python, PEP 638 "Syntactic macros" (2020, not accepted) | Objections: non-Pythonic dialects, less readable code, new syntax elements, string-in/string-out is not really syntactic. |
| Zig | No macros by design; comptime instead. Users miss generated declarations and mixins, not new syntax. |

### 1.2 What users ask for

| Source | Most wanted |
| --- | --- |
| Kotlin features survey (2021) | union types and multicatch 45%, collection literals 32%, multiple receivers 30% |
| C# (csharplang #75 and the type-unions proposal) | discriminated unions, the longest-running top request, now planned |
| State of Rust (2025) | generic const expressions and better trait methods |
| TC39 and State of JS | pattern matching and the pipeline operator |
| r/ProgrammingLanguages wish lists, HN effect threads | algebraic effects, refinement types, structural records, datatype-generic programming, "macros" without detail |
| Lean 4 users | metaprogramming called "unsurpassed", but the syntax for writing it called noisy; open questions on parse-error quality |
| Rust users (sqlx, rsx, json) | DSLs checked at compile time, with editor support inside them |

### 1.3 Reading

The pushback is not against new syntax. It is against **running user code in the compiler and the
IDE**: build time, crashes, broken completion, unformattable code, irreversible expansions. Every
complaint above disappears when a rule is declarative, bounded, deterministic, known before name
resolution and invertible. The wants that touch syntax are DSLs with editor support and notation for
mathematics and data. Most other wants (unions, pattern matching, pipelines, effects, refinements)
Yan Core or Yan Meta already has or has accepted for research; they need no user syntax.

## 2. Recommendation

### D1 No procedural macros, ever

No `elab`, no token-stream or AST-to-AST functions, no reader macros, no per-file readers. RFC 0001
entry E2 (`macro` over `Code[T]`) is withdrawn as a syntax feature; typed staging (R525) stays a
code-generation feature for values, not a way to define syntax. Derives stay on reflection and
`derived` bodies (10.4).

Why: this is where all of the measured pain comes from (Swift build times, rust-analyzer crashes,
rustfmt refusing macros), and Yan's guarantees (determinism, capability safety, reproducible tooling)
would have to be re-proven for each macro.

### D2 Notations: declared, bidirectional, total

A `notation` is a rule over the fixed shape tree: a pattern with typed holes and a template. One
rule gives the parser, the printer and the round-trip law.

```
pub notation argmax
  syntax  "argmax" b:binder "in" xs:expr(tier: range) "by" key:expr[b]
  means   xs.max_by(\b -> key)
```

- **No user code runs.** Expansion is template substitution with a depth budget.
- **Bidirectional.** The printer is the template read backwards. `ymt(elaborate(F), spelling(F)) == F`
  holds for every notation with no extra code, so `yan fmt`, `yan view`, diff and patches work.
- **Append-only.** A notation cannot change the meaning of text that parses without it; its leading
  word or operator must be new in the importing file.
- **Known up front.** `use syntax m.{a, b}` names every notation; they resolve before ordinary name
  resolution and are file-local. This is the rust-analyzer list item for item.
- **Holes are typed by syntax class** with a description, so the error is "expected a place, such as
  `x` or `a[i]`", and binders declare their scope (`expr[b]`), so hygiene is checked once, at the
  definition.
- **Facts declared.** A notation states the capabilities and failures its expansion adds; it can
  require a capability, never grant one.
- **Operators are notations.** Infix and mixfix operators are notations whose precedence is "like",
  "above" or "below" an existing operator. No numeric levels.

### D3 Islands: DSLs with editor support

An island is a block or prefixed literal whose body follows a declared `grammar`. The grammar is
LL(1), checked at definition, and bidirectional like a notation. Its action is a template (for small
languages) or a stage-0 function from std or the package, under stage-0 budgets and declared
capabilities (`embed` to read a schema, for example).

```
use syntax std.sql.{sql}

top(db: Db, n: int) -> [Row] ! DbErr |- db :=
  sql db
    select name, total from orders where total > {n} order by total desc
```

The grammar gives the editor highlighting, completion and formatting inside the island, which Rust's
`json!` and `rsx!` users ask for and cannot get. Splices (`{n}`) are typed and cannot capture island
names. The same grammar form gives runtime parsers and printers for data formats (R527 `std.syntax`),
so there is one grammar language in Yan.

The stage-0 action is the one place user code runs, and only to compute a value from an already
parsed tree. Parsing, printing, highlighting and formatting never run it, so the IDE and the
formatter stay pure. This is the line Rust's formatter maintainers drew.

### D4 Lenses: your syntax, my file

A lens is a set of notations a reader turns on for viewing:

```
yan view --lens ~/.yan/lenses/math.ymt src/solver.yn
```

The printer tries the lens's notations first. Because notations are bidirectional and items are
hashed by their core form, the view is exact and edits made through it map back: the editor
re-elaborates the edited text and patches the item by hash. The file on disk, its hash and every
other reader are untouched. Teams can share a lens; nobody has to adopt it.

This answers the oldest objection to user syntax ("every codebase becomes its own dialect", PEP 638,
Rust, Lean) by making dialects personal. Prior art: Unison stores code by hash with names as
metadata, so renames are free; Hazel's livelits fill typed holes with live views. Neither lets a
reader choose a notation for someone else's code. Yan can, because spelling notes (16.5) already
separate spelling from meaning.

### D5 Lab discipline

Every notation, island and lens rule is `experimental` under `@lab(notations)` (R777, R778). A
published 1.0 package may define and export notations only after the mechanism is `stable`. Yan and
Yan Agent keep fixed grammars; they can read and print any code through `yan view` with lenses but
never accept a notation in source.

### D6 Required tooling with the first notation

`yan view --expand`, `yan view --as yn` and `--lens` ship with the first notation. A notation with no
working expansion view is not admitted.

## 3. What the platform gives up

| Cost | Size |
| --- | --- |
| Spec: 26.2 non-goal becomes "no syntactic macros in Yan Core, Yan Agent or Yan; Yan Meta notations only, declarative" | one row |
| Spec: 16.6 "a file parses before any import is resolved" holds for the shape layer; `use syntax` lines resolve first | one sentence |
| Reader: notation table lookup after the shape reader | already the 2026-11-04 plan (P5, mix 8.1) |
| Printer: template recognisers | the same recogniser machinery spelling notes use |
| New: grammar checker (LL(1)) and island highlighter | one module each |
| Not built: plugin processes, sandboxes for user code in the IDE, macro caches | avoided by D1 |

Nothing changes in Yan Core, the checker, hashing, Yan or Yan Agent. Timing: after the Yan Meta reader
(M4), after the freeze lifts.

## 4. Why language people will come for it

- **Lenses** are new. A working `yan view --lens` demo on real std code is the hook.
- **Notations that are proven invertible** answer the formatting and IDE complaints Rust and Swift
  users have about macros.
- **Typed DSL islands with editor support** are what sqlx, rsx and json! users want and cannot get.
- **No macro tax.** Builds and IDEs never run user syntax code, the same promise that keeps Zig users
  happy, while still allowing new syntax.
- The rest of the nerd list is already on Yan's track: sum types and pattern matching (core),
  pipelines (Yan Meta `|>`), effect handlers with one-shot resume (research), auto-active
  verification (core), typed staging (research).

## 5. Open questions

- **Q1** Should island actions be limited to templates plus std actions (no package stage-0 code)
  until a package-level review exists? Leaning no: stage-0 is already budgeted and capability-checked.
- **Q2** Lens storage: per user only, or also a project file (`lenses/`) that teams opt into?
- **Q3** Should Yan Agent output ever be shown through a lens to a model? Default no; R566 probe first.
- **Q4** Does an edit through a lens record the lens's spelling in the file's notes, or always write
  the file's own spelling? Leaning: the file's own spelling.

## Sources

- Swift forums, "Compilation extremely slow since macros adoption": https://forums.swift.org/t/compilation-extremely-slow-since-macros-adoption/67921
- Swift forums, "Macro adoption concerns around SwiftSyntax": https://forums.swift.org/t/macro-adoption-concerns-around-swiftsyntax/66588
- rust-analyzer, "IDEs and Macros": https://rust-analyzer.github.io/blog/2021/11/21/ides-and-macros.html
- Rust internals, grammar information for proc macros: https://internals.rust-lang.org/t/discussion-adding-grammar-information-to-procedural-macros-for-proper-custom-syntax-support-in-the-toolchain/21496
- Rust compiler performance survey 2025: https://blog.rust-lang.org/2025/09/10/rust-compiler-performance-survey-2025-results
- 2025 State of Rust survey: https://blog.rust-lang.org/2026/03/02/2025-State-Of-Rust-Survey-results
- PEP 638 discussion: https://discuss.python.org/t/pep-638-syntactic-macros/5352
- Ziggit, "Things Zig comptime won't do": https://ziggit.dev/t/things-zig-comptime-wont-do/9719
- Kotlin features survey 2021: https://blog.jetbrains.com/kotlin/2021/12/kotlin-features-survey-2021-results/
- csharplang discriminated unions: https://github.com/dotnet/csharplang/issues/75
- HN on Lean 4 metaprogramming: https://hn.nuxt.dev/item/48763291
- HN on control structures and algebraic effects: https://hn.svelte.dev/item/45798068
- Unison, "The big idea": https://www.unison-lang.org/docs/the-big-idea
- Livelits (TyDe 2019): https://icfp19.sigplan.org/details/tyde-2019-papers/12/Livelits-Filling-Typed-Holes-with-Live-GUIs

## Decisions (lead cards, 2026-11-13)

All as recommended. Direction for the RFC after the freeze (#1025) lifts; no rows yet.

- Proposal D1 to D6 adopted.
- 26.2 and 10.4 to be amended: no syntactic macros in Yan Core, Yan or Yan Agent; Yan Meta admits declarative notations only.
- Imports: explicit `use syntax m.{a, b}` lines, resolved before other imports; the shape layer still parses with no imports.
- Use-site marks: islands only (head word or literal prefix); word notations carry no mark.
- Grammars: one grammar language, also giving runtime parsers and printers for data (R527 `std.syntax`).
- Templates are written in Yan Meta.
- Exports: none from published packages until the mechanism is `stable`.
- Shadowing: none; a clash is an error at the `use syntax` line.
