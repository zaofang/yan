## 16. Names, projection and round-trip

### 16.1 Names are word lists

Yong stores every identifier as a sequence of lowercase ASCII words plus a class, `value` or `type`:

| Class | Jian | Wen | Yong text |
| --- | --- | --- | --- |
| value | words joined by `_` | words joined by `_` | words joined by `_` |
| type | PascalCase | PascalCase | PascalCase |

PascalCase is split into words at each capital letter, which is why consecutive capitals are forbidden (E112). Digits attach to the preceding word. A `@capability` trait binds in `uses` under its value-class name (`HttpClient` is `http_client`).

### 16.2 Foreign names

Names that come from outside (C symbols, SQL columns, JSON keys, Python attributes) are stored verbatim with the flag `foreign` and printed unchanged by both surfaces. They are introduced only by extern blocks, `@export(name=...)`, schema declarations and the Python bridge. Within Wen, a foreign name that is not a valid Wen name is written in backticks.

### 16.3 The projection table

Some names and forms differ by surface on purpose, so each reads natively. The table is part of the edition (section 20.4), and the round-trip laws hold through it.

| Yong | Jian | Wen |
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
| `yong view --as wen F` | a Jian module | idiomatic Wen: `let` added, implicit-subject lambdas where a lambda is a single field-access chain, contracts and examples kept |
| `yong view --as jian W` | a Wen module | canonical Jian, for every item within the Jian subset |
| `yong view --as yong X` | either surface | Yong text |

**Projection as composition (R559).** Each surface has six steps: parse (text to lossless tree), abstract (tree to surface AST), lower (AST to HIR, with spelling notes and the name table), and their inverses print, resugar and the shared raise (Yong to HIR). Each surface has exactly one canonical printer, over its own AST, and both `yong fmt` and `yong view` use it: `fmt F` is print(abstract(parse(F))), and `view --as S X` is print(resugar(raise(elaborate(X)))). The laws of 16.5 are statements about these compositions. Raise restores written names from the name table and removes what elaboration inserted (R565).

Items outside the Jian subset are reported, not dropped. The report names the first construct that excludes each item (for example "uses `Ref` (L4)"). `view` takes `--edition`. Neither surface allows shadowing (E230), so projection renames nothing.

### 16.5 Round-trip laws

These MUST hold within each edition and MUST be tested on every conformance program (section 24):

1. For any canonical Jian module F: `jian(elaborate(F)) == F`, byte for byte.
2. For any canonical Jian module F: `jian(wen(F)) == F`, byte for byte.
3. For any Wen module W within the Jian subset: `elaborate(jian(W)) == elaborate(W)` as Yong, compared by item hash.
4. For any module X in any surface: the item hashes of `elaborate(X)` do not depend on formatting, comments or item order.
5. For any Wen module W: `wen(elaborate(W)) == fmt(W)`, byte for byte, comments kept at their items. A function whose whole body is one `match` on its parameters prints in clause form (15.3, R266), so this law keeps one printing per tree.
6. For any canonical Fei module F: `fei(elaborate(F), spelling(F)) == F`, byte for byte, comments kept at their items; and every Yong item has a canonical Fei printing with no spelling notes (R518).

**Trivia through Yong.** Yong carries six hash-neutral kinds of trivia, so laws 1, 2, 5 and 6 hold byte for byte: comments and docstrings; blank lines; literal spelling (18.5); local names (3.6, R519); Fei spelling notes, which record which of several Fei spellings produced a node (16.6, R518); and origin marks, which record that elaboration inserted a node (a capability argument, a copy, a move or a conversion) rather than the author writing it (R565). Trivia is kept in one side table per module, keyed by item hash and node path, and is never hashed. A note whose spelling no longer matches its node after an edit is dropped, and the default spelling prints. Comments attach as leading (on their own line, to the next node), trailing (on the same line, to the previous node) or dangling (in an empty construct or before a closer).

**Raise (R565).** Raise removes every node that carries an origin mark. For Yong text without marks, such as hand-written `.yong` files, raise removes a node of an inserted kind only if elaborating the result inserts it again at the same place, so the raised item elaborates to the same Yong.

**Checking the laws (R564).** `tools/conform.py --laws` checks every law that applies (laws 1 to 6, and N7 for each niche surface) on every conformance program, with no extra case files. CI reports the results once the printers from Yong exist and gates on them from M3. `conformance/roundtrip/` holds cases that pin spellings the suite does not otherwise reach.

