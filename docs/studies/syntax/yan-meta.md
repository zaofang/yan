# Yan Meta 言元 (`.ymt`): syntax frameworks study

- Date: 2026-11-04
- Status: brainstorm study. Not a spec change, not an RFC. Nothing here is normative.
- Surface: Yan Meta, the dense ASCII surface and the lab, kitchen sink and canary of the language
  (RFC 0001, RFC 0029, spec 16.6).
- Brief (Antonio, 2026-11-04): research peers, what users want, what works and what does not, old and
  new research; write five proposals that each push one axis; mix them into one; aim for syntax that
  is elegant, concise, powerful, expressive and opinionated without being obnoxious.

Every code sample in this file is hand-written and unchecked: no Yan Meta reader exists yet. Samples
that need a research-track core extension are marked so.

## Contents

1. Methodology and process
2. Where Yan Meta stands today
3. Findings: peers, users, research
4. Frameworks the findings suggest
5. The five tasks
6. Five proposals, one axis each
7. Measurements
8. The mix: one consolidated proposal
9. What the mix changes in RFC 0001
10. Open questions
11. Sources

## 1. Methodology and process

**Question.** Not "which operators", but which *framework* lets a lab surface stay dense, admit
everything the lead's open rule allows, and still read as one designed language a year from now.

**Read (narrowly).**

- `docs/surfaces.md` section 4 (Yan Meta status, base forms, the portfolio mock) and section "How
  every surface relates to the core" (the law 4 marks table).
- RFC 0001: decisions D1 to D8 (admission rules K1 to K5, keyboard rule, open rule, research track)
  and the reference sections 1 to 9 (lexis, token set, the 17-level precedence table, grammar,
  spelling notes, name table, printing, lenient reader, base forms), plus the "Yan Meta additions"
  entries.
- RFC 0029 in full (name, charter, lifecycle, the experimental and core-change feature sets).
- Plain Yan section 3 of `docs/surfaces.md`, as the baseline every Yan Meta form prints back to.
- Project notes on the Yan Meta naming memo and the surface-semantics study (law 1 stays).

**Searched.** Rhombus and shrubbery notation; token efficiency of programming languages under LLM
tokenizers; Lean 4 extensible syntax and hygiene; Uiua's glyph formatter; Stefik and Siebert's syntax
studies; Scala 3 quotes and splices; the Verse calculus; Zig comptime limits. The rest of section 11 is
from prior knowledge, marked by confidence.

**Tried.**

1. A BPE token count of every sample under a public tokenizer. Blocked: the container's proxy denies
   the tokenizer downloads and no API key is set. Section 7 uses proxies instead (characters,
   lexical tokens, distinct glyph forms), computed by a short script over the samples in this file.
   A real BPE and model-written measurement belongs in the model-probe harness runs (Plan A step 2).
2. Writing the same five tasks in each proposal. Most of what the proposals teach came from this:
   several "obviously good" ideas fell apart on the second task (section 6 notes each one).
3. Retrofitting each proposal onto the K1 to K5 admission rules. Two ideas died there (juxtaposition
   calls, user-declared notations before the user-grammar extension).

**Process.**

1. Read the current design and list its pressure points (section 2).
2. Survey peers in six families, then research on readability, learnability, staging, hygiene and
   tokens (section 3).
3. Extract frameworks: structural ideas that decide many syntax questions at once (section 4).
4. Fix five tasks that exercise the surface's three jobs: dense everyday code, arrays, and
   metaprogramming and lab forms (section 5).
5. Write five proposals, each optimizing one axis and saying what it sacrifices (section 6).
6. Measure what can be measured offline (section 7).
7. Mix: take from each proposal what survived the tasks, and resolve the conflicts (section 8).
8. Diff the mix against RFC 0001 (section 9) and list what the lead must decide (section 10).

**Scoring.** Proposals are scored 1 to 5 on seven axes by judgment, not measurement: brevity,
readability for a newcomer, agent token cost, explicitness of the five facts, expressive power,
tooling and parse robustness, and admission fit (K1 to K5, keyboard rule, law 1).

## 2. Where Yan Meta stands today

What is designed and accepted:

- **Admission.** K1 same core, K2 no invented hashed names, K3 recognisable and LL(1) but for four
  factorings, K4 spelling kept as hash-neutral notes, K5 the five facts keep a visible mark. Every
  token outside strings and comments is ASCII.
- **Open rule.** Anything that makes the surface more expressive and can be built is admitted.
- **Lifecycle.** Stages experimental, stable, graduated, retired; `@lab(slug)` opt-in per module;
  promotion and graduation by evidence; retirement by `yan fmt` respelling within four minors.
- **Spelling notes.** Several spellings elaborate to one tree; the author's choice is trivia, like a
  comment. The printer needs a recogniser per form.

Pressure points, found while writing the tasks against RFC 0001:

1. **Glyph load.** About 75 operator tokens and over 100 reserved words. Several glyphs carry two or
   three jobs: `@` (attribute, matrix product, method receiver, invariant brace), `'` (symbol and
   transpose), `|` (variant, arm, bitwise or, comprehension bar), `\` (lambda and the `f \ xs`
   operator), `#` (compile time; also tempting for length).
2. **Precedence.** A 17-level total order. Nobody memorizes 17 levels; readers fall back on
   parentheses, which costs the brevity the levels were meant to buy.
3. **Spacing traps.** Longest match makes `x<-1` an assignment and `x:-1` a rule head; the lenient
   reader warns, but the reader accepts them.
4. **Contract sigils.** `?{ !{ ={ @{ -{` are five rare forms spelled as five near-identical sigils.
   They are read far more than written.
5. **The lab has no shape.** Each new experimental form adds grammar, precedence and printer cases by
   hand. The lifecycle governs *whether* a form lives, but nothing makes forms uniform, so the
   kitchen sink grows by accretion.
6. **Staging spelling.** MetaOCaml's `.< e >.` and `.~e` are faithful to the literature but read as
   noise, and they collide with `.{` records and `.~` elsewhere.

None of these is a bug. They are the expected cost of a surface whose rule is "admit everything".
The question this study asks is what structure absorbs that cost.

## 3. Findings: peers, users, research

