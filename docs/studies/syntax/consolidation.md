# Yan syntax consolidation: the family grammar and an RFC-ready decision list

Study and proposal, 2026-11-04. Docs only: no spec rows, no conformance edits, no diagnostic codes.
Everything below is non-normative until an RFC and accepted rows say otherwise. Items are labelled
D1, D2 and so on, and open questions Q1, Q2 and so on; none carries a row number. Names are frozen
and used as decided.

Brief (Antonio): syntax that is elegant, concise, powerful, expressive and opinionated without being
obnoxious; creative, vanguardist, framework-seeking; methodology documented. Inputs: the 18 per-surface
studies in this folder, the surface-semantics study, and Antonio's recorded decisions on Yan Core
(adopt the ledger cut after M1, conventions stay words with `let` renamed `borrow`, dotted intrinsics,
`par` outputs in the header, annotated printing as trivia).

## Summary

1. The 18 studies converge. They keep plain Yan bodies and spend novelty in the header. The same
   mechanisms recur across studies: a header that states the policy or contract, an item noun per
   surface, checked examples, pinned artifacts, and views instead of syntax. Section 3 names eleven
   frameworks.
2. The conflicts are small and mostly about glyphs and a few shared words, not about design. There are
   seventeen (section 4). The glyph ones resolve by one rule: a glyph is shared only if every surface
   uses it for the same job; otherwise one surface owns it.
3. The proposal (section 5) is a family grammar: one item shape, a budget of 13 shared glyph jobs plus
   a handful owned by one surface each, and one vocabulary table of about 40 words. Section 5.5 shows
   five surfaces on one domain to check that they read as one language.
4. Section 6 lists 19 decisions (D1 to D19), ordered by dependency, each with a recommendation. Section 7
   turns eight of them into cards for Antonio.
5. Adoption (section 8) follows Plan A: no spec rows before M1. Before M1 only toolchain bugs are
   fixed and, if a run needs them, three Yan Agent elaboration changes are measured as a second arm.
6. Caveat that governs every number in this family of studies: token counts come from four different
   proxies and none from a current model's tokenizer (section 9). Sizes rank proposals inside one study;
   they do not compare across studies, and they are not model cost.

## 1. Methodology and process

**Question.** Combine 18 independent syntax studies into one official-spec direction without losing the
evidence behind each choice.

**Method.**

1. Read each study's summary, its mix (the consolidated proposal) and its open questions, in full.
   Greps for the specific conflicts in the brief (`->`, `~`, `&`, `given`, `<-`, `.yosh`, fence tags).
   I did not re-read the five-proposal sections or the sources, so statements about declined
   proposals repeat each study's own account.
2. Extract from each mix: item keyword, signature move, glyphs and words introduced, core changes
   required, evidence behind it (measured, run on the real parser, probe, or size only).
3. Cluster by recurrence. A framework is a mechanism that two or more studies reached separately.
4. Collect collisions: the same glyph or word with two meanings, the same meaning with two spellings,
   and statements about existing spec rows that disagree.
5. Resolve collisions by rule where a rule exists, by evidence where a study measured, and otherwise
   by a recommendation plus a card.
6. Order decisions by dependency, then by cost.
7. Write a coherent sample set (section 5.5) as a smoke test of the vocabulary. The samples are
   hand-written and unchecked, MOCK in the sense of `docs/surfaces.md`.

**What was not done.** No experiment was rerun and no sample was parsed. The only toolchain-checked
evidence in the family is Plain Yan's (its mix ran through the real `yan-yn` parser and checker) and
the Yan Agent probe (100 runs of one model family). Surfaces that are partial with MOCK today stay MOCK.

**Judgments that are mine, not the studies'.** The glyph ownership rule, the shared-glyph budget, the
wave order in section 8, and the recommendations in D3 to D12. Each is labelled as a recommendation.

**Constraints honored.** Law 1 (surfaces do not change semantics) stands, per the surface-semantics
study. Every surface form must lower to Yan Core; where a study needs a core change, section 6 lists it
separately so that a surface RFC cannot smuggle one in.

## 2. Where the studies stand

