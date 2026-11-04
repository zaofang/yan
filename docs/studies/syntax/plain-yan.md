# Plain Yan 言 (`.yn`) syntax study: five proposals and a mix

Study, 2026-11-04. Brainstorm for frameworks, not a spec edit: nothing here is a row, and every
proposed form is non-normative until an RFC and accepted rows say otherwise (`CLAUDE.md`). Names are
frozen and used as decided. Samples live next to this file in
[`plain-yan/`](plain-yan/), one folder per variant, with the script that measures them.

## Summary

1. Plain Yan is already close to the frontier: it combines Python's layout, Rust's `?` and exhaustive
   `match`, Swift's `\(x)` and guard-style `else`, Elm's signature style and Pyret's inline examples.
   The proposals mostly confirmed the current spine instead of replacing it.
2. Five axis-pure proposals (brevity, plain reading, machine economy, explicitness, expressive power)
   were written against the same four tasks and run through the real `yan-yn` parser and checker.
   Brevity saved 10% of lexemes; the explicit proposal cost 10%; the rest landed within ±6%.
3. The mix adopts seven moves: keywords as method names after `.`, one-line guards
   (`if c then fail E`), `for` and `with` as values (parity with `par for`), trailing block lambdas,
   one implicit-subject rule for rows and closures, example tables, and typed holes (`...`).
   Like-for-like it is 5% smaller than today's Yan and adds no new concept to the five facts.
4. The experiment found four things in today's toolchain: `take` (a keyword) cannot be a method name,
   so `.take(k)` fails to parse; recursion needs `decreases`; `fail` is not yet inferred on non-`pub`
   functions; and unit literals and `Entry` are not yet supported.
5. The study's main output is a framework, "the Yan hand": seven rules for judging any future `.yn`
   syntax (section 6.1). The open questions in section 7 can all be settled by measurement.

## 1. Methodology and process

**Brief.** Antonio asked for one study per surface: research peers, user wants and the literature,
brainstorm five proposals that each optimize one axis, mix them, and document the process. The
target is syntax that is elegant, concise, powerful, expressive and opinionated without being
obnoxious, for people who value efficiency and appreciate craft.

**Fixed constraints.** Plain Yan adds syntax only (law 1): every form here must lower to Yan Core.
Every text has one parse with one token of lookahead after the stated factorings (law 3), the five
facts stay marked at the use site (law 4), and every tree has one printing (law 8, spec 15). No user
macros. Names are frozen.

**What I read.** Spec 15 in full (lexis, grammar, semantics, canonical form), the design laws in
spec 2, section 3 of [`docs/surfaces.md`](../../surfaces.md), and `examples/tally/g5_tally.yn` as a
real program. I did not read other spec sections except where a sample needed a fact.

**What I searched.** The 2025 Stack Overflow survey (admiration), Stefik and Siebert's syntax trials,
the 2026 "tokenmaxxing" study of coding agents across languages, Roc's syntax revisions, effect
syntax in Koka, Flix and Unison, and the readability and identifier-style literature. Section 2.3
marks each claim with a confidence: **H** (replicated or primary source read), **M** (one good
study, or a consistent practitioner signal), **L** (plausible, weakly supported).

**Experiment.** Four shared tasks (section 3), each written in today's Yan (the baseline), in each of
the five proposals and in the mix: 28 programs. Every program was run through `yan check` and
`yan fmt --check` from this repository (`cargo build -p yan-cli`, commit `dd33531`), which uses the
real `yan-yn` parser, printer and checker. A proposal's new forms are expected to fail to parse; the
point was to make sure the baseline is valid Yan, that failures are exactly the proposed deltas, and
to catch surprises (section 2.4 lists them). Size was measured with
[`plain-yan/metrics.py`](plain-yan/metrics.py): non-blank lines, non-space characters and lexemes
(identifiers, literals, operators). Model tokenizers could not be downloaded in this environment, so
lexemes stand in for tokens; the evaluation harness in Plan A will measure real tokens.

**Evaluation lens.** Each proposal is scored on a subset of the Cognitive Dimensions of Notations
(Green and Petre 1996): terseness, role-expressiveness (does the text say what a part is for),
hidden dependencies, error-proneness, consistency, viscosity (cost of a change), plus closeness to the
existing corpus (what readers and models already know) and fit with the laws.

**Process.** I read the spec first, wrote the baseline samples, ran them, fixed the baseline until it
checked clean (two fixes came from the checker: `decreases e` on the recursive evaluator, a blank
line between `type` items), and only then wrote the proposals, each by pushing one axis as far as it
would go without breaking a law. Ideas I tried and dropped are listed with their reasons in
section 6.3, because the rejections carry as much of the design as the adoptions.

## 2. Findings

### 2.1 Peers: what to take, what to avoid

