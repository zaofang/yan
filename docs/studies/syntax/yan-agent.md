# Yan Agent (`.yag`) syntax study: five axes and one mix

Study, 2026-11-04. Docs only: it changes no spec section, conformance case or row. Items are labelled D1,
D2 and so on; nothing here is decided. Rows and diagnostic codes cited are existing ones; this study
allocates none. Names are frozen and used as decided (Yan Agent, seal 言信, `.yag`).

## Summary

- Yan Agent's body syntax is already where it should be: as close to typed Python as the core allows.
  Five rival designs written over the same five tasks agree on bodies and differ only in headers,
  declarations and contracts.
- Shorter spellings for their own sake save almost nothing. `fail` and `!` cost the same in o200k, and so do
  `list[str]` and `[str]`. The tokens that count come from rules: forced keywords, `Ok` wrappers,
  copies the checker demands, and annotations it cannot infer.
- The strongest evidence is what models write unprompted. In 100 Yan Agent runs of the Sonnet 5.5 probe,
  the most frequent diagnostic came from `ex` lines on fallible functions (18 runs). In 18 places the model
  invented `ex f(x) == fail E(...)` or `ex f(x) fails E(...)` without being shown either.
- The mix keeps Python's body, spends the novelty budget only in the header, and turns the model's
  desire paths into canonical forms: success-value examples (D1), `char` literals (D2), empty-collection
  inference (D3), a destructuring fallback (D4), arrow function types (D5), lenient bridges for Python
  habits (D6) and `law` contracts (D7).
- One compiler bug surfaced along the way: E413 asks for copies across mutually exclusive `return`
  branches. That forces two deep copies into a binary-tree insert, so the insert copies whole subtrees.

## 1. Methodology and process

The brief: research the niche, read the current syntax, write five proposals that each optimize one axis
over the same tasks, mix them, and record the process. The steps, in order:

1. **Read the current surface, narrowly.** `docs/surfaces.md` (status table, the five law-4 facts, section
   2), spec 13.1 and 13.2 (lexis and the complete grammar), spec 14.1, 14.3 and 14.4 (semantics, canonical
   form, lenient reader), and spec 21.2, 21.3 and 21.6 (holes, constrained decoding, the evaluation harness).
   I also read the project notes on earlier Yan Agent work: the probe report and its rerun, the ambiguity
   audit, the learnability study and the Yue token-minimal study. I did not re-read their full documents.
2. **Search the field.** Web searches on LLM-oriented grammars, token efficiency across languages,
   type-constrained decoding, LLM performance on new and low-resource languages, typed holes, vericoding,
   test-exploitation benchmarks, agent-orchestration languages, the 2026 Python typing survey and the 2025
   Stack Overflow survey. I fetched the primary page for each claim used below. Each claim is marked
   **high**, **medium** or **low** confidence.
3. **Fix five shared tasks** that exercise every law-4 fact (section 3).
4. **Write the tasks eight ways**: typed Python with pytest tests as the baseline, current Yan Agent, the
   five proposals, and the mix. All 40 files are in `/mnt/project-files/syntax-study-yag/samples/`.
5. **Check the current-Yan-Agent files with the toolchain** (`yan check`, `yan test`, built from main at
   dd33531). Three of five pass with their tests. T3 needs `http`, which the toolchain lacks. T4 hits the
   E413 false positive.
6. **Count tokens** with o200k and cl100k (js-tiktoken) and the legacy Anthropic tokenizer
   (`@anthropic-ai/tokenizer`, an approximation of current Claude tokenizers). Each file is split into
   spec lines (contracts, examples, docstring examples, and pytest tests for Python) and code lines.
   Scripts: `measure.py`, `count.js`. I also counted 28 single-construct pairs (`micro.json`,
   `micro_tokens.txt`).
7. **Mine the probe runs** (`/mnt/project-files/jian-probe/runs-v2`: Sonnet 5.5, 50 tasks, 0 and 5 shots,
   the 100 Yan Agent runs) for the diagnostics models hit and the spellings they reached for: their
   *desire paths*.
8. **Score, mix, and write the open questions.**

Limits: the samples are hand-written, so they show the best case for each design, not what a model
produces. One model family was mined. The token counts cover five small tasks. A real decision needs the
R567 harness runs of Plan A.

## 2. Findings

### 2.1 Peers and competitors

