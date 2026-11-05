## 16. Names, projection and round-trip

### 16.1 Names are word lists

Yan stores every identifier as a sequence of lowercase ASCII words plus a class, `value` or `type`:

| Class | Yan Agent | Yan | Yan Core text |
| --- | --- | --- | --- |
| value | words joined by `_` | words joined by `_` | words joined by `_` |
| type | PascalCase | PascalCase | PascalCase |

PascalCase is split into words at each capital letter, which is why consecutive capitals are forbidden (E112). Digits attach to the preceding word. A `@capability` trait binds in `uses` under its value-class name (`HttpClient` is `http_client`).

### 16.2 Foreign names

Names that come from outside (C symbols, SQL columns, JSON keys, Python attributes) are stored verbatim with the flag `foreign` and printed unchanged by both surfaces. They are introduced only by extern blocks, `@export(name=...)`, schema declarations and the Python bridge. Within Yan, a foreign name that is not a valid Yan name is written in backticks.

### 16.3 The projection table

Some names and forms differ by surface on purpose, so each reads natively. The table is part of the edition (section 20.4), and the round-trip laws hold through it.

| Yan Core | Yan Agent | Yan |
| --- | --- | --- |
| `List`, `Map`, `Set` (insertion-ordered) | `list`, `dict`, `set` | `List`, `Map`, `Set` |
| `Deque`, `Heap` (min-heap) | `deque`, `heap` | `Deque`, `Heap` |
| `SortedMap`, `SortedSet` | `sorted_dict`, `sorted_set` | `SortedMap`, `SortedSet` |
| `Soa` | `soa` | `Soa` |
| `BigInt` | `bigint` | `BigInt` |
| `Str`, `Bytes` | `str`, `bytes` | `str`, `Bytes` |
| `dec128` | `decimal` | `dec128` |
| `i8` ... `i128`, `u8` ... `u128` | `int8` ... `int128`, `uint8` ... `uint128`; `int` | `i8` ... `i128`, `u8` ... `u128`; `int` |
| `f16`, `bf16`, `f32`, `f64`, `f128` | `float16`, `bfloat16`, `float32`, `float128`; `float` | `f16`, `bf16`, `f32`, `f64`, `f128`; `float` |
| `Option[T]` | `T \| None` | `T?` |
| inout parameter | `var p: T` | `var p: T` |
| inout argument | `&x` | `&x` |
| sink parameter | `take p: T` | `take p: T` |
| failure in a signature | `-> T fail E` | `-> T fail E` |
| reify a failure | the call used as a value | `try f(x)` |
| deep copy, alias | `x.copy()`, `x.share()` | `copy x`, `share x` |
| interpolation | `f"{x:.2f}"` | `"\(x:.2f)"` |
| closure | `lambda x: e` | `x -> e`, or the implicit subject `.f` |
| record | `class C:` with fields | `type C = { ... }` |
| sum | `type S = A(...) \| B()` (R423) | `type S = A(...) \| B()` (R371) |
| loop forever, while | `while True:`, `while c:` | `loop`, `while c` |
| cancel scope | `with deadline(t):` | `with deadline(t)` |
| `par for` | `par [e for x in xs]` for values, `par for x in xs:` for effects (R427) | `par for x in xs` |
| `comptime` | compile-time parameters only | `#for`, `#if`, `#expr` |
| generator | none | `gen f(...) -> T` |
| type alias | `type X = T` | `type X = T` (R371) |
| `Newtype` | `type N = newtype[T]` | `type N = newtype[T]` (R371) |
| a body that is one match on the parameters | a match body (R426) | clause form (R266, R370) |
| `become Call` | `become f(x)` | `become f(x)` |

### 16.4 Projection operations

| Command | Input | Output |
| --- | --- | --- |
| `yan view --as yn F` | a Yan Agent module | idiomatic Yan: `let` added, implicit-subject lambdas where a lambda is a single field-access chain, contracts and examples kept |
| `yan view --as yag W` | a Yan module | canonical Yan Agent, for every item within the Yan Agent subset |
| `yan view --as yir X` | either surface | Yan Core text |