| Language | Take | Avoid |
| --- | --- | --- |
| Python | Indentation, `and`/`or`/`not`, readable defaults; the corpus models know best | Statement/expression split; one-line lambdas only |
| Rust | Postfix `?`, exhaustive `match`, blocks as values | Lifetime and `::` noise; turbofish |
| Swift | `guard ... else`, `\(x)` interpolation, trailing closures, argument labels | Many closure spellings (`$0`, multiple trailing closures) |
| Kotlin | Trailing lambdas for library control flow | Receiver lambdas that hide which scope a name comes from |
| Gleam | One way to do things, no macros, labels, small surface | `use` callback sugar confuses newcomers |
| Elm | Signature style, error messages, no runtime exceptions | Rigidity that pushes boilerplate onto users |
| Roc | Static dispatch with `.method`, `?` on results | Repeated syntax churn; moved away from ML juxtaposition toward parens and snake_case |
| Koka, Flix, Unison | Effects in signatures, inferred for private code | Effect-row syntax that readers must decode |
| Zig | Explicit control flow, `try`, compile-time code in the same language | Explicitness tax (allocators passed everywhere) |
| Go | Small language, fast to read | `if err != nil` boilerplate, its most cited complaint |
| Scala 3 | Optional braces made code lighter | Two dialects (braces and indentation) split style guides |
| Pyret | `where:` and `examples:` blocks beside the function | Teaching-language reputation limits adoption |
| Lean 4, Agda, Idris, Hazel | Typed holes, Unicode rendering with ASCII input | Notation density for non-specialists |
| Hylo, Austral, Pony | Visible mutation (`inout`), linear capabilities | Austral's deliberate verbosity keeps its user base tiny |
| F#, Nim | Units of measure, uniform call syntax | Operator definitions that make code unreadable to outsiders |
| APL, K, Uiua | Whole-array thinking | Terseness that only specialists can read; belongs in Yan Meta |

### 2.2 What users want

- The most admired languages in the 2025 Stack Overflow survey were Rust (72%), Gleam (70%),
  Elixir (66%) and Zig (64%) (**H**, survey). What they share is less a syntax than a stance: one
  obvious way, strong tooling, errors as values, nothing hidden at run time. Gleam's place is notable
  because it is small and deliberately unclever.
- Error handling verbosity is the top complaint in Go's own surveys for several years (**H**). Users
  reward a one-character propagation mark (`?`) and a readable fallback (`else`).
- Practitioners love Swift's `guard` and Kotlin's trailing lambdas (**M**), and dislike having two
  spellings of the same thing (Scala 3 braces and indentation, Mojo `fn` and `def`) (**M**).
- Inline examples that run as tests are valued wherever they exist: Python and Rust doctests,
  Elixir doctests, Pyret `where:` blocks (**M**).

### 2.3 Research