| Language | What it does for this niche | What to take | Confidence |
| --- | --- | --- | --- |
| Typed Python (mypy, pyright) | The prior every model has. 85.5% of surveyed Python developers use hints often. | The body register, wholesale. | high |
| MoonBit | Positions itself as AI-native: runs static analysis during sampling, has built-in expect tests, organizes code in blocks for context. | Its stance matches spec 21.3 and `ex`. Zero-shot results on it are still near 0 (2.2). | medium |
| Gleam | Second most admired language in 2025 (70.8%): small, one way to do things, explicit errors as values. | Evidence that "opinionated and small" is loved, not merely tolerated. | high |
| Dafny | Vericoding success 82%, against 44% for Verus and 27% for Lean. Its contracts sit inline in imperative code. | Contracts in the header, near the code, in the code's own syntax. | high |
| Pel | An agent-orchestration language: homoiconic, minimal, capabilities controlled at the syntax level. | Capabilities in syntax are Yan's `uses`. Its Lisp form costs the Python prior, so not that. | medium |
| Cangjie | A new language with a 2026 study of LLM support. | A short syntax reference helps a lot, and a Python reference hurts (2.2). | high |
| Hazel | Typed holes with static context for LLM completion. | Already in Yan (`...`, `hole_context`). | high |
| Koka, Effekt, Unison | Effect rows and abilities, written in the signature. | Yan's `uses` is a simplified effect row. Effect polymorphism shows up in D5. | medium |

### 2.2 What research says

- **New languages start near zero, and few-shot beats documentation** (high). On McEval-Hard, Gleam and
  MoonBit score 0 to 1% zero-shot, against 59 to 89% for high-resource languages. Most failures are
  syntactic, and 5-shot prompting beat retrieval over the language docs (Giagnorio et al., 2026). For
  Cangjie, a 2,146-token syntax reference lifted GPT-5 from 4.8% to 50.4% (Cheng et al., 2026).
- **Python as a reference can hurt** (high). In the same Cangjie study, syntax-constrained prompting
  with a Python reference dropped GPT-5's HumanEval pass@1 from 64.0% to 44.5%. This is the *false friend*
  effect: code that looks like Python but behaves differently invites Python behaviour. For Yan Agent,
  looking like Python is safe only where it also means what Python means.
- **Type-constrained decoding halves compile errors** (high). Mündler et al. (PLDI 2025) cut compilation
  errors by more than half on HumanEval and MBPP, and syntax-only constraints did not fix type errors.
  Spec 21.3 already plans name, type and effect masks. Syntax that keeps the type at a fixed position
  makes those masks cheap.
- **Grammar changes for AI save about 10 to 13%, but only after training** (medium). SimPy reports token
  savings of that size with comparable accuracy for fine-tuned models (Sun et al., ISSTA 2024). Hosted
  models will not be fine-tuned on Yan Agent (spec 21.6), so a lean grammar has to earn its tokens
  zero-shot.
- **Formatting costs about a quarter of input tokens** (high). Stripping formatting cut input by 24.5% with
  negligible effect on correctness (ICSE 2026). That is a tooling lever (`pack_context`), not a
  grammar one: the canonical form stays readable.
- **Languages differ 2.6x in token efficiency** (low: a blog analysis of RosettaCode, Clojure against C).
  The direction matches the Yue study: dense syntax saves less once a model's prior is counted.
- **Models exploit tests when they can** (high). ImpossibleBench measures the cheating rate as passes on
  tasks whose tests contradict the spec (Zhong et al., 2025). Yan keeps tests held out. Properties
  (D7) are harder to game than single examples.
- **What typed-Python users want** (high). In the 2026 Python typing survey, the top requested feature is
  better `Callable` syntax (45.7%). Respondents citing better AI-generated code as a reason for types rose
  from 18.5% to 38.4%, and 44.6% say the type system cannot describe what their code does.

### 2.3 Evidence from Yan's own data

**Desire paths.** Counts are the Yan Agent runs, out of 100, that hit each diagnostic at least once in
the probe rerun.

| What the model wrote | Runs | Diagnostic | What it says |
| --- | --- | --- | --- |
| `ex f(x) == v` on a fallible `f` | 18 | E315 "failure not handled", 66 hits, every top hit in an `ex` line | Models read `ex` as "the result is v". |
| `ex f(x) == fail E(...)` or `ex f(x) fails E(...)` | 18 parse errors | E100 at `fail` or `fails` | Unprompted, models invent a failure example. The two spellings are 12 and 6 of those hits. |
| `xs = []`, then `xs.append(v)` | 12 | E241 "annotate the type of this empty list" | mypy infers this (partial types). Yan does not. |
| Same-typed arguments passed positionally | 11 | E210 | Costs 4 tokens per three-field constructor call. Prevents swaps. |
| `c == "a"` with `c` from `s.chars()` | 10 | E201 "cannot compare `char` with `str`", 45 hits | A false friend: in Python, iterating a `str` yields `str`. |
| Assignment to an immutable binding | 7 | E403 | Mostly receiver calls on parameters. |

**Hand-written tokens** (o200k; cl100k is within 1%, and the legacy Anthropic tokenizer gives the same
ranking):