**Projection as composition (R559).** Each surface has six steps: parse (text to lossless tree), abstract (tree to surface AST), lower (AST to HIR, with spelling notes and the name table), and their inverses print, resugar and the shared raise (Yan to HIR). Each surface has exactly one canonical printer, over its own AST, and both `yan fmt` and `yan view` use it: `fmt F` is print(abstract(parse(F))), and `view --as S X` is print(resugar(raise(elaborate(X)))). The laws of 16.5 are statements about these compositions. Raise restores written names from the name table and removes what elaboration inserted (R565).

Items outside the Yan Agent subset are reported, not dropped. The report names the first construct that excludes each item (for example "uses `Ref` (L4)"). `view` takes `--edition`. Yan Agent and Yan reject shadowing (E230), so projection between them renames nothing. A projection from Yan Meta, which admits shadowing (R521), gives each shadowed local a fresh name from the name table (RFC 0001 section 6).

### 16.5 Round-trip laws

These MUST hold within each edition and MUST be tested on every conformance program (section 24):

1. For any canonical Yan Agent module F: `yag(elaborate(F)) == F`, byte for byte.
2. For any canonical Yan Agent module F: `yag(yn(F)) == F`, byte for byte.
3. For any Yan module W within the Yan Agent subset: `elaborate(yag(W)) == elaborate(W)` as Yan Core, compared by item hash.
4. For any module X in any surface: the item hashes of `elaborate(X)` do not depend on formatting, comments or item order.
5. For any Yan module W: `yn(elaborate(W)) == fmt(W)`, byte for byte, comments kept at their items. A function whose whole body is one `match` on its parameters prints in clause form (15.3, R266), so this law keeps one printing per tree.
6. For any canonical Yan Meta module F: `ymt(elaborate(F), spelling(F)) == F`, byte for byte, comments kept at their items; and every Yan Core item has a canonical Yan Meta printing with no spelling notes (R518).

**Trivia through Yan Core.** Yan Core carries seven hash-neutral kinds of trivia, so laws 1, 2, 5 and 6 hold byte for byte: comments and docstrings; blank lines; literal spelling (18.5); local names (3.6, R519); Yan Meta spelling notes, which record which of several Yan Meta spellings produced a node (16.6, R518); origin marks, which record that elaboration inserted a node (a capability argument, a copy, a move or a conversion) rather than the author writing it (R565); and spelling marks, which record a choice the author made among spellings of one tree (R838). Trivia is kept in one side table per module, keyed by item hash and node path, and is never hashed. A note whose spelling no longer matches its node after an edit is dropped, and the default spelling prints. Comments attach as leading (on their own line, to the next node), trailing (on the same line, to the previous node) or dangling (in an empty construct or before a closer).

**Spelling marks (RFC 0034).** Elaboration of every surface writes spelling marks; no surface lowering does. A node is named by its item hash and a node path from the one walker in `yan-core` (`yan_core::path::walk`), which elaboration and raise both use (R837). Parameter k of a function is `[0, k]`, contract k is `[1, k]`, body statement s is `[2, s]` and, in the b-th nested block of a statement, `[2, s, b, s']`; function f of an `impl` is walked under the prefix `[3, f]`. The call of a statement is its path plus `0`, argument k of a call is the call's path plus `k`, a literal atom is its parent's path plus `0`, and field k of a named constructor is its statement's path plus `0, k`. A closure's statement s is its statement's path plus `0, s` (R862). Elaboration records a mark against the HIR span and kind of a node and moves it, once the item is built, to the first node in walk order with that span and kind; a span is this join key only and is never stored.

The marks are a closed set, and adding one is a row (R838). Each records a fact, never source text:

| Mark | On | Records |
| --- | --- | --- |
| `Keyword`, `Positional` | call argument, constructor field | passed by keyword, or by position where a keyword was allowed |
| `Method`, `Function` | call | written `x.f(..)`, or `f(x, ..)` |
| `Annotated` | `let` | the binding had a written type |
| `FromPre`, `FromPost` | `assert` | elaborated from a `pre` or `post` contract |
| `Out` | name atom | the name was `out` in a `post` contract |
| `Derived(trait)` | `impl` | generated by `@derive(trait)` |
| `Prove` | first item of the module | the module had `@@prove` |
| `DocAbove` | parameter `Doc` node | the doc was written above the parameter, not in an `Args:` entry (RFC 0035, R846) |
| `Const` | statement or atom | an inlined std constant (`math.pi`, `math.e`, `math.inf`, `math.nan`) was written by name (RFC 0036, R850) |
| `Negated` | float multiplication or int subtraction | written as unary `-x`, lowered to `-1.0 * x` on a float or `0 - x` on an int (R851) |
| `Qualified` | constructor call | a std variant was written module-qualified, as `math.Up()` (R852) |
| `Lenient(code)` | call, atom, or first statement of a rewrite | a lenient reader's rewrite, named by its N-code, produced the node or the statements up to the binding the alias uses (R853, R858) |
| `Operator` | call | a power was written with `**`, lowered to `int_pow`, `bigint_pow` or `std.math.pow`/`pown` (R856); on the first `std.text.concat` of a chain, the chain was written with `+`, not as an f-string (RFC 0037, R857) |
| `Explicit` | loop statement or `std.prelude.range` call | an argument equal to the parameter's default was written, as the `0` of `range(0, n)` (R859) |
| `Parallel` | first snapshot binding | k targets were assigned together, as `a, b = b, a` (R860) |
| `Prelude` | tuple or power call | the node is the expansion of a prelude `divmod` or `pow` call (R861) |
| `Hoisted` | declaring `let` | elaboration declared the name before an `if` or `with` that binds it on every path (RFC 0038, R863) |
| `View` | parameter or result type | a view type was written, as `View[int]` (R864) |
| `Schema(type)` | call | `cli.parse_args[T]` or `cli.parse[T]()` was written, lowered to a schema literal (R865) |
| `Comprehension` | binding statement | a comprehension was lowered to an empty collection and an appending loop (R866) |
| `Parenthesized` | binding statement of a tuple | the tuple was written in parentheses (R868) |
| `Example(text)` | item | an `ex` line of a Yan Agent function, as written; hash-neutral (R867) |
| `Lemma(text)`, `GhostParam(k, text)`, `GhostStmt(k, text)`, `GhostContract(k, text)` | item | what elaboration erases in a proof, as written on one line or, for a lemma, as its header and indented lines: a lemma (it rides on the next item elaboration lowers), a ghost parameter, a top-level ghost statement, and a `pre` or `post` over ghost values; `k` is the position among the written parameters, statements, or contracts other than `ex`; hash-neutral (R870) |
| `FlagsKeyword`, `FlagsPositional` | call | Python `re` flags were written on a `re` call lifted to `regex.compile` (N40), as `flags=re.I` or by position; the letters come from the inline flags of the pattern literal, printed as `re.I`, `re.M`, `re.S`, `re.X` joined by `\|` (R869) |

Literal spelling stays the existing literal trivia (18.5), valid only if it denotes the node's value; it covers float, string and char literals as well as integers (R855). Raise removes a widening conversion that carries an origin mark, as it does any inserted node (R854). An item with a name-table entry came from source, so for it raise reads marks as facts: an argument without `Keyword` is positional, a `let` without `Annotated` prints no type (R839). For an item without an entry (hand-written `.yir`, `yan-opt` output, a patch) raise uses its heuristics. Raise checks each mark against its node before use and drops one that fails, so the default prints (R840): `Keyword` needs a non-positional-only parameter of that name, `Method` needs a receiver, a literal spelling must parse to the node's value, `Derived(t)` needs an impl of `t` for the item, `Operator` on a concat chain needs every part to be a `str` operand with no format spec, `Explicit` on a range needs the literal start `0`, `Parallel` needs k snapshot bindings followed by k assignments to the targets, and `Prelude` and `Lenient(code)` need the call's expansion or rewrite to reproduce the node. Without `Parallel`, raise keeps the snapshot bindings, so a swap never prints as two sequential assignments (R860). `raise` takes the trivia table, restores the contracts, the decorators and the `out` use the marks name, and returns the marks it used. `yan view --as yag --strict` makes a mark that raise neither used nor validly dropped an error, E279, naming the mark, item and path (R841). Marks live from elaboration to raise within one load; they are not stored beside `.yir` text and are not kept across an edit (R843).