| Surface | Ext | Item noun | Signature move | Core change needed | Evidence |
| --- | --- | --- | --- | --- | --- |
| Yan Core | `.yir` | `fn` | ledger cut: words for conventions, dotted intrinsics, `par` outputs, annotated printing | none (printing), migration of all `.yir` goldens | token table (legacy tokenizer), decided by cards |
| Plain Yan | `.yn` | none (clause form) | keyword after dot, one-line guards, `for`/`with` as values, trailing lambdas, implicit subject, example tables, typed hole | none | real parser, 5% fewer lexemes |
| Yan Agent | `.yag` | `def` | `ex` on success value, `char` literal, empty-collection inference, destructuring `else`, arrow types, `law` | small elaboration rules | 100-run probe |
| Yan Meta | `.ymt` | `fn` | fixed shape reader plus closed notation table, glyph budget, word twin for every glyph | none now; research track later | measurements, samples unchecked |
| Yan Shell | `.ynsh` | script | `uses` header is the policy, `$` runs a program, argv is the tree, dot stage is Yan | `check` in std; honest children is a real core and runtime item | size vs bash (398 vs 361) |
| Yan Workflow | `.ywf` | `workflow` | implicit steps, `first`, `undo`/`commit`, `again`, hash-pinned runs | name-keyed journal entries | size |
| Yan Prompt | `.ypm` | `prompt` | header is the contract, lanes typed by trust, strategy clauses, pinned learned text | handler hook for runtime-value enums | size, scorecard |
| Yan Query Language | `.yql` | `query`, `view`, `rule` | thirteen stage words, `nest`, keyed joins, two-valued optionals, groups as scopes | keyed `Table` form | size |
| Yan Schema | `.ysd` | `type` | trailing history marks, `window`, closed by default | `window`, converters, asymmetric presence | size |
| Yan Binary Format | `.ybf` | `layout` | one layout gives parse, write, size, gen, show; derived fields | byte literals | prototype |
| Yan Infra | `.yinf` | `stack` | argument blocks, key-path identity, lifecycle words, `Deferred` discipline | none claimed | size |
| Yan Dataflow | `.ydf` | `node` | equations, `last`, `from`, modes, rates | state-machine lowering | size, five tasks |
| Yan Delegate | `.ydl` | `want` | spec-first items, ask where candidates disagree, pin body by hash | `std.prop` generators | size |
| Yan Probabilistic | `.ypb` | `model` | `~` per random site, `given` at the call site, three readings | `prob` capability | token proto |
| Yan Protocol | `.ypc` | `protocol` | directed choice as indented diagram, role families, `plays`, `play` | run-time role count | size |
| Yan Spec | `.ysp` | none | state, steps, laws, worlds, links | none claimed | size |
| Yan Law | `.ylaw` | `law` | defaults defeated by name, provenance, time axis, brackets | none claimed | default calculus run |
| Yan Docs | `.ydoc` | page | checked page, generated output, bounded page | none | review of peers |

Extensions are as `docs/surfaces.md` and the studies spell them; this table decides none.

## 3. Eleven frameworks that recur

A framework here is a mechanism two or more studies reached independently. Each is stated as a rule a
future surface can be judged against.

**F1. The header is the contract.** Everything a caller, reviewer, checker or agent needs is in the
header, as data: `uses` (Shell, Workflow, Prompt, Docs, Infra, Delegate, Agent), `fail`, `inbox`
(Workflow), `in`/`out` with `rounded` (Law), `window` (Schema), `every` (Dataflow). The body is plain
Yan. This is the strongest convergence in the set and is law 4 made visible.

**F2. One noun per surface, one shape per item.** `node`, `stack`, `want`, `model`, `law`, `layout`,
`protocol`, `workflow`, `prompt`, `query`/`view`/`rule`. Each is a declaration with parameters, clauses,
then a body, lowering to a plain item. No study needed a second block syntax.

**F3. Examples are executable and one line each.** `ex` appears in Plain Yan (table rows), Agent (`ex f(x)
== v`), Delegate (`ex a => b`), Law (`ex ... gives ...`), Dataflow (traces as lists), Spec (traces),
Docs (generated results). The mechanism is shared; the spellings differ (conflict C3).

**F4. A small closed contract vocabulary.** `pre`, `post`, `inv`, `dec`, `law`, `never`, `always`,
`held`. Meta spells contracts as words rather than five brace sigils; Agent, Delegate, Dataflow and
Spec reuse the same words.

**F5. Generated or pinned artifacts are checked in, never regenerated by build.** Delegate's `body`
hash, Prompt's tuned slot, Schema's pinned history and converters, Docs' generated `out` fences,
Workflow's hash-pinned runs, Binary Format's generated diagrams. Rule: a build reads a pin and fails
if it is stale; only an explicit tool rewrites it.

**F6. Views, not syntax.** Graphs, timelines, frame sets, SQL, protocol diagrams, inferred `fail` and
`uses`, bit diagrams are printed by `yan view` from the tree. Several studies declined a syntax because
a view gives the same information (law 8, one printing).