| Variant | T1 | T2 | T3 | T4 | T5 | Total | Spec | Code | vs Python |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Typed Python + pytest | 105 | 227 | 183 | 167 | 134 | 816 | 186 | 630 | 1.00 |
| Yan Agent today | 121 | 210 | 140 | 169 | 97 | 737 | 144 | 593 | 0.90 |
| P1 Native | 108 | 218 | 156 | 154 | 96 | 732 | 132 | 600 | 0.90 |
| P2 Thrift | 103 | 192 | 130 | 136 | 93 | 654 | 125 | 529 | 0.80 |
| P3 Fortress | 159 | 272 | 184 | 224 | 146 | 985 | 227 | 758 | 1.21 |
| P4 Proof | 187 | 253 | 193 | 218 | 112 | 963 | 370 | 593 | 1.18 |
| P5 Flow | 122 | 195 | 136 | 153 | 106 | 712 | 133 | 579 | 0.87 |
| Mix | 106 | 196 | 139 | 185 | 97 | 723 | 156 | 567 | 0.89 |

Hand-written Yan Agent is already 10% below Python on these tasks. Python pays for imports, dataclass
decorators, `async` and pytest scaffolding. The probe measured the opposite for model-written code (1.02x
to 1.29x), so the gap is in what models write and in repair rounds, not in the notation. This is why the
mix targets diagnostics, not spellings. The mix's T4 includes a `law` line (16 tokens). Without it the mix
totals 707.

**Single constructs** (o200k tokens, from `micro_tokens.txt`):

| Pair | Tokens | Verdict |
| --- | --- | --- |
| `-> T fail E` against `-> T ! E` | 10 / 10 | Sigils save nothing. Keep the word. |
| `list[str]) -> dict[str, float]` against `[str]) -> {str: float}` | 12 / 12 | Same. Keep Python's spelling. |
| `== Err(DupKey(key="a"))` against `== fail DupKey("a")` | 22 / 20 | D1. |
| `== Ok({...})` against `== {...}` | 17 / 16 | D1. |
| `ex` line against a three-line doctest | 16 / 20 | Doctest stays lenient only (D6). |
| `case Leaf():` with `return 0` on two lines, then on one | 9 / 8 | Not worth a canonical change. |
| Record as a class block, then as one line | 13 / 10 | Not worth the lost prior. |
| `Node(left=..., key=k, right=r)` against positional | 14 / 10 | Real cost. Measure first (open question 3). |
| `key=lambda p: (-p[1], p[0])` against `key=(-.n, .word)` | 13 / 8 | D8 for single accessors. |
| `Callable[[str], int, fail(E), uses(net)]` against `(str) -> int fail E uses net` | 15 / 12 | D5. |
| `partition` plus an empty-separator check, against `split_once(...) else fail` | 26 / 20 | D4. |
| `transfer(src=&a, ...)` against `transfer(src=a, ...)` | 13 / 11 | Keep `&` (law 4). |

**Toolchain check.** T1, T2 and T5 in today's Yan Agent pass `yan check` and their tests. T3 stops at
`http` (not in the toolchain yet). T4 gets E413 on `r` and `l` in branches that each end in `return`, so
they can never both run. The suggested fix inserts `.copy()` on subtrees, which makes the insert copy
whole subtrees, and the printer never drops a copy. This is a flow-sensitivity bug in the copy checker,
not a syntax question, but agents will learn the habit of copying.

### 2.4 What works and what doesn't

Works today: Python layout and keywords. `fail E` in the signature with `?` and `else` in the body. The
`x else fail E(...)` fallback, which is the most expressive piece of the surface. Inline sums with
`case Name():`. Contracts in the opening lines of the body. `with` handlers on `ex` lines. `par`
comprehensions. Lenient-in, canonical-out.

Doesn't work: places where a Python-looking spelling means something else (`char` against `str`), and
places where a rule forces text the model has no prior for (`Ok(...)` in examples, annotations on
empty literals, keywords on same-typed arguments, copies the checker asks for in error). Each costs a
repair round, and a repair round costs far more than any spelling saves.

## 3. Framework: six rules for judging a construct

1. **Two registers.** The *body register* (statements and expressions) is typed Python, because that is
   where most tokens are written and where the prior is strongest. The *header register* (signature,
   `uses`, contracts, examples, laws) is Yan's own, because it is short, read on every turn by
   `pack_context` and `yan sig`, and it is where the checker learns facts. Spend novelty only in the
   header.
2. **No silent false friends.** A spelling Python also has must mean what it means in Python. Where Yan
   must differ, the difference is loud: a diagnostic with a fix, never a silent change in behaviour.
   Better still, remove the difference where the core allows it (D2).
3. **Pave the desire paths.** Mine model errors. When models reach for an unambiguous spelling again and
   again, make it canonical (D1, D4) or accept it in the lenient reader (D6).
4. **Price the loop, not the line.** Tokens are paid on every turn: write, re-read, repair. BPE makes
   sigils free or nearly so, and a repair round costs hundreds of tokens. Remove rounds before
   characters.
5. **Every header token buys a fact.** A construct in the header earns its place only if the checker,
   the solver or the test generator learns something from it that it could not infer.
6. **One canonical spelling, many accepted ones.** The printer emits one form. The lenient reader's
   acceptance set only grows (spec 20.4). This is how D6 adds bridges at no cost to the canonical
   form.

## 4. The shared tasks

