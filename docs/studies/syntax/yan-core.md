# Yan Core (`.yir`) syntax study

Study, 2026-11-04. Brainstorm only: no spec, conformance or row changes. Question (Antonio): research
the peers of Yan Core 言核, find what works, write five proposals that each push one axis, then mix
them into one opinionated design for the core text. Read against main `dd33531`.

## 1. Methodology and process

**What Yan Core is for.** Yan Core is not a language people write programs in. It is the one text
every surface lowers to, and it has four readers, each with its own needs:

| Reader | Needs | Where in the spec |
| --- | --- | --- |
| The hasher | One canonical printing per item; locals by position; nothing hash-relevant left implicit | 3.6, R519 |
| The checker and interpreter | An unambiguous, fast grammar; every convention, capability and type stated | 3.5, law 5 |
| The person debugging | Can read a dump, grep it and diff it; can map a line back to the surface | 16 (raise, R565) |
| The test author | Can hand-write a small `.yir` golden file without fighting the syntax | 24, `conformance/yir/` |

A design that serves only one reader fails the others: LLVM IR serves the checker and hurts the test
author, GHC Core dumps serve the compiler author and almost nobody else. So the study scores every
proposal against all four readers, and each proposal pushes one axis to its limit to see what that
axis buys and costs.

**What I read in the repo.** `docs/surfaces.md` section 1; spec 3.5 (the complete grammar) and 3.6
(item identity, positional locals); the twelve files in `conformance/yir/run/` (closely:
`failure_propagation`, `join_points_and_match`, `become_constant_stack`, `par_join`, `hello`); the
row texts for R260, R456, R519, R565 and R772 as quoted in 3.5 and 3.6. I did not read the rest of
`spec/` or the compiler crates.

**What I searched and read outside.** Web searches on MLIR's generic and custom assembly forms,
token efficiency of code syntax for LLMs, join points in Lean 4 and GHC, and Unison's
content-addressed code. Read in full or in summary: Token Sugar (Sun et al. 2025), the
"Tokenmaxxing" agent study (Wu, Guha, Anderson 2026), and Lambda the Ultimate SSA (Bhat and Grosser,
CGO 2022). The rest of the peer table comes from my own knowledge of those systems and is marked
with a confidence level.

**What I tried.** I wrote the same four tasks in the current syntax and in each of the five
proposals and the mix (section 4 and 5), then measured tokens, characters and non-blank lines with a
real BPE tokenizer. The tasks:

- **T1, failure:** `check` fails with a record on negative input; `twice` propagates with `?`.
- **T2, match:** area of a three-variant sum, a value-producing `match` through a join point.
- **T3, tail loop:** a counter with `requires` and `decreases`, ending in `become`.
- **T4, effects:** an `inout` helper, a `par` block, and a capability passed to `std.io.print`.

**Tokenizer caveat.** The cloud proxy blocks the current OpenAI and Hugging Face tokenizer files, and
the project has no Anthropic API key yet, so I used the Claude tokenizer that shipped inside the
`anthropic` Python SDK 0.34 (a 2023-era BPE, not the tokenizer of current models). Absolute counts
will differ on current models; the ranking between proposals is the useful part (confidence medium).
The measured text is exactly the code blocks in sections 4 and 5, split at the task comments, so the
table can be rerun on a current tokenizer through the count-tokens API once the key is in the cloud
environment.

## 2. Findings

### 2.1 Peers