**Sub-item docs (RFC 0035, R844).** A doc on a parameter, field, variant, associated type or literal entry is one `Doc` trivia node, hash-neutral like an item doc (3.6), keyed by item hash and a node path from the same walker; adding or removing a doc at any position leaves every item hash unchanged. The walker gains these paths, rooted at the item: field k of a record or class `[0, k]`; variant k of a sum `[0, k]` and field j of that variant `[0, k, j]`; associated type k of a trait `[0, k]`; entry k of a dict, list or tuple literal at literal path p is `p` then `k`, where a dict entry (key and value) is one node. A parameter keeps `[0, k]`. A doc written in an `Args:` section and one written above the parameter elaborate to the same node; the `DocAbove` mark records the second, and a node without it prints in the section (R846).

**Raise (R565).** Raise removes every node that carries an origin mark. For Yan Core text without marks, such as hand-written `.yir` files, raise removes a node of an inserted kind only if elaborating the result inserts it again at the same place, so the raised item elaborates to the same Yan.

**Checking the laws (R564).** `tools/conform.py --laws` checks every law that applies (laws 1 to 6, and N7 for each niche surface) on every conformance program, with no extra case files. CI reports the results once the printers from Yan exist and gates on them from M3. `conformance/roundtrip/` holds cases that pin spellings the suite does not otherwise reach.

Law 1 also has a bare companion for every case it runs on: raise with an empty trivia table, print, elaborate the result, and compare item hashes with `elaborate(F)`; `yan view --bare` produces the raised text. A case that passes law 1 and fails the companion shows a mark covering for a raise bug, and fails law 1 (R842). `tools/conform.py --laws` runs law 1 in strict mode and runs the companion.

**Item identity.** An item's hash is defined in section 3.6. Two items with the same hash are the same item, whichever surface they came from.

### 16.6 Yan Meta: the lab and canary surface

Yan Meta (言元; 元语言, "metalanguage") is the dense ASCII surface and the lab, kitchen sink and canary of the language (R516, R776): where Yan and Yan Core try syntax and features before committing to them. Files use the extension `.ymt`. It covers layers L0 to L6. Its print-only view, `yan view --as ymt`, ships in M3 beside the Yan printer, printing any Yan Core item with no spelling notes (R563); its reader, lenient reader and spelling-note round trip are built in M4. Any Yan Core item prints as Yan Meta. Yan Meta adds only syntax (law 1): every Yan Meta module elaborates to the same Yan Core items, with the same hashes, as its Yan projection. Where Yan Meta leaves a conversion, a `None` or a loop implicit in its text, the Yan Core text is explicit (law 5). The full reference, with the lexicon, precedence, grammar, spelling-note model and one entry per form, is RFC 0001 (`rfcs/0001-yan-meta.md`); it moves into this section before the Yan Meta build starts.

**Admission (R517).** A Yan Meta form is admitted only if it passes all five rules:

1. **K1 Same core (R768).** It lowers to Yan Core: the core, or an accepted Yan Core extension (26.6). Yan need not have idiomatic syntax for it; Yan prints it as 15.4 says. A form that needs an extension is rejected with a diagnostic naming it until the extension lands.
2. **K2 No invented hashed names.** A lowering never creates or drops a name the item hash includes (3.6).
3. **K3 Recognisable.** The printer has a decidable recogniser for the lowered shape, and the grammar is LL(1) after the four factorings of RFC 0001 section 4.
4. **K4 Spelling kept like comments.** Where several spellings elaborate to one tree, the author's choice is a spelling note (16.5): `ymt(elaborate(F), spelling(F)) == F`. A tree with no notes prints with each shape's default.
5. **K5 Five facts visible.** Each of the five facts (law 4) keeps a mark in the text. A form may move a mark to the definition (variadic parameters, properties) but never drop it.

**Lifecycle (R777).** Every Yan Meta form carries a stage in RFC 0001: `experimental`, `stable`, `graduated` or `retired`. Forms accepted on 2026-11-01 are `stable`; a new form enters `experimental`. A form becomes `stable` by an accepted row, once it appears in the std or examples corpus and law 6 of 16.5 holds for it over the whole suite with and without spelling notes. It `graduates` when Yan or Yan Agent adopts its spelling, or the extension behind it lands in Yan Core, with a probe on the R566 harness showing models read and write it and a reviewer reading its `yan view --as yn` print without the note; Yan Meta keeps reading it. A `retired` form is withdrawn within the preview window of 20.6, and until then `yan fmt` rewrites each use to the default spelling of the same tree, so no item hash changes. The forms that need no core change (RFC 0029 D3, R779) are RFC 0001's "Yan Meta additions", all `experimental`; the forms that need one wait on 26.6 (R780 to R786 and the rows before them).