| Task | Exercises |
| --- | --- |
| T1 `top_words(text, k)` | strings, dicts, sorting with a key, a precondition, an example |
| T2 `parse_config(src)` | failure: an error sum, `fail`, fallbacks, success and failure examples |
| T3 `fetch_all(syms)` | capabilities (`uses net`), concurrency (`par`), a fake handler in an example |
| T4 `insert` and `depth` on a binary search tree | sums, exhaustive `match`, recursion and `decreases`, ownership |
| T5 `transfer(src, dst, amount)` | mutation of passed variables (`var`), `pre`, `post` with `old` |

Today's Yan Agent, for reference. T1, T2 and T5 pass the toolchain as written.

```
def top_words(text: str, k: int) -> list[tuple[str, int]]:
    pre k >= 0
    ex top_words("b a b", 1) == [("b", 2)]
    counts: dict[str, int] = {}
    for w in text.lower().split():
        counts[w] = (counts.get(w) else 0) + 1
    pairs = [(w, n) for w, n in counts.items()]
    pairs.sort(key=lambda p: (-p[1], p[0]))
    return pairs[:k]
```

```
type ConfigErr = BadLine(line: int, msg: str) | DupKey(key: str)

def parse_config(src: str) -> dict[str, int] fail ConfigErr:
    ex parse_config("a = 1\n# c\nb=2") == Ok({"a": 1, "b": 2})
    ex parse_config("a=1\na=2") == Err(DupKey(key="a"))
    out: dict[str, int] = {}
    n = 0
    for raw in src.splitlines():
        n += 1
        line = raw.strip()
        if line == "" or line.startswith("#"):
            continue
        key, sep, val = line.partition("=")
        if sep == "":
            fail BadLine(n, "missing =")
        key = key.strip()
        if key in out:
            fail DupKey(key.copy())
        out[key] = val.strip().parse_int() else fail BadLine(n, "bad int")
    return out
```

```
type FetchErr = Http(msg: str) | BadPrice(sym: str)

def fetch_px(sym: str) -> float fail FetchErr uses net:
    body = http.get_text(f"https://px.example/{sym}")?
    return body.parse_float() else fail BadPrice(sym.copy())

pub def fetch_all(syms: list[str]) -> dict[str, float] fail FetchErr uses net:
    ex fetch_all(["A"]) == Ok({"A": 10.0}) with net = fake_net({"/A": "10.0"})
    pxs = par [fetch_px(s)? for s in syms]
    return dict(zip(syms, pxs))
```

```
type Tree = Leaf() | Node(left: Tree, key: int, right: Tree)

def insert(t: Tree, x: int) -> Tree:
    decreases t
    match t:
        case Leaf():
            return Node(left=Leaf(), key=x, right=Leaf())
        case Node(l, k, r):
            if x < k:
                return Node(left=insert(l, x), key=k, right=r)
            if x > k:
                return Node(left=l, key=k, right=insert(r, x))
            return Node(left=l, key=k, right=r)

def depth(t: Tree) -> int:
    decreases t
    match t:
        case Leaf():
            return 0
        case Node(l, _, r):
            return 1 + max(depth(l), depth(r))
```

```
class Account:
    id: str
    balance: int

type TransferErr = Insufficient(short: int)

def transfer(var src: Account, var dst: Account, amount: int) -> fail TransferErr:
    pre amount > 0
    post src.balance + dst.balance == old(src).balance + old(dst).balance
    if src.balance < amount:
        fail Insufficient(amount - src.balance)
    src.balance -= amount
    dst.balance += amount
```

## 5. Five proposals

Each proposal pushes one axis as far as it will go while keeping law 1 (syntax only) and law 3
(one-token lookahead after factoring). Each shows the tasks where it differs. The rest are in the samples
folder.

### P1 Native: optimize the model's prior

*Thesis: write what a Python model already writes, and let the elaborator read it as Yan.*

- Examples are doctests: `>>>` lines in the docstring, the format of every HumanEval prompt.
- Leading `assert` lines on parameters are preconditions.
- `raise E(...)` is `fail E(...)` in a function that declares `fail` (there is no `try`, so nothing can
  catch it). `post` names the result `result`.
- Sums are sealed class hierarchies (`class BadLine(ConfigErr):`), constructor arguments are
  positional, and truthiness (`if not line:`) is canonical.

```
class ConfigErr:
    pass

class BadLine(ConfigErr):
    line: int
    msg: str

class DupKey(ConfigErr):
    key: str

def parse_config(src: str) -> dict[str, int] fail ConfigErr:
    r"""
    >>> parse_config("a = 1\n# c\nb=2")
    {"a": 1, "b": 2}
    >>> parse_config("a=1\na=2")
    DupKey(key="a")
    """
    out: dict[str, int] = {}
    for n, raw in enumerate(src.splitlines(), start=1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        key, sep, val = line.partition("=")
        if not sep:
            raise BadLine(n, "missing =")
        key = key.strip()
        if key in out:
            raise DupKey(key)
        out[key] = val.strip().parse_int() else raise BadLine(n, "bad int")
    return out
```