| System | Text shape | What works | What does not | Confidence |
| --- | --- | --- | --- | --- |
| LLVM IR | Flat SSA, `%` sigils, typed every use | Parses and prints round-trip; tests (`FileCheck`) are written against it | Very long; phis are hard to read; types repeated on every operand | High |
| MLIR | Generic op form plus per-dialect custom assembly formats; nested regions | The generic form always round-trips, the custom form is readable; regions give structured control flow inside SSA | Two printings per op double the surface area; custom formats drift between dialects | High |
| Cranelift CLIF | Flat SSA with block parameters instead of phis | Block parameters are easy to read and to write by hand in filetests | Low-level only; no sums, no structured `match` | High |
| WebAssembly text | S-expressions; folded and linear forms are both valid | Trivial to parse; the folded form reads like a tree | Two valid shapes for one module, so text is not canonical; parenthesis noise | High |
| GHC Core | Small typed lambda calculus; join points since 2017 | Join points (Maurer et al. 2017) keep code in direct style without CPS | Dumps are notoriously hard to read: generated names, coercions, type applications everywhere | High (join points), medium (readability folklore) |
| Lean 4 IR and LCNF | ANF with `jp` join points and explicit `inc`/`dec` | Very close to Yan Core; shows ANF plus join points scales to a real compiler | Reference counting is printed into the IR; Yan has no implicit RC | High |
| Koka core (Perceus) | Explicit `dup`/`drop`/reuse after passes | Precise, reuse made visible (Reinking et al. 2021) | Only meaningful after the passes, same as Yan's `drop` and `reuse` | High |
| Swift SIL (OSSA) | SSA with ownership conventions (`@owned`, `@guaranteed`, `copy_value`, `begin_borrow`) | The closest peer to `let`/`inout`/`sink`: conventions are part of the text and checked | Verbose; many instruction kinds | High |
| Rust MIR | Places, `move _1`, `copy _2`, numbered locals | Ownership operations explicit; numbered locals like R519 | The printed MIR cannot be parsed back, so it cannot carry tests (only the `custom_mir` macro can) | Medium-high |
| QBE IL | Tiny SSA text, few types, sigils by kind (`%` temp, `$` global, `@` block) | Readable and hand-writable; whole backend is small | Too small a type system for Yan | High |
| RVSDG | Structured regions for branches, loops and functions | Regions make parallel structure and dependencies explicit | Graph first; text form is secondary | Medium |
| V8 Turbofan to Turboshaft | Sea of Nodes replaced by a CFG IR (2025) | The V8 team cited debuggability and readability of the IR as a reason | Lesson only: an IR nobody can read costs real engineering time | Medium-high |
| Unison | Code stored as hashed syntax trees; names are metadata | Renames are free; Yan already has this through positional locals (R519) | Needs tooling for everything, since text is a view | High |

### 2.2 Research