**Item identity.** An item's hash is defined in section 3.6. Two items with the same hash are the same item, whichever surface they came from.

### 16.6 Fei: the third surface

Fei (斐, "brilliantly patterned") is the dense ASCII surface and the language-design playground (R516). Files use the extension `.fei`. It covers layers L0 to L6. Its print-only view, `yong view --as fei`, ships in M3 beside the Wen printer, printing any Yong item with no spelling notes (R563); its reader, lenient reader and spelling-note round trip are built in M4. Any Yong item prints as Fei. Fei adds only syntax (law 1): every Fei module elaborates to the same Yong items, with the same hashes, as its Wen projection. Where Fei leaves a conversion, a `None` or a loop implicit in its text, the Yong text is explicit (law 5). The full reference, with the lexicon, precedence, grammar, spelling-note model and one entry per form, is RFC 0001 (`rfcs/0001-fei.md`); it moves into this section before the Fei build starts.

**Admission (R517).** A Fei form is admitted only if it passes all five rules:

1. **K1 Same core.** It lowers to Yong that Wen can write, or to an accepted Yong extension (26.6). A form that needs an extension is rejected with a diagnostic naming it until the extension lands.
2. **K2 No invented hashed names.** A lowering never creates or drops a name the item hash includes (3.6).
3. **K3 Recognisable.** The printer has a decidable recogniser for the lowered shape, and the grammar is LL(1) after the four factorings of RFC 0001 section 4.
4. **K4 Spelling kept like comments.** Where several spellings elaborate to one tree, the author's choice is a spelling note (16.5): `fei(elaborate(F), spelling(F)) == F`. A tree with no notes prints with each shape's default.
5. **K5 Five facts visible.** Each of the five facts (law 4) keeps a mark in the text. A form may move a mark to the definition (variadic parameters, properties) but never drop it.

**Keyboard rule (R520).** Every Fei token outside string literals and comments is ASCII, so all of Fei can be typed on an ordinary keyboard. Unicode is allowed in strings and comments, as in every surface (R338). Operator glyphs outside ASCII are not admitted, even as aliases.

**Open admission (R521).** Any form that makes Fei more expressive is admitted if it passes K1 to K5. Fei therefore admits forms that Jian and Wen leave out: shadowing, `where`, for-else, `goto` to join points, parallel assignment, try/catch over typed failures, `defer`, null as `None`, lossless implicit widening, `extends` as delegation (no subtyping), `async`/`await` as branch and join inside `par concurrent`, file-local operator fixity, tacit trains and the rest of RFC 0001's catalogue. Forms that need an accepted extension (macros, `@memo`, effects with resume, symbols, `any`, `recover`, `atomic`, `eval`) wait for it. Out: operator glyphs outside ASCII (R520), juxtaposition calls and implicit multiplication (ambiguous, law 3), global monkey patching (law 6) and multi-shot continuations.

**Core spellings.** `:=` binds, `<-` assigns, `=` is equality, prefix `~` marks mutation, `!` in a type and `!!` in code mark failure, `|-` lists capabilities, `|>` is the pipeline, `e |?` opens a match, `||` and `|||` are `par` and `par concurrent`, and `<~` sends and receives on channels. Precedence comes from an operator's characters, so a file parses before any import is resolved; a file may declare fixity for operators it defines or imports, relative to an existing operator, for that file only.

### 16.7 Planned niche surfaces (R536 to R550)

A niche surface is a small, domain-shaped language over Yong. It may be a spelling of a subset of Yong, or it may give its items semantics of their own (R568). None is part of 1.0. The reference for each is RFC 0002; each enters this section normatively through its own RFC, with its grammar, subset predicate, projection column (16.3), diagnostic codes and conformance column.

**Admission (R536).** A niche surface is admitted only if it meets all nine rules:

1. **N1 Semantics (R568).** It lowers to Yong that Wen can write or to an accepted Yong extension, or it defines semantics of its own under the boundary rules below. Its RFC says which, per form.
2. **N2 Subset with a checker.** A decidable predicate says which items print in the surface; outside it, `yong view --as <name>` reports the first excluding construct, as 16.4 does for Jian.
3. **N3 One printing.** Law 8 holds as for Jian and Wen. Sugar is recovered from the Yong tree, from stage-0 metadata attached by the lowering, or from the trivia of 16.5.
4. **N4 Same item hash.** A lowering never creates or drops a hashed name (3.6); generated names follow a fixed rule.
5. **N5 Five facts visible.** Each of the five facts (law 4) keeps a mark in the text.
6. **N6 Crate.** `yong-<name>` holds the lexer, parser, AST and lowering into `yong-hir`, and touches nothing below it.
7. **N7 Round-trip laws.** For surface S: `S(elaborate(F)) == F` byte for byte for canonical F, and `elaborate(S(W)) == elaborate(W)` by item hash for any Wen W in the subset.
8. **N8 Names and diagnostics.** A column in 16.3 for names it spells differently; messages render per surface (R297); new codes only for its own syntax.
9. **N9 Files and lexis.** Extension `.<name>`, one surface per file, any mix per package, calls across surfaces by name; lexical rules as 13.1.