```
def transfer(var src: Account, var dst: Account, amount: int) -> fail Insufficient:
    assert amount > 0
    post src.balance + dst.balance == old(src).balance + old(dst).balance
    if src.balance < amount:
        raise Insufficient(amount - src.balance)
    src.balance -= amount
    dst.balance += amount
```

Findings: tokens equal today's (0.90). The doctest form needs `r"""` so that `\n` survives, which is a
trap of its own. A doctest's failure output is ambiguous with a success value. `assert` as `pre` collides
with the contract region: today's toolchain rejects the `post` after it (E106). Sealed hierarchies cost
lines. Truthiness hides the `0` and `""` cases. **Take:** the bridges, lenient only (D6). **Leave:** every
canonical change.

### P2 Thrift: optimize tokens per solved task

*Thesis: cut every token BPE lets you cut, keeping Python's keywords.*

- `-> T ! E`, `[T]`, `{K: V}` and `(A, B)` types, one-line records `class Account(id: str, balance: int)`.
- Anonymous examples: `ex ("a=1\na=2") == fail DupKey("a")` leaves out the function name.
- Single-statement suites on the same line: `if key in out: fail DupKey(key)`, `case Leaf(): return 0`.
- Positional arguments everywhere, implicit copies.

```
type ConfigErr = BadLine(line: int, msg: str) | DupKey(key: str)

def parse_config(src: str) -> {str: int} ! ConfigErr:
    ex ("a = 1\n# c\nb=2") == {"a": 1, "b": 2}
    ex ("a=1\na=2") == fail DupKey("a")
    out: {str: int} = {}
    for n, raw in enumerate(src.splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"): continue
        key, sep, val = line.partition("=")
        if not sep: fail BadLine(n, "missing =")
        key = key.strip()
        if key in out: fail DupKey(key)
        out[key] = val.strip().parse_int() else fail BadLine(n, "bad int")
    return out
```

```
type Tree = Leaf() | Node(left: Tree, key: int, right: Tree)

def insert(t: Tree, x: int) -> Tree:
    match t:
        case Leaf(): return Node(Leaf(), x, Leaf())
        case Node(l, k, r):
            if x < k: return Node(insert(l, x), k, r)
            if x > k: return Node(l, k, insert(r, x))
            return t
```

Findings: the cheapest variant (0.80), but the micro pairs show where the savings come from. The sigils
save zero. One-line suites save one token each. Nearly all of the 11% comes from positional arguments,
dropped copies, dropped `Ok` and dropped `decreases`, which are rules, not notation. **Take:** the
anonymous-example idea goes to open question 4. Positional same-typed arguments become a measurement
(open question 3). **Leave:** the sigils and one-line forms, which buy nothing and cost the prior.

### P3 Fortress: optimize safety and explicitness

*Thesis: everything that can go wrong is visible in the text an auditor reads.*

- Attenuated capabilities in the signature: `uses net("px.example")`.
- Untrusted input as a type: `Untrusted[str]`, narrowed only by validators.
- `uses none` and keyword arguments on every multi-argument call, `cost` bounds on public functions,
  deadlines as fallible blocks (`with deadline(2 * s) else fail TimedOut():`), and `par(limit=8)` on
  comprehensions.

```
type FetchErr = Http(msg: str) | BadPrice(sym: str) | TimedOut()

def fetch_px(sym: str) -> float fail FetchErr uses net("px.example"):
    body: Untrusted[str] = http.get_text(t"https://px.example/{sym}")?
    return body.parse_float() else fail BadPrice(sym=sym.copy())

pub def fetch_all(syms: list[str]) -> dict[str, float] fail FetchErr uses net("px.example"), time:
    ex fetch_all(syms=["A"]) == Ok({"A": 10.0}) with net = fake_net({"/A": "10.0"})
    with deadline(2 * s) else fail TimedOut():
        pxs = par(limit=8) [fetch_px(sym=s.copy())? for s in syms]
    return dict(zip(syms, pxs))
```

```
pub def transfer(var src: Account, var dst: Account, amount: int) -> fail TransferErr uses audit:
    pre amount > 0
    post src.balance + dst.balance == old(src).balance + old(dst).balance
    post src.balance >= 0
    if src.id == dst.id:
        fail SameAccount()
    if src.balance < amount:
        fail Insufficient(short=amount - src.balance)
    src.balance -= amount
    dst.balance += amount
    audit.log(t"transfer {amount} from {src.id} to {dst.id}")
```

Findings: 1.21x Python and the most repair-prone design. Every one of its rules is a new way to fail a
check. But two of its ideas are worth more than their tokens. Attenuation in the signature makes
`yan review` show a narrowing of authority. `Untrusted[T]` is the open untrusted-text item in the project
notes. Both change core semantics, not syntax, so they go to open questions 1 and 2. `par(limit=n)` on a
comprehension is a plain grammar gap (D9). **Leave:** the mandatory keywords, `uses none` everywhere,
and audit-by-default.