Confidence: **high** (well-known, primary source read or long established), **medium** (secondary
source or recalled detail), **low** (inference).

### 3.1 Dense array languages: APL, J, K and q, BQN, Uiua

- **What works.** Iverson's thesis that notation is a tool of thought holds for experts: whole
  algorithms fit on a line and become pattern-recognizable idioms (high). Aaron Hsu's co-dfns
  compiler is the strongest modern evidence that a large real program can live in extreme density
  (medium). K and q keep a following in finance because a terse, ASCII, small-core language makes
  interactive data work fast (high). BQN's lesson is consistency: it cleaned APL's ad hoc rules
  into a few uniform ones (syntactic roles by spelling, uniform modifiers) (high).
- **What fails.** Unicode glyphs need special keyboards (Yan already rules them out) and tokenize
  badly: a 2026 RosettaCode comparison under the GPT-4 tokenizer found APL's glyphs split into
  several tokens each, so APL lost its density advantage, while ASCII J was the most compact of 19
  languages, about 70 tokens per task against about 109 for Clojure, with a 2.6x spread across the
  set (medium; secondary report). Tacit trains are the most contested feature in the APL community:
  powerful, but readers trace them slowly (medium).
- **Uiua's move.** You type names (`reverse`, `fold`); the formatter prints glyphs (high). One tree,
  two densities, chosen by the tool rather than the author. Yan Meta's spelling notes can do the same
  without the formatter forcing one side.

### 3.2 Metaprogramming systems

- **Racket and Rhombus.** Racket's macro tradition (hygiene, syntax objects, language-oriented
  programming) is the most complete there is (high). Rhombus (Flatt et al., OOPSLA 2023) brings it to
  infix, indentation syntax with two layers: *shrubbery notation*, a fixed reader that groups lines,
  blocks, brackets and operator runs without knowing any macro, and *enforestation*, which resolves
  operators and forms from bindings in scope (high). Rhombus 1.0 shipped in 2026 (medium; one news
  report). This is the single most relevant peer for Yan Meta.
- **Lean 4.** Notation, macros and elaborators are layered: `notation` and `infix` cover most needs,
  `macro_rules` the next, elaborators the rest, all hygienic (Ullrich and de Moura, "Beyond
  notations", IJCAR 2020, LMCS 2022) (high). Mathlib shows a large community living happily with
  user notations because the layers are explicit and tooling-aware (medium).
- **Scala 3.** Quotes `'{ e }` and splices `${ e }` with a typed `Expr[T]`, one design for macros
  and runtime staging (Stucki, Biboudis, Odersky, GPCE 2018) (high). Users found Scala 3 macros
  harder to write than Scala 2's untyped ones but far more robust across versions (medium).
- **MetaOCaml.** Typed multi-stage programming: generated code is well typed by construction (Taha
  and Sheard; Kiselyov) (high). The bracket syntax is the part nobody imitates (low).
- **Template Haskell, Rust procedural macros.** Popular and painful: untyped or token-level output,
  stage restrictions, slow builds, IDEs that cannot see through expansion (high). Users want macros
  for derive, DSL literals and boilerplate removal, and mostly nothing else (medium).
- **Zig comptime.** Ordinary code at compile time replaces generics, reflection and much macro use.
  Its strength is what it refuses: no host leakage, no code generation from strings, no new syntax,
  no I/O (matklad, "Things Zig comptime won't do", 2025) (high). Yan's stage 0 already has this
  discipline.

### 3.3 Lab and canary mechanisms

- **GHC language extensions.** Per-module `LANGUAGE` pragmas let Haskell trial features for decades,
  and also produced dialect sprawl (over a hundred extensions) until the GHC2021 and GHC2024
  editions bundled the graduated ones (high). Lesson: opt-in works; it needs an exit, which RFC 0029's
  lifecycle already gives.
- **Rust feature gates.** Nightly-only gates keep stable clean, but some gates stay open for years
  (specialization) (high). Lesson: graduation needs evidence and deadlines, not only a gate.
- **Python `__future__`.** A dated, named switch with a promised default year (high). Lesson: name
  the retirement date when a form enters.

### 3.4 Logic and relational

- **Datalog** (Souffle and successors) is the relational form users actually adopt: stratified,
  terminating, deterministic (high). **miniKanren** shows how small a relational core can be (Byrd,
  2009) (high). **Verse** (Augustsson et al., ICFP 2023) puts functional logic programming in a
  deterministic calculus, which matters for a deterministic Yan (high).

### 3.5 Opinionated minimalists

- Go (`gofmt`, one loop), Elm and Gleam (few features, one way, great errors) show that strong
  opinions attract users when they remove decisions rather than add rules (high). Gleam removed
  features on purpose and grew (medium). The Yan Meta charter is the opposite of minimal, so the
  lesson to take is *uniformity*: many forms, one shape.

### 3.6 Research on syntax for people

- **Learnability.** Stefik and Siebert (TOCE 2013): for novices, C-family syntax is not more
  intuitive than a randomly chosen syntax ("Randomo"); English words such as `repeat` beat symbols
  for rare constructs (high). Static types help (Hanenberg and colleagues) (medium).
- **Readability.** Buse and Weimer (TSE 2010): line length, identifier count and parenthesis density
  predict readability judgements more than blank lines or comments do (high).
- **Huffman coding.** Larry Wall's principle: frequent things short, rare things long (medium).
  Perl applied it without a budget and became the cautionary tale (high). The principle is right;
  the missing piece was a cap on how many short forms exist.
- **Growing a language.** Steele (1998): a language should grow by users adding words that look like
  the built-in ones (high). This is the argument for one shape for all forms.
- **Precedence.** Carbon's design specifies precedence as a partial order: operators from unrelated
  families do not mix without parentheses (medium). Fortress did similar (medium). Danielsson and
  Norell (2008) give a precise treatment of mixfix with precedence graphs (high).
- **Whitespace-sensitive operators.** Swift decides prefix, postfix or infix by spacing, and it is
  rarely a complaint (high). It removes Yan Meta's longest-match traps cleanly.
- **Bidirectional syntax.** Invertible syntax descriptions (Rendel and Ostermann 2010) and Boomerang
  lenses give a parser and printer from one definition (high). Yan already holds this as a research
  item; it is exactly what spelling notes and recognisers need.

### 3.7 Agents and tokens

- Models write low-resource languages much worse than high-resource ones (MultiPL-E, Cassano et al.,
  TSE 2023), and transfer helps only partly (Cassano et al., OOPSLA 2024) (high, medium). Yan Meta
  will always be low resource.
- Token efficiency now matters to cost, and ASCII dense syntax wins (section 3.1, medium). But a
  novel glyph saves tokens only if the model writes it correctly; familiar spellings cost a few
  tokens more and fail less (low; to measure).
- Consequence for Yan: agents should read Yan Meta through `yan view --as yag` or `--as yn` and write
  Yan Agent. Yan Meta optimizes for expert humans and for the lab, and only secondarily for agents.

### 3.8 What users want, summarized

From the families above: derive and boilerplate removal; DSL literals (SQL, regex, JSON, formats);
operators for their domain (math, units, arrays); terse pipelines and folds; staging for
specialization; good errors through all of it. They do not want: unbounded user syntax that breaks
tools, untyped code generation, two meanings for one glyph, memorizing precedence.

## 4. Frameworks the findings suggest

Six structural ideas. Each decides many syntax questions at once.

1. **Two layers (Rhombus, Lean).** A fixed *shape* reader that every tool can run, and a *notation
   table* that gives forms their meaning. Lab forms become table entries, not grammar patches.
2. **Huffman with a budget (Wall, corrected).** Frequent forms earn a glyph, but the glyph budget is
   fixed and small. A new glyph form must displace one, or be spelled as a word.
3. **One glyph, one job (BQN, law 4).** A glyph means one thing everywhere. The five facts own their
   marks outright.
4. **Elastic spelling (Uiua, spelling notes).** Every glyph form has a word twin that elaborates to
   the same tree. Density is a per-reader choice, not a dialect.
5. **Stage hygiene by brackets (Scala 3, MetaOCaml, Zig).** One mark for compile time, one bracket
   for quotes, one sigil for splice; typed code; stage 0 cannot reach the host.
6. **Lab as data (GHC editions, RFC 0029).** Each table entry carries stage, opt-in slug, evidence
   and an exit; the lifecycle runs on the table.

## 5. The five tasks

Each proposal writes the same five tasks.

- **T1 Portfolio.** Value positions against quotes; fail on a missing quote; contracts and an example;
  fetch quotes in parallel under `io` and `net`. The program from spec 15.6.
- **T2 Stencil.** Three-point moving mean over the interior of a float array, then scale to unit
  sum. Index notation (experimental entry O6).
- **T3 Derive.** A `Show` derivation for any record by stage-0 field reflection (entries S1, S2).
- **T4 Staging.** A power function specialized at compile time by quote and splice. Needs the typed
  staging research extension in every proposal; K1 rejects it until that lands.
- **T5 Rules.** Ancestors from a parent relation, then list Ann's ancestors (entry S3, Datalog
  style).

