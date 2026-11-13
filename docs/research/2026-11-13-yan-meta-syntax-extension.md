# Yan Meta: user-defined syntax and the macro model

Study, 2026-11-13, against main at `7caba5c`. Exploratory: the spec is frozen (#1025), so this
proposes no rows and opens no RFC. Items are labelled D1, D2 (decisions) and Q1, Q2 (questions).
Builds on the [Yan Meta syntax frameworks study](../studies/syntax/yan-meta.md) (2026-11-04), whose
P5 "Loom" left user notations waiting for "the user-grammar extension". This is that extension.

## 0. Summary

The three surfaces, as the lead framed them: Yan is the practical, pretty everyday language; Yan
Agent is the Python-shaped skin models write; Yan Meta is the expressive, experimental surface for
people who want power. That reading fits the spec (16.6: lab, kitchen sink and canary). The lead wants
Yan Meta to let programs **define new syntax**.

It can work, and it fits Yan's laws better than macros fit most languages, because Yan already has the
three hard parts: a fixed shape-level reader is the 2026-11-04 recommendation, spelling notes keep the
author's text separate from the hashed tree (16.5), and stage 0 is a deterministic, budgeted,
capability-checked evaluator (10.4). User syntax becomes **declared notations resolved over a fixed
shape tree, expanding to core Yan before checking**. Everything the checker promises (determinism,
capabilities, ownership, the five facts) is checked on the expansion, so no notation can weaken it.

Recommendation, in three tiers, each usable without the next:

1. **Notations** (rule plus template, `notation`): mixfix forms, operators, binders, statement and
   pattern forms. Declarative, total, hygienic by binding specs; the printer recogniser is derived
   from the template, so K3 and K4 hold by construction. Needs no core change.
2. **Islands** (`grammar`): a DSL body (SQL, regex, HTML, a state machine) inside a block or prefixed
   literal that the shape reader delimits. The island grammar is checked LL(1) at definition and its
   action is a stage-0 function. Needs typed staging (R525) for typed output, or lowers to existing
   comptime string DSLs (10.4) until then.
3. **Elaborators** (`elab`): procedural stage-0 functions over `Code[T]`, Lean 4 `elab` style. Only
   after R525 lands, and only where 1 and 2 do not reach.

Two spec conflicts must be decided first (section 6): the non-goal "No syntactic macros in any
surface" (26.2, also 10.4), and the 16.6 sentence "a file parses before any import is resolved".

## 1. Constraints agreed in discussion

| # | Constraint | Where it already lives |
| --- | --- | --- |
| C1 | Definitions run at compile time only and expand to core Yan; determinism and capability safety are checked on the expansion | K1 (16.6), stage 0 (10.4) |
| C2 | Hygienic and module-scoped; a notation is visible only where imported | 16.6 file-local fixity is the precedent |
| C3 | Parsed by a bounded mechanism: registered rules, never arbitrary code inside the parser | shape reader (study 8.1) |
| C4 | Compile-time effects need a capability | stage-0 caps: alloc, package-scoped embed, diag (10.4) |
| C5 | Diagnostics map back to user source | one span representation (CLAUDE.md practice 12), quote anchors (R293) |
| C6 | Lossless syntax tree; formatter and tooling cope | `yan-cst` rowan trees (16.5, R995) |
| C7 | Yan and Yan Agent keep fixed grammars | law 1; R566 probes |
| C8 | Names are frozen | yong-naming |

Derived from the admission rules (16.6):

- **K2** (no invented hashed names): a notation may introduce locals (hashed by position, R519) but
  may only create or drop an *item* name that the use site wrote.
- **K3** (decidable recogniser): tier 1 gets one by inverting the template; tiers 2 and 3 use a
  spelling note validated by re-expansion (section 5.3).
- **K5** (five facts visible): a notation declares the facts its expansion adds (failure,
  capabilities, mutation, ...) in its header, as a function does; the use site keeps any mark K5
  requires. A notation cannot hide a capability.

## 2. Prior art, with the tradeoffs that matter here

| System | Mechanism | What it gets right | What it costs | Take for Yan Meta |
| --- | --- | --- | --- | --- |
| Scheme `syntax-rules` (R7RS) | pattern plus template over s-expressions | total, hygienic, readable, no evaluation | weak errors, no syntax classes, needs s-expressions as the shape | the template model of tier 1 |
| Racket `syntax-parse` (Culpepper, ICFP 2010), scope sets (Flatt, POPL 2016) | patterns with syntax classes, `~describe`, procedural transformers | best macro error messages in practice: "expected binding pair" from declared classes; hygiene as sets of scopes | full phase-separated language at compile time | syntax classes with descriptions; scope-set hygiene |
| Racket `#lang`, readers | whole-file reader replacement | any syntax at all; Rhombus, Typed Racket, Scribble | each `#lang` is a new language for tools; a reader is arbitrary code | reject for Yan Meta: a new reader is a niche surface (16.7), not a notation |
| Rhombus (Flatt et al., OOPSLA 2023), Honu enforestation (GPCE 2012) | fixed shrubbery reader, then operator and macro enforestation with relative precedence | conventional infix syntax with macros; tools read the fixed layer | precedence resolution interleaved with expansion | the shape-reader plus notation design; the closest model |
| Rust `macro_rules!` | token-tree patterns, repetition, fragment specifiers | delimited invocation `m!(...)`, so tools never lose sync; fragment kinds (`expr`, `ty`, `pat`) | mixed-site hygiene (items unhygienic), poor errors, follow-set restrictions | delimited invocation; fragment kinds |
| Rust proc macros | arbitrary Rust over token streams | anything; `derive` and `sqlx::query!` | unsandboxed build-time code, slow builds, IDE must run them, no hygiene | the warning: tier 3 must be stage-0 Yan under caps and budgets |
| Swift macros (SE-0382, 2023) | declared roles (expression, attached), sandboxed compiler plugins, type-checked arguments | expansion always viewable, roles bound what a macro may touch, sandbox | separate plugin process, SwiftSyntax dependency | declared roles; `yan view --expand` |
| Lean 4 `syntax`/`macro`/`elab` (Ullrich and de Moura, IJCAR 2020) | extensible Pratt-style parser by categories, `macro_rules`, elaborators in Lean | user syntax at full power; whole libraries (mathlib notation, tactics) built on it | the parser changes as imports load, so parsing needs elaboration of earlier commands; errors from ambiguous notations | categories (`term`, `tactic`, `command`) as syntax classes; reject import-time parser mutation of the shape layer |
| Rocq (Coq) `Notation` | mixfix with numeric levels | concise mathematics | numeric levels collide across libraries; printing ambiguity | precedence relative to existing operators only |
| Agda mixfix (Danielsson and Norell, 2008) | `if_then_else_` names, precedence as a partial order | mixfix without a global numbering; parse unambiguous or rejected | parsing cost on large precedence graphs | mixfix by holes, precedence as "like/above/below" an existing operator (16.6 already uses `like`) |
| Template Haskell, quasi-quoters | staged splices; `[sql| ... |]` string islands | islands are delimited, so the host parser never changes | quasi-quoter output untyped until spliced; stage restriction | the island model of tier 2 |
| Wyvern TSLs (Omar et al., ECOOP 2014), Relit (Omar and Aldrich, ICFP 2019) | literal body parsed by the expected type's parser; typed splices | three reasoning principles: responsibility (whose parser), context independence, capture avoidance for splices | literal-only | splices in islands are typed and hygienic; the rules of 4.2 |
| Resugaring (Pombrio and Krishnamurthi, PLDI 2014, ICFP 2015) | map core evaluation steps back to surface syntax | diagnostics and traces in the user's syntax | needs emulation checks | origin chains on spans; error placement rules (5.2) |
| SDF/Spoofax, SugarJ (Erdweg et al., 2011) | generalized parsing, grammar modules as libraries | any CFG composes | ambiguity found at use, not definition; slow | the warning against general CFG composition |
| Copper/Silver (Schwerdfeger and Van Wyk, PLDI 2009) | LALR extensions with a modular determinism analysis | extensions proven to compose before use | strong restrictions (marking terminals), complex tooling | islands get composition for free by being delimited; no global analysis needed |
| Julia macros | AST in, AST out, `esc` for capture | ergonomic (`@time`, `@test`, `@einsum`) | gensym hygiene, call-site-only invocation mark `@` | a visible use-site mark is cheap and helps every reader |
| Nim templates and macros | templates (substitution), macros over typed or untyped AST | `untyped` vs `typed` arguments: check before or after expansion | two kinds of arguments to learn | per-argument choice: syntax class or typed `Code[T]` |
| Elixir `quote`/`unquote` | hygienic AST macros, `use` | Ecto queries, Phoenix routes: DSLs people like | invisible imports via `use` | DSL demand is real; imports of syntax must be explicit |
| Raku grammars and slangs | first-class grammars; slangs mutate the main grammar | grammars as a language feature people love | slangs make tooling near impossible | grammars yes (tier 2), mutation of the host grammar no |
| PEG and packrat (Ford, 2004), Pratt (1973) | ordered choice; top-down operator precedence | PEG: no ambiguity by construction; Pratt: cheap operator extension | PEG hides ambiguity in choice order, so printing is ill-defined | Pratt for operator tiers (already the reader); LL(1) over PEG for islands so a printer exists |
| Zig `comptime` | no syntax extension; compile-time execution of ordinary code | one language at two stages | no new syntax | already Yan's core model (10.4); notations sit above it |
| Scala 3 inline and quotes | `inline`, `'{ }` and `${ }`, macros in a separate compilation unit | typed, well-scoped generated code | phase separation | typed staging is R525; tier 3 waits for it |
| Binding specifications: Romeo (Stansifer and Wand, ICFP 2014), Herman (typed hygienic macros, 2010) | declare in the macro's signature which part binds where | hygiene checked at definition time; errors before any use | annotation burden | `b:binder ... body:block[b]` in tier 1 |

Lessons in one line each:

1. Keep a fixed, lossless first layer; every successful tooling story (Rust, Rhombus, Swift) has one.
2. Delimit every extension point so the host parser never changes shape (Rust `!`, TH quoters).
3. Declare syntax classes with descriptions; error quality comes from them, not from the expander.
4. Precedence is relative and local, never a global number.
5. Binding structure declared up front makes hygiene checkable at definition.
6. Arbitrary compile-time code needs a sandbox, budgets and capabilities, or it costs the ecosystem.
7. Printing is the forgotten half; a model without a printer breaks K3 and K4.

## 3. Three candidate grammar models

All three keep the shape reader (study 8.1) fixed: lines, indentation blocks, `( ) [ ] { }`, the
record opener `.{`, quotes, strings, comments, words and operator runs. They differ in what may be
declared above it.

### M1 Rule plus template (`notation`)

A notation is a pattern over shape nodes with typed holes, and a template in Yan Meta.

```
pub notation swap
  syntax  "swap" a:place "," b:place
  means   (a, b) <- (b, a)
  tier    stmt
```

Holes have syntax classes: `expr(tier)`, `place`, `pattern`, `type`, `binder`, `block`, `NAME`,
`OP`, `lit(str)`, plus `sep(c, ",")`, `opt(c)`, `many(c)`. A `binder` hole names where it binds:
`body:block[b]`. Templates are total (no recursion except through other notations, with a depth
budget), so expansion always terminates.

- Soundness: high. Templates are checked once at definition for scope (binding specs) and the
  expansion is type-checked at each use, as two-phase checking already does for stage-0 code (R346).
- Tooling cost: low. The printer recogniser is the template read in reverse (matching a core tree
  against the template), so `yan fmt`, `yan view` and the K4 law work with no extra code.
- Error quality: good. Each class carries a description ("expected a place, such as `x` or
  `a[i]`"); a type error inside the expansion is placed on the hole that supplied the node.
- Agent-readability: high. Declarations are short, uniform and readable without running anything;
  `yan view --as yn` shows the expansion.
- Limit: no computation. A notation cannot inspect types or count fields.

### M2 Pratt operator and mixfix extension

Only operators and mixfix forms, placed relative to an existing operator.

```
pub infixl <+> like ++ := \a, b -> a.merge(b)
pub mixfix _ "clamp" _ ".." _ like .. := \x, lo, hi -> x.max(lo).min(hi)
```

- Soundness: high; it is the file-local fixity of 16.6 made exportable.
- Tooling cost: lowest; the Pratt table gains entries, nothing else.
- Error quality: precedence errors only; "mixing `<+>` with `|>` needs parentheses".
- Agent-readability: medium. Glyph soup is the risk; the study's glyph budget and word twins apply.
- Limit: expressions only. It is a strict subset of M1 (`syntax _ "<+>" _` with `tier`), so M1
  covers it.

### M3 Grammar modules (`grammar`)

A grammar is a set of nonterminals with an entry rule, entered only through a delimited island: an
indented block after a declared head word, or a prefixed literal `name"..."`.

```
pub grammar re |- diag
  entry   alt
  alt     := seq { "|" seq }
  seq     := { atom [ quant ] }
  atom    := CHAR | "." | "(" alt ")" | "{" splice:expr "}" | class
  quant   := "*" | "+" | "?" | "{" INT [ "," [ INT ] ] "}"
  means   re.compile_stage0
```

- Soundness: high on the boundary (the island is opaque to the shape reader except for bracket and
  indent balance, so the host grammar cannot be broken); inside, the action is stage-0 Yan under the
  usual budgets and declared capabilities.
- Grammar class: LL(1) checked at definition, with the conflict named in the diagnostic. PEG would
  accept more, but ordered choice hides ambiguity and leaves the printer undefined; LL(1) gives the
  printer (R527, Boomerang) and predictable errors.
- Tooling cost: medium. The formatter keeps island text verbatim unless the grammar declares a
  printer; the LSP gets tokens and completion from the grammar.
- Error quality: good inside the island (LL(1) "expected one of"), with spans on island text.
- Agent-readability: medium. A grammar is a small language to read, but a familiar one (EBNF, as
  15.2 and RFC 0001 section 4 already use).
- Composition: islands nest only through splices back into Yan Meta (`{expr}`), so two grammars never
  meet in one parse. Copper's modular analysis is not needed.

### Comparison

| | M1 notation | M2 Pratt/mixfix | M3 grammar island |
| --- | --- | --- | --- |
| Reach | statements, expressions, patterns, items, binders | infix and mixfix expressions | any delimited DSL |
| Needs core change | no | no | no for string output; R525 for typed `Code[T]` |
| Printer (K3, K4) | derived from template | derived | declared, or verbatim island text |
| Hygiene | binding specs, scope sets | n/a (functions) | typed splices (Relit rules) |
| Termination | total, depth budget | trivial | stage-0 budgets (R487) |
| Error quality | class descriptions | precedence only | LL(1) expected-sets |
| Tool cost | low | lowest | medium |
| Agent read cost | low | medium (glyphs) | medium |

## 4. Syntax brainstorm

Every example lowers to Yan Core through ordinary Yan Meta; the lowering is shown in Yan Meta
spelling for brevity. All forms are hypothetical. Spellings follow RFC 0001 (`:=` binds, `<-`
assigns, `|-` capabilities, `!` failure in a type, `!!` in code, `|>` pipeline) and its E2 quote
spelling `.< e >.` with `.~e` splice; the 2026-11-04 study's `'{ }` and `$` would read the same.

### 4.1 Declaring and importing syntax

```
@@ app.report
use syntax std.sql.{sql}          -- syntax imports come first and are explicit
use syntax ./notations.{swap, sum}
```

A `use syntax` line names each notation it brings in; no wildcard. Resolution needs only the syntax
imports, not full name resolution, so the file still shape-parses before any import resolves (6.2).

### 4.2 Statement notations

```
pub notation swap
  syntax  "swap" a:place "," b:place
  means   (a, b) <- (b, a)
  tier    stmt

pub notation repeat
  syntax  "repeat" n:expr(tier: additive) "times" body:block
  means   for _ in 0..n => body
  tier    stmt
```

Use and lowering:

```
swap xs[i], xs[j]          ==>  (xs[i], xs[j]) <- (xs[j], xs[i])
repeat 3 times             ==>  for _ in 0..3 => io.print("hi")
  io.print("hi")
```

`a:place` rejects `swap f(x), y` at the use with "expected a place, such as `x` or `a[i]`".

### 4.3 Binder notations with binding specs

```
pub notation argmax
  syntax  "argmax" b:binder "in" xs:expr(tier: range) "by" key:expr[b]
  means   xs.max_by(\b -> key)
  tier    prefix
```

`key:expr[b]` says `b` is in scope in `key` and nowhere else, so the definition is checked for
capture once:

```
best := argmax p in players by p.score    ==>  best := players.max_by(\p -> p.score)
```

### 4.4 Operators and mixfix

```
pub infixl <+> like ++ := \a, b -> a.merge(b)
pub notation clamp
  syntax  x:expr(tier: additive) "clamp" lo:expr(tier: additive) ".." hi:expr(tier: additive)
  means   x.max(lo).min(hi)
  tier    like ..

y := v clamp 0 .. 255     ==>  y := v.max(0).min(255)
```

Precedence is only ever "like", "above" or "below" an existing operator, and families that do not
mix still need parentheses (study 8.3).

### 4.5 Pattern notations

```
pub notation pattern ipv4
  syntax  "ip" "(" a:binder "." b:binder "." c:binder "." d:binder ")"
  means   s if (let .{a, b, c, d} := ip.parse4(s)?)    -- view pattern over std
  tier    pattern

route(s: str) -> str :=
  s |?
    | ip(10 . _ . _ . _) => "internal"
    | ip(a . b . c . d)  => "external {a}"
    | _                  => "not an address"
```

Lowers to a match on `ip.parse4(s)` with the nested patterns; exhaustiveness is checked on the
expansion, so a partial pattern notation behaves as the extractors study recommends (`Option`
result, covers nothing).

### 4.6 Query and pipeline forms

```
pub notation query
  syntax  "from" b:binder "in" src:expr
          many(clause[b])
          "select" out:expr[b]
  clause  "where" p:expr[b]           => .filter(\b -> p)
        | "let" n:binder ":=" e:expr[b] => .map(\b -> (b, e))  -- extends b with n
        | "order" "by" k:expr[b]      => .sorted_by(\b -> k)
  means   src |> clause... |> .map(\b -> out)
  tier    expr

top := from o in orders
         where o.total > 100.0
         order by -o.total
         select (o.cust, o.total)
```

Lowers to `orders |> .filter(\o -> o.total > 100.0) |> .sorted_by(\o -> -o.total) |> .map(\o ->
(o.cust, o.total))`. Query compilation of stage-0-constant plans (10.4) applies to the expansion
unchanged.

### 4.7 DSL islands

SQL against a schema read at stage 0:

```
use syntax std.sql.{sql}

top(db: Db, n: int) -> [Row] ! DbErr |- db :=
  sql db
    select name, total from orders
    where total > {n}
    order by total desc
```

The `sql` grammar declares `|- embed` (to read `schema.sql` at stage 0, package-scoped) and its
expansion calls `db.query(...)`, so the function still states `|- db` (K5). The island body is
checked against the schema at compile time; `{n}` is a typed splice that cannot capture island names
(Relit's capture avoidance). Lowering, roughly:

```
db.query(Plan.scan("orders").filter(.total > n).project(.{name, total}).sort(.total, desc))
```

Prefixed literals for small languages:

```
date_re := re"(\d{4})-(\d{2})-(\d{2})"
page    := html"<p>Hello, {name}</p>"     -- splices escaped by the grammar's action
```

Until R525, an island's action returns Yan Meta source text that is then read and checked as if
written in place; that is the existing comptime string DSL model (10.4) with a real grammar in front.

### 4.8 Test tables

```
pub notation cases
  syntax  "cases" f:NAME INDENT "|" hs:sep(NAME, "|") "|" many(row) DEDENT
  row     "|" vs:sep(expr, "|") "|"
  means   ex f(v1, ..., vk) = want    -- one ex line per row, holes by header
  tier    contract

add(a: int, b: int) -> int :=
  cases add
    | a | b | want |
    | 1 | 2 | 3    |
    | -1| 1 | 0    |
  a + b
```

### 4.9 State machines

```
use syntax std.fsm.{machine}

machine Door
  Closed  --open-->   Open
  Open    --close-->  Closed
  Closed  --lock(k)-> Locked(k)
  Locked(k) --unlock(k2) if k = k2--> Closed
```

Lowers to `type Door := Closed() | Open() | Locked(k: Key)`, `type DoorEv := ...` and a total
`step(d: Door, e: DoorEv) -> Door ! BadTransition`. The item names (`Door`, the constructors) are all
written at the use site, so K2 holds.

### 4.10 Capability-scoped notations

```
pub notation retry
  syntax  "retry" n:expr "times" "backoff" d:expr body:block
  means   retry.run(n, d, \ -> body)    -- std helper; sleeps through clock
  facts   |- clock
  tier    stmt
```

`facts |- clock` declares what the expansion adds. Using `retry` in a function without `|- clock` is
the same error as calling `clock.sleep` there, placed on the `retry` line. A notation never grants a
capability; it can only require one.

### 4.11 Elaborators (tier 3, after R525)

```
pub elab derive_eq(T: type) -> Code[impl Eq for T] |- diag :=
  .< impl Eq for T
       eq(a: T, b: T) -> bool := .~( fields(T) |> .map(\f -> .< a.#f = b.#f >.) |> and_all ) >.
```

For the cases a template cannot express: field-driven code, type-directed choice. Note that
`derived` bodies over reflection (10.4) already cover most derives without any of this; tier 3 exists
for what remains.

### 4.12 Ideas considered and dropped

- Reader macros and per-file readers (`#lang`): a new reader is a niche surface (16.7), with its own
  RFC and conformance column, not a notation.
- Changing the meaning of existing glyphs: breaks "one glyph, one job" (study 8.1) and law 3.
- Unicode operator glyphs: R520 forbids them.
- Juxtaposition-based notations (`2 x`, `f x`): out under R521.
- Notations that define items whose names are computed: K2.

## 5. Implementation sketch

### 5.1 Pipeline

```
text --shape reader--> lossless shape tree (yan-cst)
     --syntax imports--> notation table for this module
     --resolve--> surface AST with notation-use nodes (shape tree untouched)
     --expand (stage 0, budgets)--> Yan Meta AST --> elaborate --> Yan Core --> check
```

The shape tree is never rewritten; notation resolution annotates it. That keeps the tree lossless
(16.5) and lets the formatter and highlighter work on files whose notations fail to resolve.

### 5.2 Spans and diagnostics

Each expanded node carries one origin: either a hole (span of the user text that filled it) or the
template (the notation definition, plus the use site). Diagnostics are placed by the first rule
that applies:

1. A node from a hole: report on the user's text.
2. A node from the template, error caused by a hole value: report on the hole, note the notation.
3. Otherwise: report on the use site, note the template line ("in the expansion of `retry`").

This is one span representation with an origin field, not a second span table (practice 12).
`yan view --expand` shows the expansion with the same origins.

### 5.3 Spelling notes and printing

A notation use is a spelling note: `(node path, notation id, hole assignment)`. Printing tries the
notation's recogniser (the template matched against the core subtree); if it matches, the use prints
as written. Tier 2 and 3 notes store the island text or call, and are valid only if re-expansion
yields the same subtree; otherwise the note drops and the core prints as Yan Meta (16.5 validity
rule). The K4 law `ymt(elaborate(F), spelling(F)) == F` then holds for every tier.

### 5.4 Hashing

The item hash is of the expanded core text, as R525 already says for splices. Two consequences:
changing a notation's definition changes the hashes of items that use it (correct: their meaning
changed), and the same code written with or without a notation has one hash (law 1).

## 6. Conflicts with the current spec

| # | Text | Conflict | Options |
| --- | --- | --- | --- |
| X1 | 26.2 "No syntactic macros in any surface"; 10.4 "no ... syntactic macros in Yan Core, Yan Agent or Yan"; 26.1 metaprogramming row | tier 1 notations are syntactic macros, even if total and hygienic | (a) amend 26.2 to "none in Yan Core, Yan Agent or Yan; Yan Meta notations only, under K1 to K5"; (b) keep the non-goal, ship only M2 operators |
| X2 | 16.6 "a file parses before any import is resolved" | notations depend on imports | keep the sentence for the shape layer; add that form resolution reads only `use syntax` lines, which resolve before other imports |
| X3 | 16.6 K3 "the grammar is LL(1) after the four factorings" | user notations extend the grammar | require each notation's leading token or tier position to keep LL(1) at the shape level; check at the `use syntax` line, error names the two notations |
| X4 | RFC 0001 E2 `macro m(e: Code[int]) -> Code[int]` | E2 is tier 3 only; no tier 1 or 2 | keep E2 as tier 3, add `notation` and `grammar` beside it |
| X5 | 16.6 R521 lists macros among forms that wait for an extension | tier 1 needs no core extension | tier 1 is admitted by K1 to K5 alone; tiers 2 (typed) and 3 wait for R525 |

X1 is the lead's decision; the rest follow from it. All of them are spec text, so under the freeze
they wait for the lead to lift it or to rule this an M1 unblock (it is not).

## 7. Recommendation

- **D1** Adopt the three-tier model: `notation` (M1, with M2 operators as its special case), then
  `grammar` islands (M3), then `elab` on R525. Reject reader replacement and host-grammar mutation.
- **D2** The shape reader stays fixed in every tier. Notation resolution annotates, never rewrites.
- **D3** Syntax imports are explicit (`use syntax m.{a, b}`), named, and resolve before other imports.
- **D4** Holes have syntax classes with descriptions; binders declare their scope (`expr[b]`).
- **D5** Precedence is relative (`like`, `above`, `below` an existing operator); non-mixing families
  stay non-mixing.
- **D6** A notation declares the facts its expansion adds (`facts |- clock`, failure, mutation); it can
  require a capability, never grant one.
- **D7** Island grammars are LL(1), checked at definition; splices are typed and capture-avoiding.
- **D8** Every notation is `experimental` under `@lab(notations)` (R777, R778) until the lifecycle
  promotes the mechanism; a published 1.0 package cannot export notations until then.
- **D9** `yan view --expand` and `yan view --as yn` are required tooling for the first notation, so a
  reader can always see the core.

Staging, consistent with "working, right, fast" and the M1 focus: nothing before M1. After M1 and the
freeze lift: X1 decision, then one RFC for tier 1 with three std notations (`swap`, `argmax`, `cases`)
as the conformance cases; tier 2 with `re` and `sql` once R525 is scheduled; tier 3 last.

## 8. Open questions

- **Q1** Should a notation use carry a visible mark (Julia `@`, Rust `!`), or is the leading word
  enough? A mark costs one glyph per use and makes every notation greppable and obvious to agents.
  Leaning: no mark for tier 1 (they are words), mark for islands (the head word or literal prefix is
  the mark).
- **Q2** May `pub` notations cross package boundaries before the mechanism is `stable`? D8 says no;
  it limits early adopters to one package.
- **Q3** Is the M1 template language Yan Meta, or a quoted core subset? Yan Meta is friendlier; core is
  smaller to specify.
- **Q4** Should island grammars also produce runtime parsers and printers for data (R527 `std.syntax`
  for user formats)? One grammar language for both would be a strong unifier.
- **Q5** How do notations show up in Yan and Yan Agent views? Default: expanded. Alternative: a Yan
  call to a generated function, which K2 forbids unless the user names it.
- **Q6** Should the R566 model-probe harness test notation-heavy code (read, edit, write) before tier 1
  graduates anything into Yan?
- **Q7** Depth budget for notation-in-notation expansion: a fixed default with `@limit`, like stage-0
  steps?
- **Q8** Can a notation be shadowed or overridden locally? Leaning no: one name, one notation per
  module, conflicts are errors at `use syntax`.
- **Q9** Does a notation's declaration participate in the item hash of its users (as a dependency
  hash) or only through the expansion? 5.4 says only the expansion; build caching may want both.

## References

Culpepper and Felleisen, "Fortifying macros", ICFP 2010. Flatt, "Binding as sets of scopes", POPL
2016. Flatt et al., "Rhombus: a new spin on macros without all the parentheses", OOPSLA 2023. Rafkind
and Flatt, "Honu: syntactic extension for algebraic notation through enforestation", GPCE 2012.
Ullrich and de Moura, "Beyond notations: hygienic macro expansion for theorem proving languages",
IJCAR 2020. Danielsson and Norell, "Parsing mixfix operators", IFL 2008. Schwerdfeger and Van Wyk,
"Verifiable composition of deterministic grammars", PLDI 2009. Erdweg et al., "SugarJ: library-based
syntactic language extensibility", OOPSLA 2011. Omar et al., "Safely composable type-specific
languages", ECOOP 2014. Omar and Aldrich, "Reasonably well-typed literal macros", ICFP 2019. Pombrio
and Krishnamurthi, "Resugaring", PLDI 2014; "Hygienic resugaring of compositional desugaring", ICFP
2015. Stansifer and Wand, "Romeo: a system for more flexible binding-safe programming", ICFP 2014.
Herman, "A theory of typed hygienic macros", PhD thesis, 2010. Ford, "Parsing expression grammars",
POPL 2004. Pratt, "Top down operator precedence", POPL 1973. Swift Evolution SE-0382, "Expression
macros", 2023. Foster et al., "Boomerang: resourceful lenses for string data", POPL 2008.