### P4 Proof: optimize what the checker can verify

*Thesis: the header is a specification, so give it the power of one.*

- Example tables: `ex f:` followed by indented argument tuples and results, sharing one handler binding.
- `law` lines: properties over the parameters, with optional `forall` binders, checked by property
  tests and handed to the solver.
- Failure postconditions: `post failed implies src == old(src)`.
- Richer `post`: membership, sortedness, set equalities.

```
def parse_config(src: str) -> dict[str, int] fail ConfigErr:
    post forall k in out: k != "" and "=" not in k
    ex parse_config:
        ("a = 1\n# c\nb=2") == {"a": 1, "b": 2}
        ("a=1\na=2") == fail DupKey("a")
        ("a") == fail BadLine(1, "missing =")
    law parse_config(src + "\n# note") == parse_config(src)
    ...
```

```
def insert(t: Tree, x: int) -> Tree:
    pre is_bst(t)
    post is_bst(out) and keys(out) == keys(t) | {x}
    decreases t
    law insert(insert(t, x), x) == insert(t, x)
    ...
```

Findings: spec tokens are 2.6x today's (370 against 144), and code tokens are unchanged. The spec phase is
priced separately against pytest (spec 21.6), so this is the right place for the cost if it buys
verification. Dafny's 82% vericoding rate says models handle inline contracts well. A `law` is one line,
cannot be satisfied by special-casing an input, and tests a whole family of inputs, which is what an
ImpossibleBench-style cheat cannot fake. Example tables save little and add an indentation form.
**Take:** `law` (D7), and failure examples written as `== fail E(...)` (D1). **Leave:** tables, and
failure postconditions for now (open question 5).

### P5 Flow: optimize expressive power

*Thesis: say more with each line, Python 3.12's way where it has one and Yan's way where it doesn't.*

- Destructuring fallback: `key, val = line.split_once("=") else fail BadLine(n, "missing =")`.
- Accessor lambdas from plain Yan: `.sym`, `key=(-.n, .word)`.
- Walrus with narrowing: `if (short := amount - src.balance) > 0:`.
- Expression-bodied functions and `match` as an expression with arm values.

```
def parse_config(src: str) -> dict[str, int] fail ConfigErr:
    ex parse_config("a = 1\n# c\nb=2") == {"a": 1, "b": 2}
    ex parse_config("a=1\na=2") == fail DupKey("a")
    out: dict[str, int] = {}
    for n, line in src.splitlines().map(.strip()).enumerate(start=1):
        if line == "" or line.startswith("#"):
            continue
        key, val = line.split_once("=") else fail BadLine(n, "missing =")
        key = key.strip()
        if key in out:
            fail DupKey(key.copy())
        out[key] = val.strip().parse_int() else fail BadLine(n, "bad int")
    return out
```

```
def insert(t: Tree, x: int) -> Tree = match t:
    case Leaf(): Node(left=Leaf(), key=x, right=Leaf())
    case Node(l, k, r) if x < k: Node(left=insert(l, x), key=k, right=r)
    case Node(l, k, r) if x > k: Node(left=l, key=k, right=insert(r, x))
    case Node(_, _, _): t
```

Findings: 0.87x, and the most pleasant to read of the five. Its gains split cleanly. The destructuring
fallback and accessors extend forms Yan Agent already has. Expression-bodied `match` brings back the
clause form Yan Agent dropped on purpose, has no Python prior, and makes `return` optional in one
place only. **Take:** D4 and D8. Walrus goes to open question 6. **Leave:** expression bodies and
`match` expressions, which belong to plain Yan.

### Scoreboard

Scores are judgments on a 1 to 5 scale. Tokens are measured.

| Axis | Today | P1 Native | P2 Thrift | P3 Fortress | P4 Proof | P5 Flow | Mix |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Model prior (first-try parse) | 4 | 5 | 2 | 3 | 3 | 3 | 5 |
| Tokens (vs Python, measured) | 0.90 | 0.90 | 0.80 | 1.21 | 1.18 | 0.87 | 0.89 |
| Expected repair rounds | 3 | 4 | 3 | 1 | 3 | 3 | 4 |
| Safety and explicitness | 4 | 3 | 2 | 5 | 4 | 4 | 4 |
| Verification power | 3 | 2 | 3 | 3 | 5 | 3 | 4 |
| Expressiveness | 3 | 3 | 3 | 3 | 3 | 5 | 4 |
| Tooling (LL(1), masks, round trip) | 5 | 3 | 4 | 4 | 4 | 3 | 5 |

"Expected repair rounds" is scored so that higher means fewer rounds.

## 6. The mix

The mix is today's Yan Agent plus nine changes. Each change either removes a repair round that
models demonstrably hit or adds a header fact the checker can use. None of them adds a sigil.