Shared types, the same in every proposal except where shown:

```
type Position := {sym: str, qty: int}
type Quote := {sym: str, px: f64}
type PortErr := MissingQuote(sym: str) | Fetch(msg: str)
```

## 6. Five proposals, one axis each

### P1 Glyph: brevity first

**Axis.** Fewest characters per idea, for experts at a REPL. K and J are the models.

**Ideas.**

- Adverbs as postfix glyphs: `f'` each, `f/` reduce, `f\` scan; `+/ xs` stays.
- Implicit-argument lambdas: braces with free `x` and `y` are a function, as K and dfns.
- `#xs` is length; `xs ## .k` indexes by key; trains `(| f g h |)` are the default for point-free.
- Positional record literals `.{"A", 2}` when the type is known.

**T1 Portfolio**

```
value(ps: [Position], qs: {str => Quote}) -> f64 ! PortErr :=
  ?{ #ps > 0 }
  ={ value([.{"A", 2}], {"A" => .{"A", 10.0}}) = Ok(20.0) }
  +/ { f64(x.qty) * (qs[x.sym] ?? !! MissingQuote(x.sym)).px }' ps

main() ! PortErr |- io, net :=
  ps := [.{"SPX", 10}, .{"NDX", 5}]
  qs := { fetch_quote(x.sym)? }||' ps
  io.print("value: {value(ps, qs ## .sym)?}")
```

**T2 Stencil**

```
smooth(a: [f64]) -> [f64] :=
  s := (| +/ / # |)' a.win(3)
  s / +/ s
```

**T3 Derive**

```
derive Show for T :=
  show(v: T) -> str := "{#name(T)}{{{", " `join` [ "{f.name}: {v.#f.show()}" | f in #fields(T) ]}}}"
```

**T4 Staging** (needs typed staging)

```
spow(n: int, x: Code[f64]) -> Code[f64] :=
  n = 0 ? .<1.0>. : .< .~x * .~(spow(n - 1, x)) >.

cube := # .< \x -> .~(spow(3, .<x>.)) >.
```

**T5 Rules**

```
rel anc(x, y) :- parent(x, y)
rel anc(x, z) :- parent(x, y), anc(y, z)

show_anc() |- io := { io.print(x) }' rel anc("ann", ?who)
```

**What the tasks showed.** T1 is short and nearly unreadable on first contact. `#ps` (length) and
`#name(T)` (compile time) collide on one glyph; `/` is reduce in `+/` and divide inside the train in
T2, and `(| +/ / # |)` takes most readers several passes. Implicit `x` breaks down as soon as lambdas
nest (T5 inside T1 would need `x` twice). The T3 string is a wall of braces. Density pays off in T2
and T5 only.

**Gives up.** Newcomer readability, one-glyph-one-job, printer simplicity (trains need recognisers
for every shape).

### P2 Prose: readability and learnability first

**Axis.** A reader who knows Python or plain Yan understands every line without the reference.
Words for every rare form; glyphs only for arithmetic, comparison and the most common flow.

**Ideas.**

- Plain Yan's keywords: `let`, `var`, `fail`, `uses`, `match`, `pre`, `ex`.
- Folds are words: `sum p in ps: e`, `each i in r: e`.
- Metaprogramming in words: `at compile time`, `quote { }`, `splice e`, `code T`.