**F7. Time as a small closed vocabulary.** `within d`, `after d`, `at t`, `every d`, `until c`, `wait`,
`first`, `again`. Workflow, Protocol and Dataflow use overlapping subsets with the same meanings; a
deadline always lowers to `with timeout`.

**F8. History marks.** Schema (`since`, `was`, `via ... back ...`, `drop`) and Infra (`was`, `adopt`,
`replace via`, `keep`) both attach "what this used to be" to the declaration, never to a migration file.
The shared word is `was`.

**F9. Boundaries are typed by trust and authority.** Prompt lanes, Agent `Untrusted[T]` (open item),
Shell argv-only words, Infra `Deferred` and schema-checked literals, Workflow journaling capabilities,
Docs bounded pages. All use existing label propagation or capability checks; none invents a second
mechanism.

**F10. The strict default has a named exit.** `open` (Schema), `lax` (Binary Format), `@rolling`
(Workflow), `keep` and `adopt` (Infra), `rounded` (Law), the waiver (Prompt), `unsafe`. This is how the
family is opinionated without being obnoxious: the default is the safe reading, the exception is a word
in the source, and review can grep it.

**F11. Lowering is the specification.** Each mix says what it lowers to (nested `match`, `with timeout`,
`proc.only`, a group-join, a tick counter). The studies that could not say it (run-time role count,
asymmetric presence, name-keyed journal) are exactly the ones that need core work.

**The family hand.** Plain Yan's study wrote seven rules ("the Yan hand"). Generalized to the family:

1. Marks for facts, words for structure, symbols for data.
2. One printing, chosen by rule.
3. The header states; the body does.
4. A shorthand pays twice: it cuts tokens and removes a concept or merges two rules.
5. Borrow from the corpus unless evidence says otherwise (Python, Rust, Swift, SQL, prose).
6. Plain text in files; typography in the editor.
7. Infer what is local, write what crosses a boundary.
8. A glyph belongs to one job, and to one owner if it cannot be shared (section 5.2).
9. The strict default has a named exit (F10).
10. If a view shows it, do not spell it.

## 4. Conflicts between studies and how to resolve them

Recommendation column is mine. "Rule" means the glyph ownership rule or the family hand.