- **ANF, CPS, SSA are one family.** ANF (Flanagan, Sabry, Duba, Felleisen, PLDI 1993), CPS (Kennedy,
  ICFP 2007) and SSA ("SSA is functional programming", Appel 1998) are inter-translatable. Join
  points (Maurer, Downen, Ariola, Peyton Jones, PLDI 2017) and Cong et al. (ICFP 2019, "Compiling
  with continuations, or without? Whatever.") show that direct-style ANF with second-class join
  points gets the benefits of CPS without its readability cost. Yan Core's `jp` already sits at this
  sweet spot. Confidence high.
- **Regions inside SSA work for functional code.** Bhat and Grosser (CGO 2022) put Lean 4's IR into
  MLIR with regions as values; the backend passes all 648 Lean tests at parity with the C backend
  (1.09x geomean). Structured regions do not cost performance. Confidence high.
- **Syntax and novices.** Stefik and Siebert (ACM TOCE 2013) found that for novices, symbol-heavy
  syntax (Perl, Java) was not measurably more intuitive than a randomly designed syntax, while word
  choice mattered. This argues for words where meaning is semantic (authority, failure) and symbols
  only where they are frequent and learned once. Confidence high.
- **Tokens and syntax.** Token Sugar (Sun et al., arXiv 2512.08266, 2025) cut tokens 15% on LeetCode
  and 13% on HumanEval with reversible shorthands, with pass@1 almost unchanged; its predecessor
  SimPy (Sun et al., ISSTA 2024) cut indentation and delimiter tokens. The 2026 agent study by Wu,
  Guha and Anderson (arXiv 2607.22807) found OCaml cost 1.3x to 1.7x Python's tokens, but not
  because the code was longer: agents looped on compile errors in less familiar syntax. Familiar
  shapes matter more than raw length. Confidence high for the numbers, medium for transfer to IR
  text, which agents read far more than they write.

### 2.3 Audit of today's `.yir`

What already works and should stay: ANF with every value named and typed; join points that map one
to one to Cranelift block parameters; capabilities as explicit `cap` parameters; contracts in the
header; positional locals in the hashed text; braces and `--` comments.

Friction found in the five conformance files:

1. **`let` means two things.** It binds (`let y: i64 = ...`) and it is the shared-borrow argument
   convention (`check(let x)`). A line like `let neg: bool = lt_i64(let x, let 0)` has three `let`s
   doing two jobs. The convention keyword on every argument is also the main source of length.
2. **Literals carry a convention.** `let 0` and `let 1` say nothing: a literal can only be passed by
   value.
3. **Conventions are not uniform.** Calls write `let`/`inout`/`sink` on each argument, but `jump k(a)`
   and constructor fields take bare atoms.
4. **`par` leaks names implicitly.** In `par_join.yir`, `a` and `b` are bound inside branch blocks and
   used after the `par`. Nothing in the text says which names escape; a reader must know the rule.
5. **Intrinsic names read inside out.** `add_checked_i64` puts the type last and splits into six
   tokens; `i64.add` groups by type, greps by type and is four tokens.
6. **Field patterns repeat names.** `Rect { w: w, h: h }`.
7. **Statement separation is implicit.** `{ let a: i64 = ...  jump k(a) }` relies on two spaces.

### 2.4 Experiment

Same four tasks in each design (section 4). Tokens with the legacy Claude tokenizer; chars include
whitespace; lines are non-blank.

| Proposal | T1 | T2 | T3 | T4 | Total tokens | vs baseline | Chars | Lines |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Today (3.5) | 130 | 152 | 120 | 199 | 601 | 1.00 | 1474 | 57 |
| 4.1 Terse | 109 | 123 | 96 | 154 | 482 | 0.80 | 899 | 45 |
| 4.2 Prose | 132 | 145 | 109 | 183 | 569 | 0.95 | 1530 | 75 |
| 4.3 Line | 100 | 119 | 87 | 145 | 451 | 0.75 | 998 | 60 |
| 4.4 Ledger | 164 | 172 | 142 | 263 | 741 | 1.23 | 1717 | 57 |
| 4.5 Tree | 116 | 117 | 100 | 178 | 511 | 0.85 | 1269 | 44 |
| 5 Mix | 121 | 134 | 104 | 189 | 548 | 0.91 | 1264 | 57 |

Ablations on the current syntax:

| Variant | T1 | T2 | T3 | T4 | Total tokens | vs baseline | Chars | Lines |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Today | 130 | 152 | 120 | 199 | 601 | 1.00 | 1474 | 57 |
| Today, bare shared-borrow args and `~` at call sites only | 125 | 148 | 110 | 187 | 570 | 0.95 | 1353 | 57 |
| Today, dotted intrinsics only | 128 | 148 | 116 | 193 | 585 | 0.97 | 1410 | 57 |
| Mix, 4-space indent | 121 | 134 | 104 | 189 | 548 | 0.91 | 1264 | 57 |
| Mix, 2-space indent | 121 | 134 | 104 | 189 | 548 | 0.91 | 1178 | 57 |

What the numbers say:

- **Characters and tokens diverge.** The terse design cuts 39% of characters but only 20% of tokens:
  dropped spaces and keywords were cheap tokens already. Optimizing characters is the wrong target.
- **Indentation is free** in this tokenizer (runs of spaces merge into one token), so two versus four
  spaces is a style choice, not a cost choice.
- **The flat block form is the cheapest** (25% fewer tokens) because it drops parentheses, commas and
  braces at once. It is also the least readable for nested `match`.
- **Explicitness costs about a quarter.** The ledger design (every move, copy, literal type and
  effect spelled) costs 23% more than today.
- **Most of the mix's gain comes from two changes**: bare shared-borrow arguments (5%) and dotted
  intrinsics (3%); field punning and the rest add the remainder, while `par` outputs add a little
  back.

## 3. Frameworks

Three ideas organize the proposals and the mix.

**Positional truth, named view.** The hash already sees positions, not names (R519). Push this to
its end: everything a reader wants that the hash does not need (names, origins, effect summaries,
inferred-elsewhere facts) is trivia that the printer may add and the parser drops. One grammar, two
printings: the canonical printing (what is hashed and diffed) and the annotated printing (what a
person debugs). MLIR has the two-printings idea but with two grammars; Yan can have it with one.

**Sigils for ownership, words for authority.** Ownership conventions occur on almost every line, so
they earn a one-character mark learned once. Authority (`cap`), failure (`fail`) and control
(`ret`, `jump`, `become`, `par`) occur rarely and carry the safety story, so they stay words that a
reviewer can grep for and that a novice can read (Stefik and Siebert).

**Every line answers three questions.** What is bound (left of `=`), at what type (after `:`), by
what operation (right of `=`). A statement that does more than that is a control word at the start
of the line. Keyword-first lines make the grammar LL(1) and keep diffs line-local.

## 4. Five proposals

Each proposal pushes one axis and writes all four tasks. `-- T2` style lines separate the tasks.

### 4.1 Terse: brevity first

Rules: no `let` on bindings (`x:T=rhs`); `!E` for `fail E`; sigils `~` inout and `^` sink, bare is
shared borrow; `;` joins statements; field punning; `()` for `Unit`; no optional spaces.

```
-- T1: failure
module demo.t1
record Bad{code:i64}
fn check(x:i64)->i64!Bad{
  neg:bool=i64.lt(x,0)
  if neg{e:Bad=Bad{code:x};fail e}
  ret x
}
fn twice(x:i64)->i64!Bad{
  y:i64=check(x)?
  z:i64=i64.add(y,y)
  ret z
}

-- T2: match through a join point
module demo.t2
sum Shape{Circle{r:i64},Rect{w:i64,h:i64},Dot}
fn area(s:Shape)->i64{
  jp k(a:i64){ret a}
  match s{
    Circle{r}=>{a:i64=i64.mul(r,r);jump k(a)}
    Rect{w,h}=>{a:i64=i64.mul(w,h);jump k(a)}
    Dot=>{jump k(0)}
  }
}

-- T3: tail loop with contracts
module demo.t3
fn count(n:i64,acc:i64)->i64 requires i64.ge(n,0) decreases n{
  z:bool=i64.eq(n,0)
  if z{ret acc}
  m:i64=i64.sub(n,1)
  a:i64=i64.add(acc,1)
  become count(m,a)
}

-- T4: inout, par, capability
module demo.t4
fn bump(~n:i64)->(){
  m:i64=i64.add(n,1)
  n=m
  ret ()
}
fn sq(x:i64)->i64{y:i64=i64.mul(x,x);ret y}
fn main(cap io:Io)->(){
  par{{a:i64=sq(2)}{b:i64=sq(3)}}
  var c:i64=i64.add(a,b)
  bump(~c)
  s:Str=std.text.from_i64(c)
  std.io.print(cap io,s)
  ret ()
}
```

Buys: 20% fewer tokens, 39% fewer characters, dense dumps. Costs: hard to scan; `!` for failure
hides a safety fact behind a symbol; whitespace-free text is hostile to diffs and grep.
Keep: sigils for conventions, field punning, `;`.

### 4.2 Prose: human readability first

Rules: words everywhere (`function`, `returns`, `or fails with`, `borrow`, `update`, `give`,
`capability`); `end`-closed blocks; full English intrinsic names; `join`/`go` for join points;
`finally call` for `become`.

```
-- T1: failure
module demo.t1

record Bad
  code: i64
end

function check(borrow x: i64) returns i64 or fails with Bad
  let neg: bool = i64.less_than(x, 0)
  if neg then
    let e: Bad = Bad(code: x)
    fail with e
  end
  return x
end

function twice(borrow x: i64) returns i64 or fails with Bad
  let y: i64 = check(x) or pass failure
  let z: i64 = i64.add(y, y)
  return z
end

-- T2: match through a join point
module demo.t2

choice Shape
  Circle(r: i64)
  Rect(w: i64, h: i64)
  Dot
end

function area(borrow s: Shape) returns i64
  join done(a: i64)
    return a
  end
  match s
    when Circle(r: r) then
      let a: i64 = i64.multiply(r, r)
      go done(a)
    when Rect(w: w, h: h) then
      let a: i64 = i64.multiply(w, h)
      go done(a)
    when Dot then
      go done(0)
  end
end

-- T3: tail loop with contracts
module demo.t3

function count(borrow n: i64, borrow acc: i64) returns i64
  requires i64.at_least(n, 0)
  decreases n
  let z: bool = i64.equals(n, 0)
  if z then
    return acc
  end
  let m: i64 = i64.subtract(n, 1)
  let a: i64 = i64.add(acc, 1)
  finally call count(m, a)
end

-- T4: inout, par, capability
module demo.t4

function bump(update n: i64) returns Unit
  let m: i64 = i64.add(n, 1)
  set n to m
  return ()
end

function sq(borrow x: i64) returns i64
  let y: i64 = i64.multiply(x, x)
  return y
end

function main(capability io: Io) returns Unit
  in parallel
    branch
      let a: i64 = sq(2)
    end
    branch
      let b: i64 = sq(3)
    end
  end
  variable c: i64 = i64.add(a, b)
  bump(update c)
  let s: Str = std.text.from_i64(c)
  std.io.print(capability io, s)
  return ()
end
```

Buys: a newcomer can read it cold. Costs: 32% more lines, only 5% fewer tokens, and it no longer
looks like the surfaces it lowers from, so mapping a dump back to `.yag` or `.yn` gets harder. Words
such as `borrow` and `update` are clearer than `let` and `inout`.
Keep: the insight that conventions deserve their own words, distinct from binding; dotted intrinsics.

### 4.3 Line: agent token cost and line-local edits first

Rules: a function body is a list of labelled blocks (`@name params`), as in Cranelift; no braces,
parentheses or commas in calls (juxtaposition); `br` and multi-target `match` jump to blocks; `par`
names its branch blocks and a join block.

```
-- T1: failure
module demo.t1
record Bad code:i64
fn check x:i64 -> i64 fail Bad
  neg:bool = i64.lt x 0
  br neg @bad @ok
@bad
  e:Bad = Bad code=x
  fail e
@ok
  ret x
fn twice x:i64 -> i64 fail Bad
  y:i64 = check x ?
  z:i64 = i64.add y y
  ret z

-- T2: match through a join point
module demo.t2
sum Shape Circle(r:i64) Rect(w:i64 h:i64) Dot
fn area s:Shape -> i64
  match s Circle @c Rect @r Dot @d
@c r:i64
  a:i64 = i64.mul r r
  jump @k a
@r w:i64 h:i64
  a:i64 = i64.mul w h
  jump @k a
@d
  jump @k 0
@k a:i64
  ret a

-- T3: tail loop with contracts
module demo.t3
fn count n:i64 acc:i64 -> i64
  requires i64.ge n 0
  decreases n
  z:bool = i64.eq n 0
  br z @done @step
@done
  ret acc
@step
  m:i64 = i64.sub n 1
  a:i64 = i64.add acc 1
  become count m a

-- T4: inout, par, capability
module demo.t4
fn bump ~n:i64 -> Unit
  m:i64 = i64.add n 1
  n = m
  ret ()
fn sq x:i64 -> i64
  y:i64 = i64.mul x x
  ret y
fn main cap io:Io -> Unit
  par @a @b -> @join
@a
  a:i64 = sq 2
@b
  b:i64 = sq 3
@join
  c:i64 = i64.add a b
  bump ~c
  s:Str = std.text.from_i64 c
  std.io.print cap io s
  ret ()
```

Buys: 25% fewer tokens, the smallest of all; every edit touches one line; trivially streamable and
easy to constrain with a grammar during decoding. Costs: structure is gone. A `match` with
nested data becomes a block graph, which is exactly what makes LLVM IR hard to read; juxtaposition
calls make nested types and generic arguments ambiguous; it moves Yan Core toward MIR, which already
exists one level down.
Keep: nothing structural; the measurement that delimiters, not words, are the token cost.

### 4.4 Ledger: safety and explicitness first

Rules: every header states `uses` and `panics` sets, even when empty; every argument states its
convention, every use states `move` or `copy`; literals carry type suffixes; `par` declares the
names it binds; a one-character effect gutter before each statement (`.` pure, `?` may fail, `!` may
panic, `&` mutates, `|` concurrent, `$` uses a capability).

```
-- T1: failure
module demo.t1

record Bad { code: i64 }

fn check(let x: i64) -> i64 fail Bad uses {} panics {} {
  .  let neg: bool = i64.lt(let x, let 0i64)
  .  if neg {
  .      let e: Bad = Bad { code: copy x }
  ?      fail move e
  .  }
  .  ret copy x
}

fn twice(let x: i64) -> i64 fail Bad uses {} panics {overflow} {
  ?  let y: i64 = check(let x)?
  !  let z: i64 = i64.add(let y, let y)
  .  ret move z
}

-- T2: match through a join point
module demo.t2

sum Shape { Circle { r: i64 }, Rect { w: i64, h: i64 }, Dot }

fn area(let s: Shape) -> i64 uses {} panics {overflow} {
  .  jp k(let a: i64) { ret move a }
  .  match s {
  !      Circle { r: r } => { let a: i64 = i64.mul(let r, let r)  jump k(move a) }
  !      Rect { w: w, h: h } => { let a: i64 = i64.mul(let w, let h)  jump k(move a) }
  .      Dot => { jump k(0i64) }
  .  }
}

-- T3: tail loop with contracts
module demo.t3

fn count(let n: i64, let acc: i64) -> i64 uses {} panics {overflow}
    requires i64.ge(let n, let 0i64)
    decreases n
{
  .  let z: bool = i64.eq(let n, let 0i64)
  .  if z { ret copy acc }
  !  let m: i64 = i64.sub(let n, let 1i64)
  !  let a: i64 = i64.add(let acc, let 1i64)
  .  become count(sink m, sink a)
}

-- T4: inout, par, capability
module demo.t4

fn bump(inout n: i64) -> Unit uses {} panics {overflow} {
  !  let m: i64 = i64.add(let n, let 1i64)
  &  n = move m
  .  ret ()
}

fn sq(let x: i64) -> i64 uses {} panics {overflow} {
  !  let y: i64 = i64.mul(let x, let x)
  .  ret move y
}

fn main(cap io: Io) -> Unit uses {io} panics {overflow} {
  |  par (a: i64, b: i64) {
  !      { let a: i64 = sq(let 2i64) }
  !      { let b: i64 = sq(let 3i64) }
  |  }
  !  var c: i64 = i64.add(let a, let b)
  &  bump(inout c)
  .  let s: Str = std.text.from_i64(let c)
  $  std.io.print(cap io, sink s)
  .  ret ()
}
```

Buys: a reviewer can audit authority, failure and panics by scanning one column; nothing is left to
rules the reader must know. Costs: 23% more tokens; most of the added facts (`move` versus `copy` of
an `i64`, `panics {overflow}`) are computable, so writing them by hand in tests is busywork that
the validator would reject when wrong anyway.
Keep: explicit `par` outputs; the effect gutter, but as printer trivia, never hand-written.

### 4.5 Tree: tooling and parsing first

Rules: S-expressions, one shape per construct, no infix at all; `(? call)` for propagation; `par`
declares outputs; patterns are lists.

```
;; T1: failure
(module demo.t1
  (record Bad (code i64))
  (fn check ((let x i64)) i64 (fail Bad)
    (let neg bool (i64.lt x 0))
    (if neg (block (let e Bad (Bad (code x))) (fail e)))
    (ret x))
  (fn twice ((let x i64)) i64 (fail Bad)
    (let y i64 (? (check x)))
    (let z i64 (i64.add y y))
    (ret z)))

;; T2: match through a join point
(module demo.t2
  (sum Shape (Circle (r i64)) (Rect (w i64) (h i64)) (Dot))
  (fn area ((let s Shape)) i64
    (jp k ((a i64)) (ret a))
    (match s
      ((Circle r) (let a i64 (i64.mul r r)) (jump k a))
      ((Rect w h) (let a i64 (i64.mul w h)) (jump k a))
      ((Dot) (jump k 0)))))

;; T3: tail loop with contracts
(module demo.t3
  (fn count ((let n i64) (let acc i64)) i64
    (requires (i64.ge n 0))
    (decreases n)
    (let z bool (i64.eq n 0))
    (if z (ret acc))
    (let m i64 (i64.sub n 1))
    (let a i64 (i64.add acc 1))
    (become (count m a))))

;; T4: inout, par, capability
(module demo.t4
  (fn bump ((inout n i64)) Unit
    (let m i64 (i64.add n 1))
    (set n m)
    (ret ()))
  (fn sq ((let x i64)) i64
    (let y i64 (i64.mul x x))
    (ret y))
  (fn main ((cap io Io)) Unit
    (par ((a i64) (b i64))
      (block (let a i64 (sq 2)))
      (block (let b i64 (sq 3))))
    (var c i64 (i64.add a b))
    (bump (inout c))
    (let s Str (std.text.from_i64 c))
    (std.io.print (cap io) s)
    (ret ())))
```

Buys: a parser in any language in an afternoon; structural editing and tree diff for free; the
text is already the hashed tree. 15% fewer tokens. Costs: it reads like no surface Yan has, and
WebAssembly's experience shows S-expression text invites a second, "friendlier" form. The parser
being trivial matters less than it seems: Yan Core's grammar is already keyword-led and nearly LL(1).
Keep: the rule "one shape per construct", which makes text canonical by construction.

## 5. The mix

**Name of the design: ledger cut.** Today's braces-and-ANF skeleton, with Terse's sigils, Prose's
dotted intrinsics, Ledger's `par` outputs and gutter (as trivia), and Tree's "one shape per
construct" rule.

### 5.1 Rules

1. **Conventions are sigils, everywhere the same.** In parameters, arguments, closure captures and
   join point parameters: bare is shared borrow (today's `let`), `~x` is inout, `^x` is sink. `let`
   only binds. `copy` and `move` stay words on places, where they change ownership.
2. **Authority, failure and control stay words.** `cap`, `fail`, `ret`, `jump`, `become`, `par`,
   `with`, `unsafe`, `comptime` are unchanged.
3. **Intrinsics are dotted by type.** `i64.add`, `i64.lt`, `f64.eq`. The unmarked name is the checked
   operation; the others say what they give up: `i64.add_wrap`, `i64.add_sat`.
4. **Field punning in patterns and constructors.** `Rect { w, h }` means `Rect { w: w, h: h }`. The
   canonical printer always puns when names match.
5. **`par` declares what escapes.** `par (a: i64, b: i64) { {...} {...} }`: each listed name must be
   bound in exactly one branch, and no other branch name is visible after the block.
6. **`;` separates statements on one line.** The canonical printer uses one statement per line except
   for single-statement arms.
7. **One grammar, two printings.** The canonical printing is what 3.6 hashes (after local
   renumbering). The annotated printing adds trivia the parser ignores: the effect gutter, origin
   marks for raise, and inferred facts as end-of-line comments.
8. **Canonical text is a fixed point.** For any checked item, parsing its canonical printing and
   printing again gives the same bytes. This can be a property test over the conformance corpus.

### 5.2 The four tasks

```
-- T1: failure
module demo.t1

record Bad { code: i64 }

fn check(x: i64) -> i64 fail Bad {
    let neg: bool = i64.lt(x, 0)
    if neg {
        let e: Bad = Bad { code: x }
        fail e
    }
    ret x
}

fn twice(x: i64) -> i64 fail Bad {
    let y: i64 = check(x)?
    let z: i64 = i64.add(y, y)
    ret z
}

-- T2: match through a join point
module demo.t2

sum Shape { Circle { r: i64 }, Rect { w: i64, h: i64 }, Dot }

fn area(s: Shape) -> i64 {
    jp k(a: i64) { ret a }
    match s {
        Circle { r } => { let a: i64 = i64.mul(r, r); jump k(a) }
        Rect { w, h } => { let a: i64 = i64.mul(w, h); jump k(a) }
        Dot => { jump k(0) }
    }
}

-- T3: tail loop with contracts
module demo.t3

fn count(n: i64, acc: i64) -> i64
    requires i64.ge(n, 0)
    decreases n
{
    let z: bool = i64.eq(n, 0)
    if z { ret acc }
    let m: i64 = i64.sub(n, 1)
    let a: i64 = i64.add(acc, 1)
    become count(m, a)
}

-- T4: inout, par, capability
module demo.t4

fn bump(~n: i64) -> Unit {
    let m: i64 = i64.add(n, 1)
    n = m
    ret ()
}

fn sq(x: i64) -> i64 {
    let y: i64 = i64.mul(x, x)
    ret y
}

fn main(cap io: Io) -> Unit {
    par (a: i64, b: i64) {
        { let a: i64 = sq(2) }
        { let b: i64 = sq(3) }
    }
    var c: i64 = i64.add(a, b)
    bump(~c)
    let s: Str = std.text.from_i64(c)
    std.io.print(cap io, s)
    ret ()
}
```

### 5.3 Annotated printing of T4

The same `main` as the printer would show it for debugging. The gutter, origins and end-of-line
facts are trivia: replacing each gutter character with a space and deleting the comments gives the canonical text above byte for byte. (The origin mark
spelling, `--@`, is part of the mock.)

```
fn main(cap io: Io) -> Unit {
|   par (a: i64, b: i64) {                    --@ yn 9:3
!       { let a: i64 = sq(2) }                -- panics: overflow
!       { let b: i64 = sq(3) }
|   }
!   var c: i64 = i64.add(a, b)                --@ yn 10:3
&   bump(~c)
.   let s: Str = std.text.from_i64(c)
$   std.io.print(cap io, s)                   -- uses io
.   ret ()
}
```

### 5.4 Grammar delta (mock, not spec text)

```
Param    ::= ["comptime"] [Sigil] Name ":" Type | "cap" Name ":" CapType
Arg      ::= [Sigil] Atom | "cap" Name
Sigil    ::= "~" | "^"                  -- none: shared borrow; "~": inout; "^": sink
FnType   ::= "Fn(" [[Sigil] Type {"," [Sigil] Type}] ")" "->" Type ["uses" CapSet] ["fail" Type]
Stmt     ::= ... | "par" ["concurrent"] "(" [Name ":" Type {"," Name ":" Type}] ")" "{" {Block} "}"
Block    ::= "{" [Stmt {(";" | Newline) Stmt}] "}"
Pat      ::= ... | Path "{" PField {"," PField} ["," ".."] "}"
PField   ::= Name [":" Pat]                       -- bare Name puns
Trivia   ::= Comment | Gutter | Origin            -- dropped by the parser, never hashed
Gutter   ::= one of ". ? ! & | $" in column 1, before the indentation
Origin   ::= "--@" SurfaceExt Line ":" Col
```

### 5.5 Why this mix

- It keeps the shape of the surfaces it lowers from (braces, `fn`, `let`, `?`, `fail`), so a dump
  reads like a very explicit `.yn` and raise stays obvious. This is the familiarity effect the agent
  study measured.
- It removes the one real ambiguity (`let` as binder and as convention) and the one implicit scoping
  rule (`par`).
- It cuts 9% of tokens without hiding any fact the hash, checker or reviewer needs. The facts it
  stops writing (a literal's convention, a repeated field name) carried no information.
- It moves safety facts that are computable (panics, effects per line) from hand-written text to
  printer trivia, where they are always right and never cost a test author anything.

### 5.6 Declined from the five

Juxtaposition calls and block-only bodies (Line): lose structure. S-expressions (Tree): unfamiliar,
and invite a second form. `end` blocks and long English names (Prose): long, and unlike every
surface. Hand-written `move`/`copy` on scalars and literal suffixes (Ledger): computable. Symbol
`!E` for failure (Terse): failure is a safety fact and keeps its word.

## 6. Ideas beyond syntax (for later, not in the mix)

- **Item-level structural diff.** Since every item has a hash, `yan ir diff` can report "item
  changed, item moved, item renamed (same hash)" before any line diff.
- **Gutter in every surface's tooling.** The same one-column effect summary could be an editor
  inlay for `.yag` and `.yn`, computed from the core.
- **Hole-tolerant core.** A `?hole: T` atom that the checker types but the interpreter refuses would
  let test authors sketch golden files incrementally, as Hazel does for source.
- **Grammar export.** The core grammar is small enough to export as GBNF for constrained decoding of
  hand-written golden files.

## 7. Open questions

1. **Is a missing sigil "inferred"?** Law 5 says nothing in Yan Core is inferred. A bare argument is
   a fixed spelling of one convention, not inference, but the rule "every argument states its
   convention" would need rewording. Antonio's call.
2. **Which sigils.** `~` matches Yan Meta's mutation mark; Yan Agent uses `&x`. Should Core follow one
   surface, or pick `&` for inout to match the agent surface models write most?
3. **Checked as the unmarked intrinsic.** `i64.add` meaning checked is the safe default, but it
   renames every arithmetic intrinsic. Worth it before M1, or after?
4. **Migration cost.** Every `.yir` under `conformance/` changes, and agents may not edit it. This
   needs a lead-owned PR, and every item hash changes (acceptable pre-1.0, since hashes are salted by
   toolchain version anyway).
5. **`par` outputs.** Is the header list (`par (a: i64, b: i64)`) better than a `let (a, b) = par`
   binding form? The header keeps `par` a statement and matches how `jp` declares parameters.
6. **Does the annotated printing belong in the spec**, or only in the toolchain section as a debug
   format?
7. **Rerun the token table** on a current tokenizer once the API key is in the cloud environment.

## Sources

- MLIR: Lattner et al., "MLIR: A Compiler Infrastructure for the End of Moore's Law", https://arxiv.org/abs/2002.11054
- Bhat and Grosser, "Lambda the Ultimate SSA", CGO 2022, https://arxiv.org/abs/2201.07272
- Maurer, Downen, Ariola, Peyton Jones, "Compiling without continuations", PLDI 2017, https://www.microsoft.com/en-us/research/publication/compiling-without-continuations/
- Cong, Osvald, Essertel, Rompf, "Compiling with continuations, or without? Whatever.", ICFP 2019, https://www.cs.purdue.edu/~rompf/papers/cong-icfp19.pdf
- Sun et al., "Token Sugar", 2025, https://arxiv.org/abs/2512.08266
- Wu, Guha, Anderson, "The Best Programming Language for Tokenmaxxing", 2026, https://arxiv.org/abs/2607.22807
- Unison, "The big idea", https://www.unison-lang.org/docs/the-big-idea/
- From knowledge, not re-fetched: Flanagan et al. PLDI 1993 (ANF); Appel 1998 (SSA is functional
  programming); Kennedy ICFP 2007; Ullrich and de Moura IFL 2019 (Lean λRC); Reinking et al. PLDI
  2021 (Perceus); Stefik and Siebert TOCE 2013; Sun et al. ISSTA 2024 (SimPy); Reissmann et al. TECS
  2020 (RVSDG); V8 blog 2025 on leaving Sea of Nodes; Swift SIL and Rust MIR documentation.