**T1 Portfolio**

```
pub value(ps: List[Position], qs: Map[str, Quote]) -> f64 fail PortErr =
  pre ps.len() > 0
  ex value([Position("A", 2)], {"A": Quote("A", 10.0)}) == Ok(20.0)
  sum p in ps:
    let q = qs.get(p.sym) else fail MissingQuote(p.sym)
    f64(p.qty) * q.px

pub main() fail PortErr uses io, net =
  let ps = [Position("SPX", 10), Position("NDX", 5)]
  let quotes = par for p in ps: fetch_quote(p.sym)?
  io.print("value: \(value(ps, quotes.index_by(.sym))?)")
```

**T2 Stencil**

```
smooth(a: Array[f64]) -> Array[f64] =
  let s = each i in 1 .. a.len() - 1: (a[i - 1] + a[i] + a[i + 1]) / 3.0
  s / sum(s)
```

**T3 Derive**

```
derive Show for T =
  show(v: T) -> str =
    let parts = at compile time for f in fields(T): "\(f.name): \(v.field(f).show())"
    "\(name(T)){\(parts.join(", "))}"
```

**T4 Staging** (needs typed staging)

```
power_code(n: int, x: code f64) -> code f64 =
  if n == 0 then quote { 1.0 }
  else quote { splice x * splice power_code(n - 1, x) }

let cube = at compile time: quote { (x: f64) -> splice power_code(3, quote { x }) }
```

**T5 Rules**

```
rules
  ancestor(x, y) if parent(x, y)
  ancestor(x, z) if parent(x, y) and ancestor(y, z)

show_ancestors() uses io =
  for who in query ancestor("ann", who): io.print(who)
```

**What the tasks showed.** Every task reads on first contact, and T3 to T5 are the clearest of all
five proposals. But P2 is plain Yan with three extra phrases: it gives the lab nothing a plain Yan
extension could not, and T1 is about 40 percent longer than P1. `splice x * splice f(...)` needs a
precedence rule for a word, which is the worst of both worlds.

**Gives up.** Density, the reason Yan Meta exists as a separate surface.

### P3 Familiar: agent token cost first

**Axis.** Fewest tokens *per correct program* written by a model, not fewest characters. For each
construct, use the spelling most common in training data (Rust, Python, TypeScript, Kotlin, Scala 3)
and the shortest among those.

**Ideas.**

- `fn`, braces, `|p| e` closures, `?` propagation, `f"..."` interpolation, `comptime` (Zig),
  `'{ }` and `${ }` (Scala 3), `:-` rules (Prolog).
- Braces, not layout: a patch tool that damages whitespace cannot change meaning.
- No glyph a model has not seen thousands of times.

**T1 Portfolio**

```
fn value(ps: [Position], qs: {str: Quote}) -> f64 !PortErr {
  pre ps.len() > 0
  ex value([{sym: "A", qty: 2}], {"A": {sym: "A", px: 10.0}}) == Ok(20.0)
  ps.sum(|p| p.qty as f64 * qs.get(p.sym).ok_or(MissingQuote(p.sym))?.px)
}

fn main() !PortErr uses io, net {
  let ps = [{sym: "SPX", qty: 10}, {sym: "NDX", qty: 5}]
  let qs = ps.par_map(|p| fetch_quote(p.sym)?).index_by(|q| q.sym)
  io.print(f"value: {value(ps, qs)?}")
}
```

**T2 Stencil**

```
fn smooth(a: [f64]) -> [f64] {
  let s = (1..a.len() - 1).map(|i| (a[i - 1] + a[i] + a[i + 1]) / 3.0)
  s / s.sum()
}
```

**T3 Derive**

```
derive Show for T {
  fn show(v: T) -> str {
    let parts = comptime for f in fields(T) { f"{f.name}: {v.@field(f).show()}" }
    f"{T.name}{{{parts.join(", ")}}}"
  }
}
```

**T4 Staging** (needs typed staging)

```
fn power(n: int, x: Code[f64]) -> Code[f64] {
  if n == 0 { '{ 1.0 } } else { '{ $x * ${ power(n - 1, x) } } }
}

const cube = comptime '{ |x: f64| ${ power(3, '{ x }) } }
```

**T5 Rules**

```
rule ancestor(x, y) :- parent(x, y).
rule ancestor(x, z) :- parent(x, y), ancestor(y, z).

fn show_ancestors() uses io {
  for who in query ancestor("ann", who) { io.print(who) }
}
```

**What the tasks showed.** It is the proposal a model would write best today, and Scala 3's quote
spelling in T4 is clearly better than MetaOCaml's. But it is a pastiche: `as f64` (Rust), `f"..."`
(Python), `comptime` (Zig) and `:-` (Prolog) in one file have no common voice, and it duplicates
Yan Agent's job. Methods instead of folds (`ps.sum(|p| ...)`) also lose the shape the printer
recognises for `sum_by`.

**Gives up.** Identity and craft; most of what makes Yan Meta a lab rather than a fourth spelling of
Yan Agent.

### P4 Ledger: safety and explicitness first

**Axis.** Everything that matters for trust is visible where it happens: the five facts, stage
levels, capability flow, and every lab form in use.

**Ideas.**