**Lab opt-in (R778).** A module that uses an experimental form names it in a header line, `@lab(trains, defer)`; use without the line is E001, naming the form and the line to add. `@lab` is a spelling note (16.5) and hash-neutral. An experimental form is a preview (20.6): outside the compatibility promise, and a published package at 1.0 or later may not contain a `@lab` line (E002).

**Keyboard rule (R520).** Every Yan Meta token outside string literals and comments is ASCII, so all of Yan Meta can be typed on an ordinary keyboard. Unicode is allowed in strings and comments, as in every surface (R338). Operator glyphs outside ASCII are not admitted, even as aliases.

**Open admission (R521).** Any form that makes Yan Meta more expressive is admitted if it passes K1 to K5. Yan Meta therefore admits forms that Yan Agent and Yan leave out: shadowing, `where`, for-else, `goto` to join points, parallel assignment, try/catch over typed failures, `defer`, null as `None`, lossless implicit widening, `extends` as delegation (no subtyping), `async`/`await` as branch and join inside `par concurrent`, file-local operator fixity, tacit trains and the rest of RFC 0001's catalogue. Forms that need an accepted extension (macros, `@memo`, effects with resume, symbols, `any`, `recover`, `atomic`, `eval`) wait for it. Out: operator glyphs outside ASCII (R520), juxtaposition calls and implicit multiplication (ambiguous, law 3), global monkey patching (law 6) and multi-shot continuations.

**Core spellings.** `:=` binds, `<-` assigns, `=` is equality, prefix `~` marks mutation, `!` in a type and `!!` in code mark failure, `|-` lists capabilities, `|>` is the pipeline, `e |?` opens a match, `||` and `|||` are `par` and `par concurrent`, and `<~` sends and receives on channels. Precedence comes from an operator's characters, so a file parses before any import is resolved; a file may declare fixity for operators it defines or imports, relative to an existing operator, for that file only.

### 16.7 Planned niche surfaces (R536 to R550)

A niche surface is a small, domain-shaped language over Yan. It may be a spelling of a subset of Yan, or it may give its items semantics of their own (R568). None is part of 1.0. The reference for each is RFC 0002; each enters this section normatively through its own RFC, with its grammar, subset predicate, projection column (16.3), diagnostic codes and conformance column.

**Admission (R536).** A niche surface is admitted only if it meets all nine rules:

1. **N1 Semantics (R568, R768).** It lowers to Yan Core, the core or an accepted Yan Core extension, which Yan prints as 15.4 says; or it defines semantics of its own under the boundary rules below. Its RFC says which, per form.
2. **N2 Subset with a checker.** A decidable predicate says which items print in the surface; outside it, `yan view --as <name>` reports the first excluding construct, as 16.4 does for Yan Agent.
3. **N3 One printing.** Law 8 holds as for Yan Agent and Yan. Sugar is recovered from the Yan Core tree, from stage-0 metadata attached by the lowering, or from the trivia of 16.5.
4. **N4 Same item hash.** A lowering never creates or drops a hashed name (3.6); generated names follow a fixed rule.
5. **N5 Five facts visible.** Each of the five facts (law 4) keeps a mark in the text.
6. **N6 Crate.** `yan-<name>` holds the lexer, parser, AST and lowering into `yan-hir`, and touches nothing below it. A surface engine (R568), if the surface has one, is a separate crate `yan-<name>-engine` or a std package, at the runtime layer beside `yan-rt` (R576).
7. **N7 Round-trip laws.** For surface S: `S(elaborate(F)) == F` byte for byte for canonical F, and `elaborate(S(W)) == elaborate(W)` by item hash for any Yan W in the subset.
8. **N8 Names and diagnostics.** A column in 16.3 for names it spells differently; messages render per surface (R297); new codes only for its own syntax.
9. **N9 Files and lexis.** Extension `.<name>`, one surface per file, any mix per package, calls across surfaces by name; lexical rules as 13.1.