| Item | Change | From | Evidence | Kind |
| --- | --- | --- | --- | --- |
| D1 | `ex f(x) == v` compares the success value of a fallible `f`, and `ex f(x) == fail E(...)` states a failure. Lenient: `fails E(...)` and `== Ok(v)` or `== Err(e)` normalize to these. | P2, P4, probe | 18 runs hit E315 in `ex` lines (the top diagnostic), and 18 parse errors were models inventing the failure form. It also removes the toolchain gap "`Ok` without an expected type". | canonical form, `ex` semantics |
| D2 | A one-character string literal types as `char` where a `char` is expected, as an exact integer literal types as a float. | probe | 10 runs, 45 hits of `char` against `str`. It removes a false friend. | elaboration |
| D3 | An empty `[]`, `{}` or `set()` bound to a local takes its element type from the first use in the same function, as mypy's partial types do. | probe | 12 runs hit E241 on an empty list. | elaboration |
| D4 | Destructuring fallback: `a, b = e else fail E(...)` (also `else return` and the other fallback arms) when `e` is an optional tuple. | P5 | 6 tokens fewer than `partition` plus a check, using the existing `else`. It assumes a std `split_once -> tuple[str, str] \| None`, which spec 17 does not list yet. | grammar (a tuple target with a fallback) |
| D5 | Arrow function types: `(str, int) -> bool fail E uses net`, with the `par` prefix as today. `Callable[...]` stays lenient. | survey | Better `Callable` syntax is the top typed-Python request (45.7%). It saves 3 tokens and reads like a `def` header. | canonical form |
| D6 | Lenient bridges, with no canonical change: doctest `>>>` lines become `ex`; `raise E(...)` becomes `fail E(...)` in a function that declares `fail` and has no `try`; `result` in `post` becomes `out` when no binding is named `result`; leading `assert` lines on parameters become `pre`. | P1 | HumanEval and MBPP formats. The probe's `out` and `result` mismatch. | lenient reader |
| D7 | `law` contract lines: a boolean expression over the parameters and optional `forall x: T` binders, true for all inputs. `yan test` checks it by property testing, the solver may prove it, and it is part of the header. | P4 | Hard to game with special cases. Dafny-style inline specs work for models. | new contract word (soft, contract region only) |
| D8 | Accessor lambdas `.field` in argument position, as in plain Yan, single accessor only: `quotes.index_by(.sym)`, `sorted(xs, key=.n)`. | P5, plain Yan | 5 tokens fewer than a lambda, and the same spelling in both surfaces. | grammar |
| D9 | `par(limit=n)` on comprehensions, matching `par(limit=n) for`. | P3 | A grammar gap. | grammar |

Left out on purpose: sigils (`!`, `T?`, `[T]`, `{K: V}`), one-line records and suites, expression bodies,
dropping `&`, implicit copies, doctests as the canonical form, sealed hierarchies as the canonical form,
mandatory keywords, and `uses none` everywhere. Each costs more prior or more repair rounds than it
saves.

Not syntax, but found here: **fix the E413 false positive** across exclusive `return` branches before the
R567 runs, or the runs will measure a checker bug as a language cost.

The mix on all five tasks. T1 passes today's toolchain as written. The others use D1, D4 and D7.

```
def top_words(text: str, k: int) -> list[tuple[str, int]]:
    pre k >= 0
    ex top_words("b a b", 1) == [("b", 2)]
    counts: dict[str, int] = {}
    for w in text.lower().split():
        counts[w] = (counts.get(w) else 0) + 1
    return sorted(counts.items(), key=lambda p: (-p[1], p[0]))[:k]
```

```
type ConfigErr = BadLine(line: int, msg: str) | DupKey(key: str)

def parse_config(src: str) -> dict[str, int] fail ConfigErr:
    ex parse_config("a = 1\n# c\nb=2") == {"a": 1, "b": 2}
    ex parse_config("a=1\na=2") == fail DupKey("a")
    out: dict[str, int] = {}
    for n, raw in enumerate(src.splitlines(), start=1):
        line = raw.strip()
        if line == "" or line.startswith("#"):
            continue
        key, val = line.split_once("=") else fail BadLine(n, "missing =")
        key = key.strip()
        if key in out:
            fail DupKey(key.copy())
        out[key] = val.strip().parse_int() else fail BadLine(n, "bad int")
    return out
```

```
type FetchErr = Http(msg: str) | BadPrice(sym: str)

def fetch_px(sym: str) -> float fail FetchErr uses net:
    body = http.get_text(f"https://px.example/{sym}")?
    return body.parse_float() else fail BadPrice(sym.copy())

pub def fetch_all(syms: list[str]) -> dict[str, float] fail FetchErr uses net:
    ex fetch_all(["A"]) == {"A": 10.0} with net = fake_net({"/A": "10.0"})
    pxs = par [fetch_px(s)? for s in syms]
    return dict(zip(syms, pxs))
```