| # | Conflict | Where | Recommendation |
| --- | --- | --- | --- |
| C1 | `->` has up to eight meanings: result type, match arm, example row, lambda, function type, mode transition, protocol message, query/stage | Plain, Agent, Meta, Dataflow, Protocol | Keep `->` for "maps to" in four roles only: signature result, arm or row, function type, transition. Protocol keeps `A -> B` as a role-addressed arrow inside its own item. Dataflow already dropped its "followed by" use. Lambda is `x -> e` (one more "maps to"), so five roles, one idea. Retire `=>` outside Meta (D4). |
| C2 | `~` as inout mark (Meta), proposed Core sigil (declined), random site (Probabilistic); Plain and Agent use `&` for inout | Meta, Core, Probabilistic, Plain, Agent | `~` is owned by Probabilistic, where it is the statistics convention. Inout mark is `&` in every surface that has one (two already do). Core stays words (decided). Meta drops `~x` (D5). |
| C3 | The `ex` line has four spellings: `ex f(x) == v` (Agent), rows `in -> out` (Plain), `ex a => b` (Delegate), `ex k: v gives o: w` (Law) | F3 | One canonical expression form `ex call == value` or `== fail E(...)`; the row table is formatter sugar over it; Delegate's anonymous rows use `->`; Law keeps `gives` because it names outputs (D6). |
| C4 | Meta M4 observes by binding a `~t: Trace` parameter; Probabilistic observes by `given` at the call site | Meta, Probabilistic | `given`. Meta drops observe-by-binding and keeps the trace parameter only as the explicit form of the capability (D5, Probabilistic Q1). |
| C5 | Shell fails the script on non-zero exit; `proc.run` returns an `Output` with a status and does not fail | Shell, spec 7 | No conflict at the core: `$` lowers to run then check, and std gains a `check` on `Output` (or a `ShellError`). The existing row stays as written (D17). |
| C6 | Extension `.yosh` in the spec row vs `.ynsh` decided | Shell, spec 16 | Spec fix, not a design question (D1). |
| C7 | Docs fence tags list `xin`, `wen`, `yan`, `wen!` in the spec; names changed | Docs, spec 18 | Same fix (D1). Tags become `yag`, `yn`, `yir`, `ymt` plus the new surfaces. |
| C8 | Assignment is `=` in Spec and Plain, `<-` in the RFC example and in Meta, `:=` in Meta bind | Spec, Meta, RFC | `=` in every surface. Meta's `<-` and `:=` stay as glyph twins of `set` and `let` inside the lab only. Spec keeps `=` because steps are sequential Yan. |
| C9 | Interpolation `\(x)` (Plain, Docs) vs `{x}` (Shell command words, and a Python reader's expectation) | Plain Q2, Shell D1 | `\(x)` for strings everywhere. In Shell, `{x}` is not interpolation but an argv hole ("exactly one argument, never re-scanned"), a different construct with a different name. Measure `\(x)` vs `{x}` for strings in the harness before any change (D19). |
| C10 | `$` marks a process (Shell) and a splice (Meta) | Shell, Meta | Per-file glyphs; the files never mix. Accepted as the only repeat, noted in the budget (5.2). |
| C11 | `first` is a race (Workflow) and a built-in tick-zero stream (Dataflow); `from` starts a query and gives an initial value | Workflow, Dataflow, Query | Both are contextual words that never share a file. Rename Dataflow's stream to `tick0` if the two surfaces are ever allowed to embed each other. Low priority. |
| C12 | `law` is a contract line (Agent, Delegate) and an item noun (Yan Law) | Agent, Delegate, Law | Keep both. A contract line sits inside a header; the item sits at column 0 in a `.ylaw` file. The formatter never confuses them. Revisit if Yan Law gets an embedded form. |
| C13 | `again`/`loop` in Protocol and `again` in Workflow have different meanings (recursion within a session, continue-as-new) | Protocol, Workflow | Keep. Both mean "go back to the start of the enclosing loop-like thing"; document the shared reading. |
| C14 | `at` means an offset in Binary Format and an instant in Workflow | Binary, Workflow | Contextual, no collision (regions vs time arms). Accept. |
| C15 | Capability attenuation has four spellings: `uses proc(cargo)` (Shell), `uses net("px.example")` (Agent, open), `net.only(...)` handler, Meta's `|- io` | Shell, Agent, Meta, Infra | One data-like form `uses cap(args)` that lowers to the existing `.only` attenuators (D10). |
| C16 | Untrusted text marking is open in Agent, Prompt, Docs; each assumes a different place for it | Agent, Prompt, Docs | One core decision first (D11). |
| C17 | Doc comment `---` is prose for humans (Plain), a goal text (Delegate), an instruction (Prompt), a header (Docs, Shell) | many | Prose with semantics only on item kinds that say so (`prompt`, `want`); everywhere else it is trivia and hash-neutral (D12). |

## 5. The proposal: one family grammar

### 5.1 The item shape

Every non-core surface is plain Yan with one extra item noun and an optional file header. The shape:

```
--- doc comment (prose; semantic only on prompt and want items)
NOUN name(params) -> Result fail Errors uses caps =
  clause lines          -- pre, post, ex, law, never, where, since, was, within, ...
  body                  -- plain Yan statements, or the surface's declarative lines
```

Rules:

1. Header words come in one fixed order: parameters, result, `fail`, `uses`, then surface-specific
   words (`inbox`, `every`, `window`). The canonical printer enforces it.
2. A clause line starts with a word from the vocabulary table (5.3). A line that starts with anything
   else is body.
3. A file may begin with a shebang or a `uses` header (Shell, Docs); everything below it is script mode.
4. A surface adds one noun and at most six clause words of its own. Anything else is plain Yan.
5. Each surface form states its lowering. A form with no stated lowering is a core change and goes in
   section 6, tier E.

### 5.2 The glyph budget

Two kinds of glyph. A **shared glyph** has one job in every surface that uses it. An **owned glyph**
belongs to one surface and no other surface may use it. A glyph with two jobs in two surfaces must be
resolved into one of these (C2).

Shared (13 jobs, budget 14):

| Glyph | Job | Where it exists today |
| --- | --- | --- |
| `?` | propagate failure | Plain, Agent, Core, Meta |
| `&x` | inout mark at parameter and call site | Plain, Agent; Meta adopts (D5) |
| `->` | maps to: result, arm or row, function type, transition, lambda | Plain, Agent, Core, Dataflow, Protocol |
| `=` | bind or assign; equation in Dataflow | all |
| `==`, `!=` | equality | all |
| `:` | type, field, named argument | all |
| `\|` | alternative: sum variant, protocol choice, Meta arm | Plain, Protocol, Meta |
| `.name` | implicit subject, accessor, dot stage | Plain, Agent, Shell, Query |
| `\(x)` | string interpolation | Plain, Docs |
| `...` | typed hole | Plain |
| `---` | doc comment | all |
| `@name` | attribute | all |
| `..` | range | Plain, Meta |

Owned:

| Glyph | Owner | Job |
| --- | --- | --- |
| `~` | Yan Probabilistic | random site `x ~ d` |
| `$`, `$?`, `<`, `>`, `>>`, `&>` | Yan Shell | run a process, status, redirects |
| `#`, quote and splice marks | Yan Meta | compile-time stage, quote, splice (lab only) |
| `\|-`, `!!`, `??`, reduce operators | Yan Meta | glyph twins of `uses`, `fail`, `else`, `fold` (lab only) |
| `[..]`, `at` offsets | Yan Binary Format | bit ranges |
| `!` after a fence tag | Yan Docs | compile-only fence |

Everything else is a word. The budget rule: a new shared glyph must retire one, or be spelled as a word.
The Meta glyph set is the lab's ledger, not the family's budget; a Meta form graduates by being spelled
as a word or as one of the shared glyphs.

### 5.3 The vocabulary table

Words with one meaning across the family. Surface-local words (`nest`, `undo`, `plays`, ...) live in
that surface's section of the eventual spec.

| Group | Words | Meaning | Lowers to |
| --- | --- | --- | --- |
| Facts | `fail`, `uses`, `par`, `?`, `&` | the five facts of law 4 | core |
| Contract | `pre`, `post`, `inv`, `dec`, `law`, `never` | checkable property of the item | contract region |
| Example | `ex` | executable example, one line | test row |
| Refinement | `where` | predicate over a field or parameter | refinement or boundary check |
| Time | `within`, `after`, `at`, `every`, `until`, `wait`, `first` | deadline, delay, instant, period, condition, wait, race | `with timeout`, timers, `select` |
| History | `since`, `was`, `via`, `drop`, `adopt`, `keep` | what a declaration used to be, how to carry it | pinned history |
| Pin | `body`, `learn`, `lock` | hash-pinned generated or tuned content | checked-in file, hash-neutral |
| Escape | `open`, `lax`, `rolling`, `unsafe`, `rounded` | the named exit from a strict default | flag on the item |
| Flow | `par`, `for`, `with`, `match`, `if`, `else`, `loop`, `again` | control, as in plain Yan | core |
| Quantifier | `forall`, `exists`, `all`, `any` | bounded quantifiers in contracts | solver or enumerator |
| Provenance | `given`, `as_of` | condition a model by site; evaluate at a date | call-site argument |

Rule for new words: contextual (a keyword only in the clause region of its surface) unless two surfaces
need it, in which case it enters this table.

### 5.4 What each surface keeps as its own signature

One sentence each, so the family stays recognizable:

- Core: the ledger cut; every convention, authority and failure stated.
- Plain Yan: the seven moves; no `fn` keyword unless the harness says otherwise.
- Agent: Python's body, the header is where novelty lives, desire paths become canonical.
- Meta: a closed notation table, the lab, word twins for every glyph form.
- Shell: the header is the policy, `$` runs, argv is the tree.
- Workflow: every journaled call is a step; `first`, `undo`, `commit`, `again`; code identity is the version.
- Prompt: lanes typed by trust; the header is the contract; strategy is a clause.
- Query: `from` first, thirteen stage words, `nest`, keyed joins, groups as scopes.
- Schema: trailing history marks, closed by default, `window`.
- Binary Format: one layout, six artifacts, four laws.
- Infra: key-path identity, argument blocks, lifecycle words.
- Dataflow: equations, `last`, modes, rates.
- Delegate: a spec with a pinned body; ask where candidates disagree.
- Probabilistic: the generative story; `~` names the site, `given` observes.
- Protocol: directed choice as a diagram; roles are projected.
- Spec: five nouns (state, steps, laws, worlds, links), four uses.
- Law: defaults defeated by name; every value has a why.
- Docs: checked page, generated output, bounded page.

### 5.5 Smoke test: five surfaces on one domain

Hand-written, unchecked, MOCK. The Prompt sketch derives only from the principles in its mix, not from a
grammar the study fixed. The point is that a reader who knows one of these can read the others.

Schema:

```
window v1..v3
type Order
  id: OrderId
  qty: int where qty > 0
  tif: Tif = Tif.day since v2
  sku: str was item
```

Query:

```
query late(orders: Table[Order], today: Date) -> Table[LateRow] =
  from orders
  where due < today and status != Shipped
  nest ls = lines on ls.order_id == id
  derive total = sum(ls.qty * ls.price)
  sort -total
  take 20
```

Workflow:

```
workflow fulfil(o: Order) -> Label fail Cancelled | Timeout uses pay, ship, clock
  inbox cancel: Cancel
  let auth = pay.authorize(o.total)
  undo pay.void(auth)
  first
    cancel(c) -> fail Cancelled(c.why)
    after 2d -> fail Timeout
    until stock.ready(o) every 5min -> ()
  let label = ship.book(o)
  commit
  label
```

Protocol:

```
protocol Checkout between Client, Shop
  Client -> Shop: Order(sku: str, qty: int)
  Shop -> Client: Quote(total: Money) | OutOfStock
    Quote:
      Client -> Shop: Pay(amount: Money) where amount == total within 5min
```

Shell:

```
#!/usr/bin/env -S ynsh
--- Fail CI when the build has more warnings than allowed.
uses proc(cargo), fs(write: "build.log")
arg max: int = 0
$ cargo build --release &> build.log
let n = <build.log | .grep("warning:") | .count()
if n > max
  exit 1, "\(n) warnings"
```

What this shows: the header words (`uses`, `fail`, `inbox`, `where`) read the same; `->` means "maps to" in
the arm, the row and the result; `since` and `was` are one history family; `within` is one deadline
word; plain Yan is the body everywhere. What it does not show: whether a model writes these correctly.
That needs the harness (D19).

## 6. Decision list (RFC-ready, ordered by dependency)

Tier A has no dependency. Each later tier depends on the one before it. "Rec" is the recommendation.
Items marked CORE need a core or std change and are accepted only per the standing acceptance rule
(grows capability and can be implemented).

### Tier A: hygiene

- **D1. Name-drift sweep.** Correct the spec's stale surface names: Shell's extension, the docs fence tags,
  any remaining old names. Rec: one small spec-fix PR, lead-owned, no design content. Needs Antonio's go
  because it edits the spec.
- **D2. Toolchain fixes the runs would otherwise measure.** A keyword usable as a method name after `.`
  (so `.take(k)` parses), `fail` inference on non-`pub` functions, and the copy demanded across mutually
  exclusive `return` branches. Rec: fix before the R567 runs; they are bugs, not syntax.

### Tier B: family rules (one RFC, "family notation")

- **D3. Glyph ownership rule and budget** (5.2). Rec: adopt, with 14 shared jobs as the cap.
- **D4. The arrow family.** `->` has the five roles of C1; `=>` is retired everywhere except as the Meta
  glyph form. Rec: adopt.
- **D5. Inout mark `&`; `~` reserved for random sites; Meta drops `~x` and observe-by-binding.** Core
  stays on words (decided). Rec: adopt.
- **D6. One `ex` family.** Canonical `ex call == value` and `ex call == fail E(...)`; the row table is
  formatter output when three or more lines match; `fails` and `== Ok(...)` are lenient input. Rec: adopt.
- **D7. Contract vocabulary.** `pre post inv dec law never`, with `law` as a for-all line and `forall`
  binders optional. Rec: adopt; `always` and `held` stay surface-local to Spec and Delegate.
- **D8. Time clauses.** `within` is the deadline word on any message, call or wait and lowers to
  `with timeout`; `after`, `at`, `every`, `until` as in the Workflow mix. Rec: adopt the five words as
  clause words usable in any surface that needs them.
- **D9. History marks.** `since`, `was`, `via ... back ...`, `drop` for Schema; `was`, `adopt`,
  `keep`, `replace via` for Infra; one shared meaning for `was`. Rec: adopt together.
- **D10. Attenuated `uses`.** `uses cap(args)` is data-like and lowers to the existing `.only`
  attenuators; the narrowing is part of the signature and visible to review. Rec: adopt. CORE if the
  signature must carry the narrowing.
- **D11. Untrusted marking.** Decide once, in core: a type or label that taints, which operations
  validate, how a lane or fence consumes it. Prompt, Agent and Docs all wait on this. Rec: a card now,
  RFC after M1. CORE.
- **D12. Doc comments.** `---` is trivia everywhere except on items whose surface says it is semantic
  (`prompt`, `want`), and then it is hashed. Rec: adopt.

### Tier C: Core, Plain Yan, Yan Agent

- **D13. Core ledger cut.** Direction decided by Antonio. RFC after M1; conformance migration is a
  lead-owned PR (agents do not edit `conformance/`).
- **D14. Plain Yan moves.** The seven moves of the Plain Yan mix, in the order A1 (keyword after dot),
  A2 (one-line guards), A5 (implicit subject), A3, A4, A7, A6. A1 is a parse widening and may land first.
  Rec: adopt in that order, each its own row group.
- **D15. Yan Agent changes.** D1 to D3 of its study (success-value `ex`, `char` literal, empty-collection
  inference) address the top probe diagnostics. Its D4 to D9 wait for run evidence. Rec: measure D1 to D3
  as a second arm after the M1 baseline (section 8), adopt on pass-rate evidence.

### Tier D: surface RFCs, lowering only

- **D16. Surface order.** Section 8 gives the waves. Each surface RFC states its lowering and is
  independent of the others once D3 to D12 land.
- **D17. Shell first slice.** `$`, `$?`, `<`, redirects, dot stages, `uses` header as data; failure on
  non-zero exit with a std `check`. Program-to-program `|` after the process-pipe work; honest children
  (OS-enforced attenuation for spawned programs) is its own RFC and changes what the attenuators promise.
  Rec: slice first, sandbox separately.
- **D18. Meta restructure.** Fixed shape reader plus closed notation table, glyph budget, word twins,
  seven precedence tiers, Swift-style spacing rule. Rec: adopt as structure after M1; spacing rule needs
  the retirement window because it turns some accepted text into an error.

### Tier E: core and std changes the surfaces ask for

Listed so no surface RFC carries one silently. Rec for all: accept when the surface RFC that needs
them is accepted, not before.

| Change | Needed by |
| --- | --- |
| Name-keyed journal entries (path instead of op index) | Workflow |
| Journal discharges `nondet` for `first` | Workflow |
| Keyed `Table[R] key id` or `Keyed[K, T]` | Query |
| Asymmetric presence (a field optional in one direction), `window`, user converters | Schema |
| Run-time role count in parameterised protocols; crash-stop | Protocol |
| `ex with handler:` rows legal | Delegate, Agent tests |
| `std.prop`: seeded, deterministic, shrinking generators | Delegate `law` |
| Byte literals | Binary Format |
| `prob` capability, trace type, density transform | Probabilistic |
| Process attenuation inherited by children (sandbox) | Shell |
| Tick-state lowering for `node`, `last`, modes | Dataflow |
| Handler hook for enums over runtime values at decode time | Prompt |
| Fence-embedded constants (`\(x)` constant expressions) | Docs |

### Tier F: process

- **D19. Measurement protocol.** Every syntax decision that rests on a token or desire-path claim gets
  an arm in the R567 harness: same tasks, Sonnet and Haiku, pass@1, repair rounds, tokens with a named
  tokenizer, cheating rate. First candidates: interpolation spelling, `fn` or no `fn`, `ex` forms,
  `law` lines, `given`. No syntax RFC cites "fewer tokens" without its tokenizer named.

## 7. Open questions for Antonio (each can be a card)

**Q1. Adopt the glyph ownership rule?** Options: (a) adopt D3 to D5 as a package, `~` for random sites
only, `&` for inout everywhere but Core; (b) adopt D3 only and decide `~` per surface later; (c) no
rule. Rec: (a). It touches Meta and Core's open question 2, and Core's decision (words) stands.

Answer (Antonio, 2026-11-04): (a). D3 to D5 adopted as a package; RFC after M1.

**Q2. One `ex` shape?** (a) canonical `ex call == value`, rows as formatter sugar; (b) keep per-surface
spellings. Rec: (a).

Answer: (a). Canonical `ex call == value`; row tables are formatter sugar.

**Q3. Decide Untrusted marking now?** It blocks Prompt, Agent and Docs. (a) one card now to choose
between a type form and a label on the capability result, RFC after M1; (b) leave open until a run
needs it. Rec: (a), because three surfaces cannot specify a lane or a fence without it.

Answer: (a), and both forms: label propagation is the mechanism and `Untrusted of T` is the header-visible spelling. RFC after M1.

**Q4. Authorize the name-drift sweep (D1)?** One spec-fix PR for `.ynsh` and the fence tags, no design
content. Rec: yes, now; it is small and removes a known error from the spec.

Answer: yes, now. Spec-fix PR #206 is merged.

**Q5. Which wave goes first after M1?** (a) Core ledger cut, Plain Yan, Agent, Docs, Meta restructure
(wave 1); (b) put Shell first because it is the most demonstrable capability story; (c) other. Rec:
(a); (b) is the best second.

Answer: (a). Wave 1 is Core ledger cut, Plain Yan, Agent, Docs, Meta restructure.

**Q6. Meta: adopt the notation workbench and drop `~x` and observe-by-binding?** Rec: yes after M1,
inside the existing lab charter.

Answer: yes, after M1, inside the lab charter.

**Q7. Shell fails on non-zero exit by default, with `$?` as the opt-out?** This differs from the std
default of returning a status. Rec: yes, with the lowering adding a check.

Answer: yes. `$` fails on non-zero exit, `$?` is the opt-out; lowering adds a check and std gains `check` on `Output`.

**Q8. Pay for a current-tokenizer rerun?** The key in the cloud environment lets every token table in
the family be rerun with the count-tokens API (section 9). Rec: yes, once the key is in; it is cheap and
removes the caveat from every study at once.

Answer: yes, once the key is in the cloud environment.

## 8. Adoption order relative to Plan A

Plan A: (1) close the failing M1 conformance cases, then CI gates M1; (2) build the R567 harness;
(3) run on Sonnet and Haiku; (4) gaps the runs expose take priority. New RFCs pause unless a run needs
one. This document respects that.

**Before and during M1.** Nothing here enters the spec. Allowed: D2 toolchain fixes (bugs), and D1 if
Antonio authorizes it. The Plain Yan and Agent studies already say the harness should measure the
current syntax first.

**Baseline then second arm.** Run the R567 baseline on today's syntax. If the baseline shows the
friction the Agent study found (top diagnostic on `ex` of fallible functions, `char` against `str`,
empty-collection inference), add one second arm with D15 applied, and compare. Changing the language
between runs would confound the measurement, so the arm is separate.

**After M1 (RFC waves).** Each wave is a set of RFCs written after the previous wave's decisions.

| Wave | Contents | Why here |
| --- | --- | --- |
| 0 | D3 to D12 as one "family notation" RFC; D13 Core ledger cut | everything else cites them |
| 1 | D14 Plain Yan; D15 Agent; Docs; D18 Meta restructure | no core change, direct effect on the surfaces that exist today |
| 2 | Shell first slice (D17), Prompt, Schema, Delegate | lowering to existing machinery plus small std additions; high agent value |
| 3 | Query, Binary Format, Spec, Protocol layers 1 and 2, Infra, Workflow | each needs one named Tier E item |
| 4 | Dataflow, Probabilistic, Law, Protocol run-time roles, Shell sandbox | engines or real core work, lowest dependency on the others |

The wave order is my judgment from each study's core-change list and evidence; it is not scored.

**Gates.** A wave starts only after the previous wave's RFCs are decided. A surface moves from MOCK to
defined in `docs/surfaces.md` only when it has a parser, a checker path and conformance cases (the
lead owns those PRs).

## 9. Token counts and the other caveats

1. **Four proxies, no current tokenizer.** The studies counted: lexemes (Plain Yan, Query), a
   2023-era Claude tokenizer shipped in an old SDK (Core), o200k and cl100k via js-tiktoken plus the
   legacy tokenizer (Agent), and line or character counts (most others). The proxy in the cloud
   container blocked current tokenizer files and there is no API key in the environment yet. So:
   - Absolute counts do not compare across studies.
   - A ranking inside one study is evidence about that study's samples under that tokenizer only.
   - Differences under about 5 to 10 percent are within the spread between tokenizers (the Core study
     measured indentation as free in its tokenizer; others may not).
   - "Fewer tokens" is not "cheaper for the model": repair rounds and pass rate matter more, and the
     Agent study's probe shows models paying for forced keywords and wrappers more than for glyph
     length.
2. **Samples are mostly unchecked.** Only Plain Yan's mix ran through the real parser; the others are
   MOCK. Sizes of unparsed samples can hide grammar problems.
3. **One model family.** The only model evidence is the Sonnet 5.5 probe (100 runs). A second family needs
   Antonio's approval.
4. **Selection.** Each study wrote its own five proposals and chose its own mix. Consolidation inherits
   those choices; it does not re-score them.
5. **Rerun plan.** With the key in the environment, rerun every table in the family with the count-tokens
   API on the current model families, store the sample files, and cite the tokenizer by name (Q8, D19).

## 10. Not proposed

- A second block syntax for any surface. The item shape is enough.
- New control-flow keywords beyond `first` and the time words.
- Implicit capabilities, in any surface, for convenience.
- Sigils for the five facts beyond `?` and `&`, on Core's and Agent's evidence.
- Spelling views (graphs, timelines, frames, SQL, bit diagrams) as source.
- A shared package of per-surface formatters; each surface's printer comes with its RFC.

## Sources

- The 18 studies in this folder (`yan-core.md`, `plain-yan.md`, `yan-agent.md`, `yan-meta.md`,
  `yan-shell.md`, `workflow.md`, `yan-prompt.md`, `yan-query-language.md`, `yan-schema.md`,
  `yan-binary-format.md`, `yan-infra.md`, `yan-dataflow.md`, `yan-delegate.md`, `yan-probabilistic.md`,
  `yan-protocol.md`, `yan-spec.md`, `yan-law.md`, `yan-docs.md`), with their own source lists.
- `docs/research/2026-11-04-surface-semantics.md` (keep law 1).
- `docs/surfaces.md`, spec sections 7, 16 and 18 for the rows named in conflicts C5 to C7.
- Antonio's decisions recorded in `yan-core.md` and in project memory.