| Row | Surface | Extension | Job | Lowers to | Waits for | Track |
| --- | --- | --- | --- | --- | --- | --- |
| R537 | Yan Schema 言契 | `.ysd` | wire schemas and remote interfaces with version history | `derive(Wire)` types, refinements, `@remote` traits, R406 converters | streams over `Remote` (R555) for stream methods | practical |
| R538 | Yan Prompt 言询 | `.ypm` | typed model programs | `model.generate_chat` and `validate` (17.4) | none | practical |
| R539 | Yan Query Language 言查 | `.yql` | typed table queries | `LazyFrame[R]` plans (11.9) | none | practical |
| R540 | Yan Shell 言行 | `.ynsh` | capability-safe shell scripts | line iterators, rooted handles, argv-only `proc` (R448) | none | practical |
| R541 | Yan Workflow 言序 | `.ywf` | durable workflows | `@durable` functions (R416, R453), durable timers (R552) | `@yan/durable` (M4) | practical |
| R542 | Yan Infra 言基 | `.yinf` | infrastructure with plan and apply | resource capabilities with `Deferred[T]` results | dry-run handler class (R554) | practical |
| R543 | Yan Binary Format 言码 | `.ybf` | binary layouts with checked parsers and writers | view-backed records over `View[u8]`, `@layout(bits)` for LSB-first registers | none | practical |
| R544 | Yan Dataflow 言川 | `.ydf` | synchronous dataflow on the embedded profile | state records and step functions under `executor` or `srp` (R323) | none | practical |
| R545 | Yan Delegate 言代 | `.ydl` | contracts whose bodies are generated by the agent loop and pinned by hash | functions with pinned bodies; Yan Agent holes (R212) before a body exists | none | research |
| R546 | Yan Probabilistic 言随 | `.ypb` | probabilistic models with inference as handlers | a `Prob` capability in `@yan/prob` over seeded `rand` | `grad` (11.5) for gradient samplers | research |
| R547 | Yan Protocol 言礼 | `.ypc` | session-typed protocols | `@linear` state types (R443) over `std.chan` | none | research |
| R548 | Yan Spec 言规 | `.ysp` | specs checked against actors by trace refinement | `std.sim` models and replay checks (17.7) | none | research |
| R549 | Yan Law 言法 | `.ylaw` | statute-shaped rules with default logic | `uses none` functions over `Money` (11.7) | none | research |

**Own semantics (R568).** A niche surface is exempt from law 1. Inside its items it may define behavior that Yan cannot express, such as three-valued logic, inference as evaluation, or clocked time. These rules still bind it:

1. **Boundary signature.** Every item another surface can call has a signature in Yan Core types, with `fail` and `uses`. Calls across surfaces go through that signature only, and checked Yan Core sees the item as that signature.
2. **Laws kept.** Laws 2 (safe by default), 4 (five facts), 6 (local reasoning) and 7 (deterministic by default) hold under the surface's own semantics. Effects happen only through capabilities named in `uses`, so capability ceilings, `nondet` and record and replay apply unchanged.
3. **Two routes.** A form either lowers to checked Yan Core, which keeps N4, N7 and the one back end, or runs on a surface engine: an interpreter or compiler in `yan-<name>-engine` or a std package, linked to its boundary signature as an extern item is (section 12). N6 allows the engine crates. Lowering is preferred where it can express the form.
4. **Engine items.** For an item on an engine, N2, N3 and N7 apply to its boundary signature only. `yan view --as yn` and `--as yag` print the signature and report the body as "<name> semantics (R568)". The item hash covers the canonical surface text and the engine's identity hash (R348).
5. **Conformance.** The surface's conformance column tests its semantics, including determinism under two hash keys (M0), and every engine item passes `yan test --ub`.

**Order (R550).** No niche surface is built before 1.0. After 1.0, Yan Schema comes first, then Yan Prompt, then Yan Query Language and Yan Shell; Yan Workflow and Yan Infra follow the items they wait for. Each surface ships first as a print-only view, `yan view --as <name>`, before it has a reader.