| Claim | Source | Confidence | Bearing on `.yn` |
| --- | --- | --- | --- |
| C-style syntax helped novices no more than a language with random keywords; Python, Ruby and Quorum did better. `for`, `while` and `foreach` were rated the least intuitive loop words. | Stefik and Siebert, ACM TOCE 2013 | H | Words beat symbols for structure; keep `for` anyway, because familiarity outweighs intuitiveness once learned (law 10's spirit). |
| Non-programmers describe aggregate operations ("all the X that...") rather than loops. | Pane, Ratanamahatana and Myers 2001 | M | Supports `for` as a value and collection methods. |
| Readability correlates negatively with line length, parentheses and identifier count per line; blank lines help. | Buse and Weimer 2010 | M | Supports one-line guards only when short; the 100-column rule stays. |
| Underscore identifiers are recognized faster; camelCase is slightly more accurate for trained readers. | Sharif and Maletic 2010; Binkley et al. 2009 | L | snake_case values are fine. |
| Code is repetitive and predictable, more than natural language. | Hindle et al. 2012 | H | Conventional shapes are cheap for models and people; novelty must pay. |
| Coding agents' token use varies sharply by language; in less familiar languages they fail to compile, revise passing solutions and prototype in Python first. | Wu, Guha et al., "The Best Programming Language for Tokenmaxxing", 2026 | H (abstract read) | Machine economy favours the familiar; Yan Agent already takes that axis, so `.yn` need not. |
| Grammar- and type-constrained decoding cut compile errors substantially. | Geng et al. 2023; Ugare et al. (SynCode) 2024; Mündler et al. PLDI 2025 | M | An LL(1) grammar is an asset for decoding; keep every addition LL(1). |
| Static types help most when using unfamiliar or undocumented code. | Mayer, Hanenberg et al. 2012 | M | Written `pub` signatures are the right place for explicitness. |
| User-centred language design works: prototype, test with users, iterate. | Coblenz et al. (PLIERS), TOCHI 2021 | M | Contested syntax should go to a randomized comparison, as law 11 already says. |
| People argue about lexical syntax far more than semantics. | Wadler's law (folklore) | L | Decide syntax by measurement, not debate. |

### 2.4 What the experiment found in today's Yan

Running the baseline samples through the real toolchain showed four things worth a follow-up, none
of them a design question:

1. **`take` blocks a method name.** `counts.entries().sort_by_key(...).take(k)` does not parse:
   `take` is a reserved keyword (sink parameters), and the parser expects a field name after `.`.
   `take` is one of the most common iterator method names in Rust, Python's itertools and Kotlin.
   The same will happen to any keyword a library wants (`copy`, `match`, `in`). The samples use
   `.first(k)` instead. Move A1 fixes this.
2. **Recursion needs `decreases`.** The checker rejects the recursive evaluator without a
   `decreases` clause; `decreases e` (structural) is accepted, `decreases e.size()` is not yet
   supported. Every T3 sample therefore carries `decreases e`. This is a good default and the
   syntax already handles it well.
3. **`fail` is not inferred on non-`pub` functions yet.** Spec 15.3 says private functions may omit
   `fail` and `uses`, but `pay_rent` without `fail TransferErr` is rejected for passing a failure up
   with `?`. The baseline follows the spec; the toolchain gap is an elaboration task.
4. **Not yet supported:** unit literals (`2s`) and the `Entry` type. Expected before M3; noted only so
   readers do not mistake these errors for parse failures.

Also confirmed: single-field variants already take positional arguments and positional patterns
(`Num(1.0)`, `Num(v) -> v`), and that text is canonical. Much of what a brevity proposal wants is
already legal; the spec's own examples just do not use it.

### 2.5 Where the baseline stands

Strengths: no `def` noise, inferred clauses on private code, clause-form functions, `else` as one
fallback for options and failures, `name:` punning, units and dates as literals, contracts and
examples at the top of the body, `par` as an ordinary block.

Gaps found while writing the samples:

- Lambdas are single expressions, so a library cannot offer a block-shaped control structure
  (`retry`, `transaction`, `scope`) without the caller writing a named helper.
- `par for` is a value but plain `for` is not, so the parallel form is more expressive than the
  sequential one.
- A guard (`if bad` then `fail`) always takes two lines, so validation-heavy functions are
  twice as tall as their logic.
- The implicit subject covers `.a.b(...)` chains but not small compound keys such as
  `(-.value, .key)`, while the column-expression rule for tables already accepts several subjects in
  one argument: two rules where one would do.
- Keywords cannot be method names (finding 1).

## 3. The shared tasks

| Task | What it exercises |
| --- | --- |
| T1 quotes | failure types, `?` and `else fail`, capabilities, bounded `par for`, a timeout scope |
| T2 top words | a mutable map, a loop, a sort key, a method chain |
| T3 evaluator | a recursive sum type, exhaustive `match`, examples, a guard |
| T4 transfer | an inout parameter and `&` at the call site, preconditions, guards, labelled arguments |

Baseline (today's Yan, checked by the toolchain apart from the gaps in 2.4):

```
type Quote = { sym: str, px: float }

type PortErr = MissingQuote(sym: str) | Fetch(msg: str)

fetch_quote(sym: str) -> Quote =
  let body = http.get_text("https://px.example/\(sym)")?
  let px = json.parse_float(body) else fail Fetch(msg: "bad price: \(sym)")
  Quote(sym: copy sym, px:)

pub quotes(syms: List[str]) -> List[Quote] fail PortErr | Timeout uses net =
  with timeout(2s)
    let qs = par (limit: 8) for s in syms
      fetch_quote(s)?
    return qs
```

```
pub top_words(path: Path, k: int) -> List[Entry[str, int]] fail IoError uses fs =
  let text = fs.read_text(path)?
  var counts: Map[str, int] = {}
  for w in text.lower().split_whitespace()
    counts[w] += 1
  counts.entries().sort_by_key(e -> (-e.value, e.key)).first(k)
```

```
type Expr = Num(v: float) | Var(name: str) | Add(a: Expr, b: Expr) | Div(a: Expr, b: Expr)

type EvalErr = DivZero() | Unbound(name: str)

pub eval(e: Expr, env: Map[str, float]) -> float fail EvalErr uses none =
  ex eval(Add(a: Num(v: 1.0), b: Var(name: "x")), {"x": 2.0}) == 3.0
  decreases e
  match e
    Num(v:) -> v
    Var(name:) -> env.get(name) else fail Unbound(name: copy name)
    Add(a:, b:) -> eval(a, env)? + eval(b, env)?
    Div(a:, b:) ->
      let d = eval(b, env)?
      if d == 0.0
        fail DivZero()
      eval(a, env)? / d
```

```
type Account = { id: int, balance: int }

type TransferErr = Insufficient(need: int) | NoAccount(id: int)

pub transfer(var book: Map[int, Account], src: int, dst: int, amt: int) fail TransferErr uses none =
  pre amt > 0
  let a = book.get(src) else fail NoAccount(id: src)
  if a.balance < amt
    fail Insufficient(need: amt - a.balance)
  if dst not in book
    fail NoAccount(id: dst)
  book[src].balance -= amt
  book[dst].balance += amt

pay_rent(var book: Map[int, Account]) =
  transfer(&book, src: 1, dst: 2, amt: 1200)?
```

## 4. Five proposals

Each proposal pushes one axis as far as the laws allow. All four tasks for every proposal are in
[`plain-yan/`](plain-yan/); two per proposal are shown here.

### P1 Shorthand: brevity

**Thesis.** The expert's hand: every frequent construct gets its shortest unambiguous spelling, as a
calligrapher's running script abbreviates strokes without changing the character.

**Moves.** `x := e` replaces `let x = e` (`var` stays, since mutation is the marked fact);
unnamed variant fields (`Num(float)`); one-line guards `if c then fail E`; one implicit-subject rule
for compound arguments (`sort_by_key((-.value, .key))`); `par(8)` for a bounded `par for`; a `pub`
item without `uses` is pure.

```
type Expr = Num(float) | Var(str) | Add(Expr, Expr) | Div(Expr, Expr)

type EvalErr = DivZero() | Unbound(str)

pub eval(e: Expr, env: Map[str, float]) -> float fail EvalErr =
  ex eval(Add(Num(1.0), Var("x")), {"x": 2.0}) == 3.0
  decreases e
  match e
    Num(v) -> v
    Var(n) -> env.get(n) else fail Unbound(copy n)
    Add(a, b) -> eval(a, env)? + eval(b, env)?
    Div(a, b) ->
      d := eval(b, env)?
      if d == 0.0 then fail DivZero()
      eval(a, env)? / d
```

```
type Account = { id: int, balance: int }

type TransferErr = Insufficient(need: int) | NoAccount(id: int)

pub transfer(var book: Map[int, Account], src: int, dst: int, amt: int) fail TransferErr =
  pre amt > 0
  a := book.get(src) else fail NoAccount(src)
  if a.balance < amt then fail Insufficient(amt - a.balance)
  if dst not in book then fail NoAccount(dst)
  book[src].balance -= amt
  book[dst].balance += amt

pay_rent(var book: Map[int, Account]) =
  transfer(&book, src: 1, dst: 2, amt: 1200)?
```

**Result.** 39 lines, 508 lexemes: 10% under the baseline, the smallest variant. The parser rejects
exactly the new forms (`:=`, unnamed fields, `then fail`, `par(8)`).

**Costs.** `:=` breaks the `let`/`var` pair and reads as Python's walrus, which is an expression;
unnamed fields lose the names that document variants and feed `yan doc`; "pure by default on `pub`"
gives an absent clause two meanings depending on visibility, which hurts local reasoning (law 6).
`par(8)` is an unlabelled number.

### P2 Plainspoken: reading and learning

**Thesis.** Code that a Python reader can read aloud on first sight. Words for structure, the fewest
symbols, and nothing implicit that a newcomer must learn before reading.

**Moves.** `fn` before every function; prefix `try` (as in Swift and Zig) instead of postfix `?`, so
`try x` reification becomes `catch x`; Python-style `"{x}"` interpolation; match arms open with `is`
to echo `e is P`; no implicit subject (`e -> e.value`); no clause form.

```
type Quote = { sym: str, px: float }

type PortErr = MissingQuote(sym: str) | Fetch(msg: str)

fn fetch_quote(sym: str) -> Quote =
  let body = try http.get_text("https://px.example/{sym}")
  let px = json.parse_float(body) else fail Fetch(msg: "bad price: {sym}")
  Quote(sym: copy sym, px: px)

pub fn quotes(syms: List[str]) -> List[Quote] fail PortErr | Timeout uses net =
  with timeout(2s)
    let qs = par (limit: 8) for s in syms
      try fetch_quote(s)
    return qs
```

```
type Expr = Num(v: float) | Var(name: str) | Add(a: Expr, b: Expr) | Div(a: Expr, b: Expr)

type EvalErr = DivZero() | Unbound(name: str)

pub fn eval(e: Expr, env: Map[str, float]) -> float fail EvalErr uses none =
  ex eval(Add(a: Num(v: 1.0), b: Var(name: "x")), {"x": 2.0}) == 3.0
  decreases e
  match e
    is Num(v:) -> v
    is Var(name:) -> env.get(name) else fail Unbound(name: copy name)
    is Add(a:, b:) -> try eval(a, env) + try eval(b, env)
    is Div(a:, b:) ->
      let d = try eval(b, env)
      if d == 0.0
        fail DivZero()
      try eval(a, env) / d
```

**Result.** 581 lexemes, 3% over the baseline.

**Costs.** Prefix `try` does not chain: `fs.read_text(path)?.lower()` becomes
`(try fs.read_text(path)).lower()`, and the mark no longer matches Yan Agent's `?`, which law 4
wants identical in kind across surfaces. `is` arms add a word per arm and carry no information.
Removing clause form removes a printing that is already unique. `fn` is the one move with a real
argument (section 7).

### P3 Wire: machine economy

**Thesis.** Optimize for the reader that is a model or a tool: predictable item heads for grep and
patches, one way to write everything, no inference that a reader of one item must redo, and
spellings frequent in training corpora.

**Moves.** `fn` on every function; `fail` and `uses` written on every item; explicit `return`;
`"{x}"` interpolation; no clause form and no implicit subject.

```
type Quote = { sym: str, px: float }

type PortErr = MissingQuote(sym: str) | Fetch(msg: str)

fn fetch_quote(sym: str) -> Quote fail PortErr uses net =
  let body = http.get_text("https://px.example/{sym}")?
  let px = json.parse_float(body) else fail Fetch(msg: "bad price: {sym}")
  return Quote(sym: copy sym, px: px)

pub fn quotes(syms: List[str]) -> List[Quote] fail PortErr | Timeout uses net =
  with timeout(2s)
    let qs = par (limit: 8) for s in syms
      fetch_quote(s)?
    return qs
```

```
type Expr = Num(v: float) | Var(name: str) | Add(a: Expr, b: Expr) | Div(a: Expr, b: Expr)

type EvalErr = DivZero() | Unbound(name: str)

pub fn eval(e: Expr, env: Map[str, float]) -> float fail EvalErr uses none =
  ex eval(Add(a: Num(v: 1.0), b: Var(name: "x")), {"x": 2.0}) == 3.0
  decreases e
  match e
    Num(v:) -> return v
    Var(name:) -> return env.get(name) else fail Unbound(name: copy name)
    Add(a:, b:) -> return eval(a, env)? + eval(b, env)?
    Div(a:, b:) ->
      let d = eval(b, env)?
      if d == 0.0
        fail DivZero()
      return eval(a, env)? / d
```

**Result.** 587 lexemes, 4% over the baseline.

**Finding.** Pushed to its end, the machine axis turns plain Yan into Yan Agent with two-space
indentation. Yan Agent already owns this axis, and the tokenmaxxing study says the gain comes from
corpus familiarity, which Yan Agent gets by being Python-shaped. So plain Yan should take from P3
only what is free: LL(1) additions, grep-able item heads (already true at column 0), and stable line
shapes for patches.

### P4 Ledger: explicitness and safety

**Thesis.** Every fact the core knows is on the page: authority, failure, partiality, effects of
time, termination. The program is its own audit log.

**Moves.** `fail` and `uses` on every item, including `-> ()`; capabilities as receivers
(`net.get_text`, not `http.get_text`), so the use-site mark for authority is the capability's own
name; timers need `uses clock`; partial operations marked `!` (`book[src]!`); moves marked at call
sites (`take w`); `old(e)` in postconditions.

```
type Account = { id: int, balance: int }

type TransferErr = Insufficient(need: int) | NoAccount(id: int)

pub transfer(var book: Map[int, Account], src: int, dst: int, amt: int) -> () fail TransferErr uses none =
  pre amt > 0
  post book.values().sum(.balance) == old(book.values().sum(.balance))
  let a = book.get(src) else fail NoAccount(id: src)
  if a.balance < amt
    fail Insufficient(need: amt - a.balance)
  if dst not in book
    fail NoAccount(id: dst)
  book[src]!.balance -= amt
  book[dst]!.balance += amt

pay_rent(var book: Map[int, Account]) -> () fail TransferErr uses none =
  transfer(&book, src: 1, dst: 2, amt: 1200)?
```

**Result.** 621 lexemes, 10% over the baseline, the largest variant; T4 alone grows 25%.

**Costs and value.** Most of P4 is already visible in today's tooling as inlay hints (inferred
clauses, bounds checks), so writing it down mostly adds noise. Two ideas survive as questions:
capabilities as receivers would make law 4's capability mark literal at the call site, which today
relies on module names such as `http.` and `fs.`; and `old(e)` would let `post` state conservation
laws. Both are standard-library and contract questions, not syntax.

### P5 Loom: expressive power

**Thesis.** A small set of forms that compose: everything that has a value can stand where a value
goes, and libraries can add control structures without macros.

**Moves.** `for` and `with` as values (as `par for` already is); trailing block lambdas
(`retry(times: 3) ->` followed by a block); one implicit-subject rule; example tables; typed holes
(`...`); one-line guards.

```
type Quote = { sym: str, px: float }

type PortErr = MissingQuote(sym: str) | Fetch(msg: str)

fetch_quote(sym: str) -> Quote =
  let body = http.get_text("https://px.example/\(sym)")?
  let px = json.parse_float(body) else fail Fetch(msg: "bad price: \(sym)")
  Quote(sym: copy sym, px:)

pub quotes(syms: List[str]) -> List[Quote] fail PortErr | Timeout uses net, time =
  retry(times: 3, backoff: 100ms) ->
    with timeout(2s)
      par (limit: 8) for s in syms
        fetch_quote(s)?
```

```
pub top_words(path: Path, k: int) -> List[Entry[str, int]] fail IoError uses fs =
  let words = for w in fs.read_text(path)?.split_whitespace()
    w.lower()
  words.tally().entries().sort_by_key((-.value, .key)).first(k)
```

```
type Expr = Num(v: float) | Var(name: str) | Add(a: Expr, b: Expr) | Div(a: Expr, b: Expr)

type EvalErr = DivZero() | Unbound(name: str)

pub eval(e: Expr, env: Map[str, float]) -> float fail EvalErr uses none =
  ex eval(_, {"x": 2.0}) for
    Num(v: 1.0) -> 1.0
    Add(a: Num(v: 1.0), b: Var(name: "x")) -> 3.0
    Div(a: Num(v: 1.0), b: Num(v: 0.0)) -> fail DivZero()
  decreases e
  match e
    Num(v:) -> v
    Var(name:) -> env.get(name) else fail Unbound(name: copy name)
    Add(a:, b:) -> eval(a, env)? + eval(b, env)?
    Div(a:, b:) ->
      let d = eval(b, env)?
      if d == 0.0 then fail DivZero()
      eval(a, env)? / d
```

**Result.** 601 lexemes, 6% over the baseline, but P5's T1 adds a retry policy and its T3 adds two
examples; like-for-like it is smaller than the baseline (the mix measures this, section 6.4). T2 is
the shortest of any variant (74 lexemes against 92).

**Costs.** Trailing lambdas invite library-defined dialects (Kotlin's DSL problem); the mix limits
them to block bodies so there is still one spelling. `atomically(&book) b ->` in P5's T4 shows the
power and the risk: it reads well and hides which name is the real place.

### 4.6 Scorecard

Scores are judgments from 1 (poor) to 5 (strong) on the samples; lexemes are measured.

| Dimension | Baseline | P1 | P2 | P3 | P4 | P5 | Mix |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Terseness | 4 | 5 | 3 | 3 | 2 | 4 | 5 |
| Role-expressiveness | 4 | 3 | 5 | 4 | 4 | 4 | 4 |
| Hidden dependencies (5 = few) | 4 | 3 | 5 | 5 | 5 | 3 | 4 |
| Error-proneness (5 = safe) | 4 | 3 | 4 | 4 | 5 | 4 | 4 |
| Consistency | 3 | 3 | 3 | 4 | 3 | 4 | 5 |
| Viscosity (5 = easy to change) | 3 | 4 | 3 | 3 | 2 | 4 | 4 |
| Closeness to corpus | 4 | 3 | 5 | 5 | 3 | 4 | 4 |
| Law fit | 5 | 3 | 3 | 5 | 4 | 4 | 5 |
| Lexemes (4 tasks) | 566 | 508 | 581 | 587 | 621 | 601 | 582 (540 like-for-like) |

## 5. Ideas beyond the proposals

Brainstormed during the work; none is adopted, some belong to other surfaces or to the editor.

- **Typographic lens.** The text stays ASCII; the editor may render `->` as an arrow, `fail` and
  `uses` in the seal colour, and the five facts as gutter marks. Beauty lives in rendering, plainness
  in text (Lean's input method and Fira Code ligatures show people like this).
- **Semantic zoom.** Fold any module to signatures, contracts and examples: the review view of a
  module is its interface plus its examples.
- **Hole-driven agent loop.** A human writes the signature, the `ex` lines and `...`; the agent fills
  holes until the examples pass. Holes become the hand-off point between human and agent.
- **Example tables as a live grid.** The editor renders an `ex` table with a pass or fail column.
- **Facts-only diff.** A review diff that shows when a change adds a capability, a failure or a
  mutation, before showing the lines.
- **Trains, user operators and stencils.** Rejected for plain Yan; they belong to Yan Meta.

## 6. The mix

### 6.1 The framework: the Yan hand

Seven rules that produced the mix and can judge any later `.yn` proposal.

1. **Marks for facts, words for structure, symbols for data.** The five facts get short marks (`?`,
   `&`, `uses`, `fail`, `par`); control flow gets words; literals get symbols.
2. **One printing, chosen by rule.** A shorthand is allowed only if the formatter can decide, from
   the tree alone, when to print it.
3. **Guards are lines, logic is blocks.** Exits fit on one line; work gets indentation.
4. **A shorthand pays twice.** It must cut tokens and either cut a concept or merge two rules into
   one. Saving characters alone is not enough.
5. **Borrow from the corpus.** Prefer a spelling readers and models already know (Python, Rust, Swift)
   unless evidence shows a better one.
6. **Plain text, beautiful rendering.** ASCII in files; typography in the editor.
7. **Infer what is local, write what crosses a boundary.** Private code infers `fail` and `uses`;
   `pub` signatures are written in full.

### 6.2 Adopted moves

| # | Move | From | Grammar and printing | Lowers to |
| --- | --- | --- | --- | --- |
| A1 | Any keyword is a name after `.` | experiment | The lexer reads the word after `.` as a name; one token of lookahead | Ordinary method or field access |
| A2 | One-line guard `if c then fail E` (also `return`, `break`, `continue`, `unreachable`) | P1, P5 | `then` followed by a diverging keyword starts a guard, decided by one token; printed on one line exactly when the `if` has no `else`, its body is one diverging statement, and it fits in 100 columns | The two-line `if` |
| A3 | `for` and `with` as values | P5 | A `for` whose body ends in a non-unit expression collects a `List` in input order, the rule `par for` already follows; `with` yields its block's value | The loop with an accumulator; the handler block |
| A4 | Trailing block lambda as a last argument | P5 | A call, optional parameters, `->`, end of line, indented block; printed trailing exactly when the last argument is a lambda with a block body (today lambdas have no block body, so no second spelling arises) | An ordinary closure argument |
| A5 | One implicit-subject rule | P1, P5 | An argument containing `.name` subjects, not nested in another call's argument, is one closure over one parameter, the rule column expressions already follow; a lambda whose parameter is only used to start field chains prints as the subject form | A closure, or a column expression for tables |
| A6 | Example tables | P5, Pyret | `ex` call with one `_`, then `for`, then rows `input -> expected` (or `-> fail E`); printed as a table when three or more consecutive `ex` lines share the callee and differ in one argument | One `ex` per row |
| A7 | Typed hole `...` as an expression | P5, Hazel, Lean | `...` is an atom; the checker reports the expected type and the bindings in scope, and a release build rejects it | A diverging placeholder |

Also kept from the proposals without new syntax: positional single-field variants in examples
(already legal and canonical), and P4's explicitness as a view (`yan view` printing inferred clauses),
not as text.

### 6.3 Declined

| Idea | From | Why not |
| --- | --- | --- |
| `x := e` | P1 | Saves 3 characters per binding, breaks the `let`/`var` pair, and borrows the look of Python's walrus with a different meaning |
| Unnamed variant fields | P1 | Positional use is already legal with named fields; names document the variant |
| Bare field-less variants (`DivZero`) | P1, considered | `type C = Red \| Green` would be ambiguous between a sum and a union of types; `B()` exists for this |
| Pure-by-default `pub` | P1 | An absent clause would mean "inferred" on private items and "none" on `pub` |
| `par(8)` | P1 | An unlabelled number; `limit:` reads |
| Prefix `try` | P2 | Does not chain; differs from Yan Agent's `?` |
| `is` before match arms | P2 | A word per arm, no information |
| Dropping clause form, adding `return` | P2, P3 | Clause form already has one printing; `return` at the end is noise |
| `fail` and `uses` on every item | P3, P4 | Already shown as inlay hints; written only on `pub` (rule 7) |
| `!` on partial operations | P4 | The bounds-check hint exists, and `@forbid(panic)` covers strict modules |
| `take` at call sites | P4 | Moves at last use are inferred on purpose; the core keeps them explicit |

### 6.4 The mix on the four tasks

Like-for-like (T1 without the retry policy, T3 with one example) the mix is 39 lines and 540 lexemes,
5% under the baseline. As written below, with the retry and the example table, it is 582.

```
type Quote = { sym: str, px: float }

type PortErr = MissingQuote(sym: str) | Fetch(msg: str)

fetch_quote(sym: str) -> Quote =
  let body = http.get_text("https://px.example/\(sym)")?
  let px = json.parse_float(body) else fail Fetch("bad price: \(sym)")
  Quote(sym: copy sym, px:)

pub quotes(syms: List[str]) -> List[Quote] fail PortErr | Timeout uses net, time =
  retry(times: 3, backoff: 100ms) ->
    with timeout(2s)
      par (limit: 8) for s in syms
        fetch_quote(s)?
```

```
pub top_words(path: Path, k: int) -> List[Entry[str, int]] fail IoError uses fs =
  var counts: Map[str, int] = {}
  for w in fs.read_text(path)?.lower().split_whitespace()
    counts[w] += 1
  counts.entries().sort_by_key((-.value, .key)).take(k)
```

```
type Expr = Num(v: float) | Var(name: str) | Add(a: Expr, b: Expr) | Div(a: Expr, b: Expr)

type EvalErr = DivZero() | Unbound(name: str)

pub eval(e: Expr, env: Map[str, float]) -> float fail EvalErr uses none =
  ex eval(_, {"x": 2.0}) for
    Num(1.0) -> 1.0
    Add(a: Num(1.0), b: Var("x")) -> 3.0
    Div(a: Num(1.0), b: Num(0.0)) -> fail DivZero()
  decreases e
  match e
    Num(v) -> v
    Var(n) -> env.get(n) else fail Unbound(copy n)
    Add(a:, b:) -> eval(a, env)? + eval(b, env)?
    Div(a:, b:) ->
      let d = eval(b, env)?
      if d == 0.0 then fail DivZero()
      eval(a, env)? / d
```

```
type Account = { id: int, balance: int }

type TransferErr = Insufficient(need: int) | NoAccount(id: int)

pub transfer(var book: Map[int, Account], src: int, dst: int, amt: int) fail TransferErr uses none =
  pre amt > 0
  let a = book.get(src) else fail NoAccount(src)
  if a.balance < amt then fail Insufficient(amt - a.balance)
  if dst not in book then fail NoAccount(dst)
  book[src].balance -= amt
  book[dst].balance += amt

pay_rent(var book: Map[int, Account]) =
  transfer(&book, src: 1, dst: 2, amt: 1200)?
```

Measured sizes, all variants:

| Variant | Lines | Non-space chars | Lexemes | Lexemes vs baseline |
| --- | --- | --- | --- | --- |
| baseline | 44 | 1437 | 566 | +0% |
| p1-shorthand | 39 | 1322 | 508 | -10% |
| p2-plainspoken | 45 | 1513 | 581 | +3% |
| p3-wire | 44 | 1526 | 587 | +4% |
| p4-ledger | 45 | 1577 | 621 | +10% |
| p5-loom | 43 | 1511 | 601 | +6% |
| mix | 43 | 1472 | 582 | +3% |

## 7. Open questions

1. **`fn` or no `fn`.** Today an item at column 0 is recognizable without a keyword, and the mix keeps
   it that way. The case for `fn` is newcomer reading (a definition and a call look alike at line
   start) and agent editing. The Plan A harness can test it cheaply, since Yan Agent has `def`.
2. **`\(x)` or `{x}` interpolation.** `\(x)` keeps braces literal (good for JSON and regex templates);
   `{x}` is what Python readers and models expect. Measure model errors on interpolation in the
   harness before changing anything.
3. **Capabilities as receivers.** Should the standard library put authority-using operations on the
   capability (`net.get_text`) so law 4's mark appears at the call site? A standard-library design
   question.
4. **`old(e)` in `post`.** Does the core already have a pre-state form for contracts? If not, it is a
   core question, not a surface one.
5. **Trailing lambdas and clause form.** A clause-form body reads a line as an arm when `->` follows it;
   a trailing lambda line also ends in `->`. A call is never a pattern, so they do not collide, but the
   RFC must state the factoring.
6. **Example-table printing.** Should the formatter turn three or more matching `ex` lines into a table
   automatically (one printing), or should tables and lines both be canonical? The rule in A6 keeps
   one printing; a randomized comparison could check readers prefer it.
7. **The `take` fix.** A1 could land on its own, before any RFC for the rest, since it only widens
   what parses.

## Sources

- Stack Overflow Developer Survey 2025, technology section: https://survey.stackoverflow.co/2025/technology/
  (figures as reported at https://byteiota.com/gleam-hits-70-developer-admiration-stack-overflow-survey/)
- Stefik and Siebert, "An Empirical Investigation into Programming Language Syntax", ACM TOCE 2013:
  https://neverworkintheory.org/2014/01/29/stefik-siebert-syntax.html
- Wu, Guha et al., "The Best Programming Language for Tokenmaxxing", 2026:
  https://www.khoury.northeastern.edu/~arjunguha/main/papers/wu-tokenmaxxing.html
- Sharif and Maletic, "An Eye Tracking Study on camelCase and under_score Identifier Styles", ICPC
  2010: https://ieeexplore.ieee.org/document/5521745/
- Cited from memory, not re-fetched in this study: Green and Petre 1996 (Cognitive Dimensions);
  Pane, Ratanamahatana and Myers 2001; Buse and Weimer 2010; Binkley et al. 2009; Hindle et al. 2012;
  Mayer, Hanenberg et al. 2012; Geng et al. 2023; Ugare et al. 2024; Mündler et al. PLDI 2025;
  Coblenz et al. 2021; Go developer surveys; Roc, Gleam, Pyret, Lean and Hazel documentation.