```
type Tree = Leaf() | Node(left: Tree, key: int, right: Tree)

def insert(t: Tree, x: int) -> Tree:
    decreases t
    law insert(insert(t, x), x) == insert(t, x)
    match t:
        case Leaf():
            return Node(left=Leaf(), key=x, right=Leaf())
        case Node(l, k, r):
            if x < k:
                return Node(left=insert(l, x), key=k, right=r)
            if x > k:
                return Node(left=l, key=k, right=insert(r, x))
            return Node(left=l, key=k, right=r)

def depth(t: Tree) -> int:
    decreases t
    match t:
        case Leaf():
            return 0
        case Node(l, _, r):
            return 1 + max(depth(l), depth(r))
```

```
class Account:
    id: str
    balance: int

type TransferErr = Insufficient(short: int)

def transfer(var src: Account, var dst: Account, amount: int) -> fail TransferErr:
    pre amount > 0
    post src.balance + dst.balance == old(src).balance + old(dst).balance
    if src.balance < amount:
        fail Insufficient(amount - src.balance)
    src.balance -= amount
    dst.balance += amount
```

How to test the mix cheaply: D1, D2, D3 and D6 need no model change and can be scored by replaying the
100 probe runs through a patched reader and counting the rounds that disappear. D4, D5, D7 and D8 need
an A/B arm in the R567 harness, with the primer kept within its 500-token budget.

## 7. Open questions

1. **Attenuated capabilities** (`uses net("px.example")`). Can the core express a capability narrowed by
   a literal, so the narrowing is part of the signature and `yan review` sees it? Or does it stay a
   handler (`with net = net.only(...)`), visible only in the caller?
2. **Untrusted text** (`Untrusted[T]`). This is the open project item. A type in the header is the
   strongest place for it, but it needs a core rule for which operations taint and which validate.
3. **Same-typed arguments.** E210 costs 4 tokens per three-field constructor call and fired in 11 runs.
   How many real swaps does it catch? Count it in the R567 runs before relaxing it, for example for
   recursive fields of one variant.
4. **Anonymous examples.** Should `ex ("a", 1) == v` leave out the function name? It saves 2 or 3 tokens
   per line, but the line stops being a valid expression on its own.
5. **Failure postconditions.** "On failure, `var` parameters are unchanged" is the property callers
   want most from `fail`. Should it be a default guarantee of the core, a contract line, or nothing?
6. **Walrus.** `if (x := e) ...` is valid Python and narrows nicely, but `else` covers most of its uses.
   Add it only if the runs show models reaching for it.
7. **Formatting on the wire.** Stripping formatting saves about 24% of input tokens. Should
   `pack_context` send dependency headers without docstrings and blank lines, and keep full text only for
   the item being edited?
8. **Accessor scope.** Should D8 extend to composite keys (`key=(-.n, .word)`, where every `.` refers to
   the one implicit parameter)? It is concise, but the scope of the implicit lambda is easy to misread.

## 8. Sources

- Giagnorio, Martin-Lopez, Bavota. No Resource, No Benchmarks, No Problem? Evaluating and Improving LLMs
  for Code Generation in No-Resource Languages. https://arxiv.org/pdf/2606.16827
- Cheng et al. Can LLM Coding Assistants Support Emerging Programming Languages? An Empirical Study on
  Cangjie. https://arxiv.org/pdf/2603.14501
- Mündler et al. Type-Constrained Code Generation with Language Models (PLDI 2025).
  https://arxiv.org/abs/2504.09246
- Sun et al. AI Coders Are Among Us: Rethinking Programming Language Grammar Towards Efficient Code
  Generation (ISSTA 2024). https://arxiv.org/abs/2404.16333
- The Hidden Cost of Readability: How Code Formatting Silently Consumes Your LLM Budget (ICSE
  2026). https://arxiv.org/abs/2508.13666
- Zhong et al. ImpossibleBench: Measuring LLMs' Propensity of Exploiting Test Cases.
  https://arxiv.org/abs/2510.20270
- A benchmark for vericoding: formally verified program synthesis.
  https://arxiv.org/abs/2509.22908
- Blinn et al. Statically Contextualizing Large Language Models with Typed Holes (OOPSLA 2024).
  https://arxiv.org/abs/2409.00921
- Pel, A Programming Language for Orchestrating AI Agents. https://arxiv.org/abs/2505.13453
- MoonBit. The future of programming languages in the era of LLM. https://www.moonbitlang.com/blog/ai-coding
- Meta Engineering. Python typing survey 2026.
  https://engineering.fb.com/2026/10/27/developer-tools/python-typing-survey-2026-ai-tools-checker-landscape/
- Stack Overflow Developer Survey 2025, technology. https://survey.stackoverflow.co/2025/technology/
- Token efficiency across 19 languages (RosettaCode analysis, low confidence).
  https://feedbagel.com/post/programming-language-token-efficiency-analysis-how-ai-coding-agents-change-langu
- Project data: probe rerun runs (`/mnt/project-files/jian-probe/runs-v2`), this study's samples and
  scripts (`/mnt/project-files/syntax-study-yag/`).