| Row | Surface | Extension | Job | Lowers to | Waits for | Track |
| --- | --- | --- | --- | --- | --- | --- |
| R537 | Qi 契 | `.qi` | wire schemas and remote interfaces with version history | `derive(Wire)` types, refinements, `@remote` traits, R406 converters | streams over `Remote` (R555) for stream methods | practical |
| R538 | Xun 询 | `.xun` | typed model programs | `model.generate_chat` and `validate` (17.4) | none | practical |
| R539 | Cha 查 | `.cha` | typed table queries | `LazyFrame[R]` plans (11.9) | none | practical |
| R540 | Ling 令 | `.ling` | capability-safe shell scripts | line iterators, rooted handles, argv-only `proc` (R448) | none | practical |
| R541 | Cheng 程 | `.cheng` | durable workflows | `@durable` functions (R416, R453), durable timers (R552) | `@yong/durable` (M4) | practical |
| R542 | Zhu 筑 | `.zhu` | infrastructure with plan and apply | resource capabilities with `Deferred[T]` results | dry-run handler class (R554) | practical |
| R543 | Wei 位 | `.wei` | binary layouts with checked parsers and writers | view-backed records over `View[u8]`, `@layout(bits)` for LSB-first registers | none | practical |
| R544 | Lu 律 | `.lu` | synchronous dataflow on the embedded profile | state records and step functions under `executor` or `srp` (R323) | none | practical |
| R545 | Meng 梦 | `.meng` | contracts whose bodies are generated by the agent loop and pinned by hash | functions with pinned bodies; Jian holes (R212) before a body exists | none | research |
| R546 | Gua 卦 | `.gua` | probabilistic models with inference as handlers | a `Prob` capability in `@yong/prob` over seeded `rand` | `grad` (11.5) for gradient samplers | research |
| R547 | Li 礼 | `.li` | session-typed protocols | `@linear` state types (R443) over `std.chan` | none | research |
| R548 | Pu 谱 | `.pu` | specs checked against actors by trace refinement | `std.sim` models and replay checks (17.7) | none | research |
| R549 | Zi 字 | `.zi` | statute-shaped rules with default logic | `uses none` functions over `Money` (11.7) | none | research |

**Own semantics (R568).** A niche surface is exempt from law 1. Inside its items it may define behavior that Yong cannot express, such as three-valued logic, inference as evaluation, or clocked time. These rules still bind it:

1. **Boundary signature.** Every item another surface can call has a signature in Yong types, with `fail` and `uses`. Calls across surfaces go through that signature only, and checked Yong sees the item as that signature.
2. **Laws kept.** Laws 2 (safe by default), 4 (five facts), 6 (local reasoning) and 7 (deterministic by default) hold under the surface's own semantics. Effects happen only through capabilities named in `uses`, so capability ceilings, `nondet` and record and replay apply unchanged.
3. **Two routes.** A form either lowers to checked Yong, which keeps N4, N7 and the one back end, or runs on a surface engine: an interpreter or compiler in `yong-<name>` or a std package, linked to its boundary signature as an extern item is (section 12). N6 allows the engine crates. Lowering is preferred where it can express the form.
4. **Engine items.** For an item on an engine, N2, N3 and N7 apply to its boundary signature only. `yong view --as wen` and `--as jian` print the signature and report the body as "<name> semantics (R568)". The item hash covers the canonical surface text and the engine's identity hash (R348).
5. **Conformance.** The surface's conformance column tests its semantics, including determinism under two hash keys (M0), and every engine item passes `yong test --ub`.

**Order (R550).** No niche surface is built before 1.0. After 1.0, Qi comes first, then Xun, then Cha and Ling; Cheng and Zhu follow the items they wait for. Each surface ships first as a print-only view, `yong view --as <name>`, before it has a reader.