- Capabilities are parameters (object-capability style, as Yan Core's `cap io: Io`), not an ambient
  `uses` list; passing `net` to a call is the visible delegation.
- Contracts sit in the header, before `:=`, as part of the signature a reviewer reads.
- Stage is a visible number: `#0` marks stage-0 code; quotes have type `Code[T]`; splice is `$`.
- Every experimental form used is named in the module's `@lab(...)` line (already the rule), and the
  printer keeps a facts column for review.

**T1 Portfolio**

```
pub value(ps: [Position], qs: {str => Quote}) -> f64 ! PortErr
  pre ps.len() > 0
  ex value([.{sym: "A", qty: 2}], {"A" => .{sym: "A", px: 10.0}}) = Ok(20.0)
:=
  +/ p in ps =>
    q := qs.get(p.sym) ?? !! MissingQuote(sym: copy p.sym)
    f64(p.qty) * q.px

pub main(io: cap Io, net: cap Net) ! PortErr :=
  ps := [.{sym: "SPX", qty: 10}, .{sym: "NDX", qty: 5}]
  quotes := || p in ps => fetch_quote(net, p.sym)?
  io.print("portfolio value: {value(ps, quotes.index_by(.sym))?}")
```

**T2 Stencil**

```
@lab(index-notation)

smooth(a: [f64; n]) -> [f64; n - 2] :=
  s[i] := (a[i] + a[i + 1] + a[i + 2]) / 3.0
  s / +/ s
```

**T3 Derive**

```
derive Show for T :=
  show(v: T) -> str :=
    parts := #0 [ "{f.name}: {v.#f.show()}" | f in fields(T) ]
    "{name(T)}{{{parts.join(", ")}}}"
```

**T4 Staging** (needs typed staging)

```
power(n: int, x: Code[f64]) -> Code[f64] #0 :=
  if n = 0 then '{ 1.0 } else '{ $x * ${ power(n - 1, x) } }

cube: (f64) -> f64 := #0 '{ \x -> ${ power(3, '{ x }) } }
```

**T5 Rules**

```
rules ancestry(parent: Table[(str, str)]) -> Table[(str, str)] :=
  anc(x, y) :- parent(x, y)
  anc(x, z) :- parent(x, y), anc(y, z)

show_ancestors(io: cap Io, parent: Table[(str, str)]) :=
  for who in ancestry(parent).where(.0 = "ann").sorted() => io.print(who.1)
```

**What the tasks showed.** T1's `fetch_quote(net, p.sym)` makes the capability flow obvious, and the
header contracts read well. T2 needs size arithmetic for `n - 2`, a research extension, so the most
explicit spelling of T2 is not available yet. Capabilities as parameters fight plain Yan's ambient
`uses`, which would make `yan view --as yn` noisy; that belongs to the core, not to one surface.
Explicit `.sorted()` in T5 is good: it states the determinism the core already guarantees.

**Gives up.** Brevity in signatures; agreement with plain Yan on capability style.

### P5 Loom: expressive power first

**Axis.** The surface can say anything the core can, and the lab can add a form without touching the
reader. Everything above the core is a *notation*: one declaration gives syntax, meaning, printer
recogniser, precedence tier and lifecycle stage.

**Ideas.**

- Two layers, as Rhombus: a fixed shape reader (lines, blocks, brackets, operator runs), then
  notations resolved by a table.
- The shipped notation table *is* the Yan Meta reference. Each RFC 0001 entry becomes one
  declaration, so the reference is executable and bidirectional.
- User-declared notations wait for the user-grammar research extension; until then the table is
  closed.

**Notation** (how the reference would define two forms; not counted in section 7)

```
notation sum_by
  syntax  "+/" b:binder "in" xs:expr(tier: range) "=>" body:block
  means   fold(xs, zero(), \acc, b -> acc + body)
  word    "sum" b "in" xs "=>" body
  tier    prefix
  stage   stable

notation rules
  syntax  "rules" name:NAME sig:signature ":=" INDENT { h:call ":-" bs:sep(call, ",") } DEDENT
  means   std.rel.program(name, sig, [clause(h, bs), ...])
  tier    item
  stage   experimental
  lab     "rules"
```

**T1 Portfolio**

```
pub value(ps: [Position], qs: {str => Quote}) -> f64 ! PortErr :=
  pre ps.len() > 0
  ex value([.{sym: "A", qty: 2}], {"A" => .{sym: "A", px: 10.0}}) = Ok(20.0)
  +/ p in ps => f64(p.qty) * (qs.get(p.sym) ?? !! MissingQuote(sym: copy p.sym)).px

pub main() ! PortErr |- io, net :=
  quotes := || p in ps => fetch_quote(p.sym)?
  io.print("value: {value(ps, quotes.index_by(.sym))?}")
  where ps := [.{sym: "SPX", qty: 10}, .{sym: "NDX", qty: 5}]
```

**T2 Stencil**

```
@lab(index-notation)

smooth(a: [f64]) -> [f64] :=
  s[i] := (a[i - 1] + a[i] + a[i + 1]) / 3.0
  s / +/ s
```

**T3 Derive**

```
derive Show for T :=
  show(v: T) -> str := "{name(T)}{{{", ".join(#for f in fields(T) => "{f.name}: {v.#f.show()}")}}}"
```

**T4 Staging** (needs typed staging)

```
power(n: int, x: Code[f64]) -> Code[f64] :=
  if n = 0 then '{ 1.0 } else '{ $x * ${ power(n - 1, x) } }

cube := #'{ \x -> ${ power(3, '{ x }) } }
```

**T5 Rules**

```
@lab(rules)

rules ancestry(parent: Table[(str, str)]) -> Table[(str, str)] :=
  anc(x, y) :- parent(x, y)
  anc(x, z) :- parent(x, y), anc(y, z)

show_ancestors(parent: Table[(str, str)]) |- io :=
  for (_, who) in ancestry(parent).where(.0 = "ann").sorted() => io.print(who)
```

**What the tasks showed.** The samples look like the current surface, which is the point: the
framework changes how forms are *defined*, not how most code looks. The notation block is where the
value is: one place states syntax, lowering, word twin, tier and stage, and the printer, lenient
reader, docs and lifecycle all read from it. Risks: a shape reader needs Swift-style spacing rules
for operators to stay LL(1); the table must be closed until the user-grammar extension or K1 and K3
break; the declaration language is itself a small language to design.

**Gives up.** Some reader speed (a table lookup per operator run) and a simple mental model of "the
grammar is this page".

### Scores (judgment, 1 to 5)

| Axis | P1 Glyph | P2 Prose | P3 Familiar | P4 Ledger | P5 Loom |
| --- | --- | --- | --- | --- | --- |
| Brevity | 5 | 2 | 3 | 3 | 4 |
| Newcomer readability | 1 | 5 | 4 | 4 | 3 |
| Agent token cost (per correct program) | 2 | 3 | 5 | 3 | 3 |
| Five facts explicit | 3 | 4 | 3 | 5 | 4 |
| Expressive power | 4 | 2 | 3 | 3 | 5 |
| Tooling and parse robustness | 2 | 4 | 4 | 4 | 5 |
| Admission fit (K1 to K5, ASCII, law 1) | 3 | 5 | 4 | 3 | 4 |

## 7. Measurements

Offline proxies over the samples in section 6 and section 8, computed by a script that reads this
file: **chars** is non-whitespace characters; **lex** counts identifiers, numbers, strings and
operator runs as one token each; **glyphs** is the number of distinct operator runs used across all
five tasks. BPE counts and model-written correctness are not here (section 1, "Tried").

| Proposal | chars (T1 to T5) | lex (T1 to T5) | distinct glyph runs |
| --- | --- | --- | --- |
| P1 Glyph | 639 | 320 | 26 |
| P2 Prose | 900 | 359 | 12 |
| P3 Familiar | 848 | 405 | 18 |
| P4 Ledger | 926 | 433 | 25 |
| P5 Loom | 882 | 416 | 25 |
| Mix | 883 | 419 | 25 |

Reading, which corrected two expectations this study started with:

- **Glyphs save characters more than tokens.** P1 is about 28 percent shorter than the mix in
  characters and about 24 percent shorter in lexical tokens. By inspection, most of P1's saving
  comes from implicit arguments and positional records, not from its glyphs.
- **Words are cheap in tokens.** P2 is the longest in characters but has fewer lexical tokens than
  P3, P4, P5 and the mix, and the smallest glyph set (12). A word such as `sum` or `fail` is one token
  where a glyph form often needs a glyph plus a delimiter. Part of the gap is that P2's T5 leaves out
  the table types the others write. Even so, this is the measured case for word twins
  (8.1, principle 4): agents can write the word spelling at no token penalty.
- **The mix is not shorter than P5.** It lands within one percent of P5; its gains are uniformity and
  meaning per glyph, not length. On five small tasks the distinct glyph count barely moves (25 against
  P1's 26); the budget's effect is on the whole inventory (about 30 fixed glyph forms against about 75
  operator tokens today), which five tasks do not exercise.
- **Familiar is not cheap.** P3 costs more lexical tokens than P2, from braces and method chains. Its
  case rests on correctness, which only a model run can show.

None of this predicts model accuracy; it bounds the token cost under a word-level proxy. BPE counts
can reorder the middle of the table.

## 8. The mix: one consolidated proposal

**Name for the framework: the notation workbench with a glyph budget.** Yan Meta keeps its job and
its look. What changes is the structure under it: a fixed shape reader, a closed table of notations
that is also the reference and the lab ledger, a small budget of glyphs that each do one job, and a
word twin for every glyph form.

### 8.1 Principles

1. **Shape first (from P5).** A `.ymt` file is read in two steps. The shape reader is fixed and
   knows only lines, indentation blocks, `( ) [ ] { }` groups, the record opener `.{`, quotes `'{`,
   strings, comments, words and operator runs. Then the notation table resolves forms. Every tool
   (formatter, highlighter, diff, the lenient reader, an LSP) can run the shape reader on any file.
2. **Huffman with a budget (from P1, capped by P2).** The fixed glyph forms are the ones in 8.2,
   about thirty. A new glyph form must replace one or be spelled as a word. Rare forms are words:
   contracts are `pre`, `post`, `ex`, `inv`, `dec`, not five brace sigils.
3. **One glyph, one job (from P4).** Each glyph has one meaning across the surface; the five facts
   own theirs.
4. **Elastic spelling (from P1, P2, P3).** Every glyph form has a word twin in its notation entry:
   `+/ p in ps => e` and `sum p in ps => e`; `|| x in xs => e` and `par x in xs => e`; `!! e` and
   `fail e`; `a ?? b` and `a else b`; `|- io` and `uses io`. Both elaborate to one tree; spelling notes
   keep the author's choice. Agents write words; experts write glyphs; review reads either.
5. **Stages by brackets (from P3, P4).** `#` evaluates at compile time and prefixes stage-0 forms
   (`#for`, `v.#f`). `'{ e }` quotes, `$x` and `${ e }` splice, `Code[T]` types the result. Three
   marks cover all metaprogramming.
6. **Precedence by tiers (from research).** Seven tiers instead of seventeen levels, and families
   that do not mix without parentheses (8.3).
7. **Spacing decides operator role (Swift).** A glyph with space on both sides or neither is infix; a
   glyph attached only on the right is prefix; only on the left, postfix. `x<-1` is then an error
   with a fix-it, not an assignment.
8. **Lab as data (from P4, P5, RFC 0029).** Each notation entry carries `stage` and, if experimental,
   its `@lab` slug and a retirement deadline. Promotion and retirement edit the entry; the
   formatter's respelling reads the entry's `word` or default form.

### 8.2 The glyph budget

| Job | Glyphs | Word twin |
| --- | --- | --- |
| bind, assign | `:=`, `<-`, compound `+=` and the rest | `let`, `var`, `set` |
| mutation mark | `~x` | `var x` |
| function, arm, lambda | `->`, `=>`, `\x ->`, hole `_` | `fn` |
| match | `e \|?` with `\| P => e` arms | `match e` |
| failure | `! E` in signatures, `!! e`, `?`, `??` | `fail E`, `fail e`, `try`, `else` |
| capability | `\|- io, net` | `uses io, net` |
| concurrency | `\|\|`, `\|\| x in xs => e`, `\|\|[e \| x in xs]` | `par`, `par for` |
| compile time | `#e`, `#for`, `v.#f` | `comptime` |
| quote, splice | `'{ e }`, `$x`, `${ e }` | `quote`, `splice` |
| pipeline, compose | `\|>`, `>>` | `then` |
| reduce, scan | any binary operator then `/` or `\`: `+/ xs`, `max/ xs`, `+\ xs` | `sum`, `fold`, `scan` |
| data | `.{a: 1}`, `.{a=}`, `[T]`, `{K => V}`, `T?`, `..`, `..=` | |
| equality | `=`, `!=` | |
| attributes, lab | `@name(...)` at line start, `@lab(...)` | |

Arithmetic, comparison and bitwise operators are outside the budget, as are domain operators the
std declares (`@` for matrix product after an operand, as numpy's PEP 465).

Reduce is a *rule*, not a list: `op/` and `op\` for any binary operator or named Monoid function
replace the separate `+/`, `*/`, `<>/`, `+\`, `*/\` and `<>\` tokens.

### 8.3 Tiers

Tightest first:

1. **postfix**: calls, indexing, `.f`, `?`, `.#f`
2. **prefix**: `-`, `!`, `~`, `#`, `$`, reduce and scan, `copy`, `share`
3. **arith**: `^` (right), then `*` `/` `//` `%%` `@`, then `+` `-` `++`
4. **compare**: `=` `!=` `<` `<=` `>` `>=` `in` `is` `=~`, chained in one direction
5. **logic**: `not`, `and`, `or` (`and` tighter than `or`)
6. **flow**: `|>`, then `??`, then `if c then a else b`
7. **lambda**: `\x -> e`, `\| P => e`

Families outside the chain: bitwise (`&` `|` `><` `shl` `shr`), composition (`>>`), range (`..`
`..=` `by`). They bind tighter than compare but do not mix with arith or each other without
parentheses (`a + b & c` is an error with a fix-it). A user or lab operator states its tier and family
in its notation entry.

### 8.4 The tasks in the mix

**T1 Portfolio**

```
pub value(ps: [Position], qs: {str => Quote}) -> f64 ! PortErr :=
  pre ps.len() > 0
  ex value([.{sym: "A", qty: 2}], {"A" => .{sym: "A", px: 10.0}}) = Ok(20.0)
  +/ p in ps => f64(p.qty) * (qs.get(p.sym) ?? !! MissingQuote(sym: copy p.sym)).px

pub main() ! PortErr |- io, net :=
  ps := [.{sym: "SPX", qty: 10}, .{sym: "NDX", qty: 5}]
  qs := (|| p in ps => fetch_quote(p.sym)?).index_by(.sym)
  io.print("value: {value(ps, qs)?}")
```

**T2 Stencil**

```
@lab(index-notation)

smooth(a: [f64]) -> [f64] :=
  s[i] := (a[i - 1] + a[i] + a[i + 1]) / 3.0
  s / +/ s
```

**T3 Derive**

```
derive Show for T :=
  show(v: T) -> str :=
    parts := #for f in fields(T) => "{f.name}: {v.#f.show()}"
    "{name(T)}{{{parts.join(", ")}}}"
```

**T4 Staging** (needs typed staging)

```
power(n: int, x: Code[f64]) -> Code[f64] :=
  if n = 0 then '{ 1.0 } else '{ $x * ${ power(n - 1, x) } }

cube := #'{ \x -> ${ power(3, '{ x }) } }
```

**T5 Rules**

```
@lab(rules)

rules ancestry(parent: Table[(str, str)]) -> Table[(str, str)] :=
  anc(x, y) :- parent(x, y)
  anc(x, z) :- parent(x, y), anc(y, z)

show_ancestors(parent: Table[(str, str)]) |- io :=
  for (_, who) in ancestry(parent).where(.0 = "ann").sorted() => io.print(who)
```

The same T1 with every word twin, which prints from the same tree and the opposite spelling notes:

```
pub value(ps: [Position], qs: {str => Quote}) -> f64 fail PortErr :=
  pre ps.len() > 0
  ex value([.{sym: "A", qty: 2}], {"A" => .{sym: "A", px: 10.0}}) = Ok(20.0)
  sum p in ps => f64(p.qty) * (qs.get(p.sym) else fail MissingQuote(sym: copy p.sym)).px

pub main() fail PortErr uses io, net :=
  ps := [.{sym: "SPX", qty: 10}, .{sym: "NDX", qty: 5}]
  qs := (par p in ps => fetch_quote(p.sym)?).index_by(.sym)
  io.print("value: {value(ps, qs)?}")
```

### 8.5 What each proposal contributed

| From | Kept | Left behind |
| --- | --- | --- |
| P1 Glyph | reduce and scan as a rule; `+/` density; short binding | implicit `x`, trains as default, `#` for length, positional records |
| P2 Prose | word twins for every glyph form; contracts as words; `rules` as a block | words for everything; `splice` as a prefix word |
| P3 Familiar | Scala 3 quotes `'{ }` and `${ }`; Swift spacing | braces, `fn`, method-chain folds, mixed voices |
| P4 Ledger | one glyph one job; visible stage mark; explicit `.sorted()`; lab ledger | capabilities as parameters (a core question), header contracts before `:=` |
| P5 Loom | shape reader plus closed notation table; reference as table; tiers per entry | user notations before the user-grammar extension |

### 8.6 Why this meets the brief

- **Elegant and concise.** T1 is three lines of body; the fixed glyph inventory is less than half of
  today's and each glyph means one thing.
- **Powerful and expressive.** Nothing in RFC 0001 or RFC 0029 is dropped as a capability; forms that
  lose their glyph keep a word, and the lab gains a uniform way to add forms.
- **Opinionated without being obnoxious.** One meaning per glyph, seven tiers, spacing rules and
  closed notations are strong opinions. Word twins and kept spellings mean nobody is forced into
  the dense side.
- **Craft.** The reference becomes a table that drives reader, printer, lenient reader, docs and
  lifecycle, so polishing one entry polishes every tool at once.

### 8.7 Fit with the admission rules

- **K1, law 1.** Notations lower to Yan Core or an accepted extension; the table is closed, so no
  user can add semantics. Quotes and splices stay rejected until typed staging lands.
- **K2.** A notation's `means` may not introduce hashed names; the table checker can enforce this
  per entry once instead of per use.
- **K3.** The shape reader is LL(1); notation resolution is a table lookup keyed by the leading word
  or operator run, so each entry's first token must be unique in its position (as the mixfix entry
  already requires). The four factorings remain.
- **K4.** Word twins and glyph forms are spellings of one tree; notes keep the choice.
- **K5.** The five facts keep reserved glyphs and reserved words; no entry may claim them.
- **Keyboard rule.** All glyphs are ASCII.

## 9. What the mix changes in RFC 0001

Proposals only; each would go through an RFC and the lead.

| Today | Mix | Why |
| --- | --- | --- |
| 17-level total precedence | 7 tiers plus non-mixing families | memorability; parentheses where they help |
| Longest match, warn on spacing | spacing decides prefix, infix, postfix | removes `x<-1` and `x:-1` traps |
| `?{ !{ ={ @{ -{` | `pre post ex inv dec` | rare, read-mostly forms deserve words |
| `.< e >.`, `.~e` | `'{ e }`, `$x`, `${ e }` | Scala 3 precedent; no clash with `.{` |
| `'` transpose | `.T` | frees `'` for quotes and symbols |
| `$0` positional closure parameter | `_` hole and `\x ->` only | frees `$` for splice |
| `/\`, `\/` and their quantifiers | `and`, `or`, `all i in r => e`, `any i in r => e` | `\/` collides with lambda and scan reading |
| `_\|_` | `unreachable` | rare |
| six fixed reduce and scan tokens | the `op/`, `op\` rule | one rule instead of a list |
| `>>>`, `<<<` composition | `>>` only, shift uses words `shl`, `shr` | one composition glyph |
| grammar patches per new entry | one notation entry per form | uniform lab |

Retired spellings follow the RFC 0029 retirement step: they read for four minors and `yan fmt`
respells them, so no item hash changes.

## 10. Open questions

- **Q1 Framework.** Adopt the two-layer reader with a closed notation table as Yan Meta's structure,
  with the RFC 0001 reference rewritten as table entries? (Recommended.)
- **Q2 Budget.** Fix the glyph budget at the 8.2 list, with "displace or use a word" for new glyph
  forms?
- **Q3 Tiers.** Replace the 17 levels with the 8.3 tiers and non-mixing families?
- **Q4 Spacing.** Adopt Swift's spacing rule for operator role? It makes some currently accepted
  text an error (with a fix-it), so it needs the retirement window.
- **Q5 Staging spelling.** Move to `'{ }` and `$` before typed staging is designed, so the research
  extension's RFC inherits the spelling?
- **Q6 Shift operators.** `<<` and `>>` as shift (familiar) versus `>>` as composition (dense). The
  mix picks composition and word shifts; numeric code may disagree.
- **Q7 Capabilities as parameters.** P4's object-capability spelling is a core question for every
  surface, not a Yan Meta one. Worth a separate study?
- **Q8 Measurement.** Add Yan Meta words-versus-glyphs pairs to the model-probe harness, so word twins
  are tested on agents with real BPE counts and pass rates?
- **Q9 User notations.** When the user-grammar extension is designed, should user notations use the
  same declaration language as the shipped table? (This study assumes yes.)

## 11. Sources

Searched or read for this study:

- Rhombus 1.0 report: https://runtimewire.com/article/racket-rhombus-1-0-release (medium)
- Rhombus meta tutorial: https://docs.racket-lang.org/rhombus-meta-tutorial/index.html
- Flatt et al., "Rhombus: A New Spin on Macros without All the Parentheses", OOPSLA 2023 (high)
- Token efficiency across languages (RosettaCode, GPT-4 tokenizer):
  https://engineered.at/articles/which-programming-languages-are-most-token-efficient (medium)
- Ullrich and de Moura, "Beyond Notations: Hygienic Macro Expansion for Theorem Proving Languages",
  IJCAR 2020, LMCS 2022: https://arxiv.org/abs/2001.10490v7 (high)
- Uiua (APL Wiki): https://aplwiki.com/wiki/Uiua (high)
- Stefik and Siebert, "An Empirical Investigation into Programming Language Syntax", TOCE 2013:
  https://neverworkintheory.org/2014/01/29/stefik-siebert-syntax.html (high)
- Stefik et al., Quorum, Perl and Randomo, 2011:
  https://neverworkintheory.org/2011/10/24/an-empirical-comparison-of-the-accuracy-rates-of-novices-using-the-quorum-perl-and-randomo-programming-languages.html
  (high)
- Scala 3 metaprogramming reference: https://docs.scala-lang.org/scala3/reference/metaprogramming
  (high)
- Stucki, Biboudis, Odersky, "A Practical Unification of Multi-stage Programming and Macros", GPCE
  2018:
  https://infoscience.epfl.ch/record/257176/files/A%20Practical%20Unification%20of%20Multi-stage%20Programming%20and%20Macros.pdf
  (high)
- Augustsson et al., "The Verse Calculus", ICFP 2023:
  https://www.cs.tufts.edu/~nr/cs257/archive/simon-peyton-jones/verse-icfp23.pdf (high)
- matklad, "Things Zig comptime won't do", 2025:
  https://matklad.github.io/2025/04/19/things-zig-comptime-wont-do.html (high)

From prior knowledge, not re-fetched:

- Iverson, "Notation as a Tool of Thought", CACM 1980 (high)
- Hsu, co-dfns compiler thesis, Indiana University 2019 (medium)
- Lochbaum, BQN documentation (high)
- Buse and Weimer, "Learning a Metric for Code Readability", TSE 2010 (high)
- Hanenberg and colleagues, controlled experiments on static typing, 2012 to 2014 (medium)
- Kohlbecker et al., "Hygienic Macro Expansion", 1986; Flatt, "Binding as Sets of Scopes", POPL 2016
  (high)
- Taha and Sheard, MetaML, 1997; Kiselyov, BER MetaOCaml, FLOPS 2014 (high)
- Rendel and Ostermann, "Invertible Syntax Descriptions", Haskell Symposium 2010; Bohannon, Foster
  et al., Boomerang, POPL 2008 (high)
- Danielsson and Norell, "Parsing Mixfix Operators", IFL 2008 (high)
- Steele, "Growing a Language", OOPSLA 1998 keynote (high)
- Carbon language design, operator precedence as a partial order (medium)
- Swift language reference, operator whitespace rules (high)
- GHC language extensions and the GHC2021 and GHC2024 editions (high)
- Byrd, miniKanren thesis, 2009 (high)
- Cassano et al., MultiPL-E, TSE 2023; Cassano et al., knowledge transfer to low-resource languages,
  OOPSLA 2024 (high, medium)
- Omar et al., Hazel typed holes, POPL 2017 and 2019 (high)
- Ragan-Kelley et al., Halide, PLDI 2013; Slepak et al., Remora, ESOP 2014; Paszke et al., Dex,
  ICFP 2021 (high)
