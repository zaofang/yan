# Yan Prompt 言询 (`.ypm`): a syntax and semantics study

Date: 2026-11-04. Status: brainstorm study, non-normative. Nothing here is a spec row, a diagnostic
code or a decision. Every code block is a mock. `spec/` wins wherever this page differs from it.

**One-line result.** A Yan prompt should be a typed function whose header is the contract, whose body
is prose in labelled lanes (`system`, `user`, `data`, `assistant`), whose typed holes put generation
order in the author's hands, and whose strategy clauses (`check`, `repair`, `vote`, `fallback`,
`learn`) lower to plain Yan combinators. The checker, not the author's discipline, keeps untrusted text
out of instruction lanes and refuses the "lethal trifecta" unless it is audited.

Contents: 1 methodology, 2 starting point, 3 findings, 4 the five shared tasks, 5 the five proposals,
6 scorecard and experiment, 7 the mix, 8 open questions, 9 sources, 10 appendix: baseline.

---

## 1. Methodology and process

The brief: research peers and literature, read the current syntax narrowly, write five proposals that
each optimize one axis, implement the same tasks in all five, then mix the best parts into one
proposal. The steps actually taken:

1. **Read the repo narrowly.** `docs/surfaces.md` section 7 (current RFC sketch and mock grammar),
   RFC 0002 D3 (job, subset, lowering, recovery), spec 16.7 rows R538 and R568 (niche surface rules),
   spec 17.4 (`Wire`, `Untrusted[T]` R451, model calls R450, `generate_chat` R551), spec 7 R305
   (recording and replay), spec 15 lexis and grammar heads (records, sums, lambdas, `par`, `ex`).
   Nothing else in `spec/` was read.
2. **Surveyed peers.** Prompt DSLs and frameworks: BAML, DSPy, Prompty, dotprompt (Genkit), POML,
   PDL, LMQL, Guidance, Outlines, SGLang's frontend, APPL, Instructor, TypeChat, Pydantic AI,
   Mirascope, ell, Promptfoo. For each: what the unit of abstraction is, where the schema lives, how
   validation and retry work, how tests and versions work.
3. **Read research.** Format sensitivity (Sclar et al. 2024; POML 2025), format restriction versus
   reasoning (Tam et al. 2024), constrained decoding coverage (JSONSchemaBench 2025), prompt injection
   defenses by design (CaMeL 2025; Beurer-Kellner et al. 2025; spotlighting; instruction hierarchy),
   prompt optimization (DSPy, MIPROv2, GEPA 2025), developer practice (Liang et al. FSE 2025), example
   order and position effects (Lu et al. 2022; Liu et al. 2024). Confidence is marked per finding.
4. **Fixed five shared tasks** (section 4) that stress the axes that differ: plain classification,
   extraction with reasoning, grounded answering over untrusted documents, robustness (vote, repair,
   fallback), and tool use.
5. **Wrote five proposals**, each optimizing one axis and allowed to be bad on the others: brevity,
   safety and explicitness, expressive power, agent token cost and tooling, readability. Each proposal
   writes all five tasks.
6. **Experimented.** Measured the size of each proposal's five tasks (characters and a rough token
   estimate, section 6), and checked each form against the project's constraints: law 1 and R568
   (lowering only, or own semantics), law 4 (the five facts stay marked), recovery (spelling must
   round-trip through the HIR), and the existing std surface (`generate_chat`, `validate`,
   `model.only`, `fake_model`, `replay`).
7. **Mixed.** Took each proposal's winning idea where it does not cost another axis much, resolved
   conflicts by the target taste (concise, opinionated, not obnoxious), and rewrote all five tasks in
   the mix.

What was not done: no model was run on any proposal (Plan A probe credits are reserved for R567), no
parser was built, and token counts are estimates, not tokenizer output.

## 2. Starting point

Current sketch (RFC 0002 D3, mocked further in `docs/surfaces.md`):

```
prompt triage(t: Ticket) -> Triage fail ModelError uses model =
  model "fast"
  system "You route support tickets. Reply only in the schema."
  user t"""
    Subject: {t.subject}
    Body: {t.body}
    """
  check r -> r.summary != ""
  ex triage(fixtures.outage) == Ok(fixtures.outage_triage)
     with model = fake_model(fixtures.outage_reply)
```

It lowers to `model.only(models=["fast"])`, then `generate_chat[Triage]` over the role sections, then
`.validate(check)`; without `check` the result stays `Untrusted[Triage]`. It is a good baseline: typed,
testable, capability-marked. Its gaps, found while writing the shared tasks:

- Prose is a string literal in every role, so prompts read as code with quoted text, and multi-line
  prose needs `t"""..."""` and dedent rules.
- No place for untrusted data distinct from instructions. Every interpolation into `user` mixes the two.
- No control over generation order (reasoning before answer), and no way to say "think, then answer in
  the schema".
- No retry with the validation error fed back, no fallback model, no sampling and voting.
- No tool section in the RFC (the mock adds `tools`), and no rule about which tools a prompt that reads
  untrusted data may hold.
- Few-shot examples are an opaque `shots expr`; no way to tune them and pin the result.
- No cache-aware layout, although provider prompt caching is the largest cost lever in production.

## 3. Findings

Confidence: **high** (several independent sources or a strong benchmark), **medium** (one good source
or strong practitioner consensus), **low** (inference or anecdote).

### 3.1 The market

| Tool | Unit | Schema lives in | Validation and repair | Tests and versions | Lesson for Yan |
| --- | --- | --- | --- | --- | --- |
| BAML | `function` with typed args and return, Jinja prompt string, `client` | its own type language; `ctx.output_format` renders it into the prompt | schema-aligned parsing: lenient parse of near-JSON, no constrained decoding needed | `test` blocks, playground | typed functions win; render the schema for the model, parse leniently |
| DSPy | signature `question -> answer` plus module | field names and docstrings | assertions, later `Refine` and `BestOfN` | optimizers (MIPROv2, GEPA) compile prompts from data | the signature alone can be the prompt; prompts can be learned |
| Prompty, dotprompt | file: YAML front matter plus template | front matter (`output.schema`) | none built in | file is the version | a prompt is a file people diff and review |
| POML | HTML-like components (`role`, `task`, `example`, data components) | output format through a stylesheet | none | VS Code tooling | separate content from presentation; presentation moves accuracy a lot |
| PDL (IBM) | YAML blocks (model, code, read, if, repeat) | JSON Schema or type spec per block | `parser` and `spec` per block | trace viewer | declarative data, every block typed; but YAML hurts at scale |
| LMQL, Guidance, SGLang | template with inline generation holes and constraints | per hole (regex, choice, length, grammar) | constraint at decode time | none | holes in prose give exact control of order and shape |
| Instructor, TypeChat, Pydantic AI | host-language types | Pydantic or TypeScript types | retry with the validation error fed back | host tests | repair rounds are the common case, not the exception |
| ell, Mirascope | decorated host function | return type | host code | ell versions every prompt by hashing its closure | content hashing for prompt versions, which Yan already has (R348) |
| Promptfoo | YAML test matrix | assertions | n/a | eval matrix across models | evals are a separate, budgeted activity from unit tests |

Market reading (medium): the field converged on "prompt as typed function" (BAML, DSPy, Instructor,
Pydantic AI, ell). The open fights are (a) where prose lives (strings in code versus a document with
holes), (b) whether output is constrained at decode time or parsed leniently, and (c) whether prompts
are written or compiled from data. No tool treats prompt injection as a type error. That is Yan's gap
to own, because Yan already has `Untrusted[T]` and capabilities.

### 3.2 What users want (medium; practitioner reports, issue trackers, Liang et al.)

1. Types in and out, with the schema shown to the model without hand-writing it.
2. Prompts readable as prose and reviewable in diffs; templating that does not fight the prose.
3. Tests that run offline and fast, plus separate evals against real models with budgets.
4. Retries, fallbacks and model swaps without rewriting the prompt.
5. Exact view of what is sent ("show me the bytes"), per provider.
6. Versioning tied to behavior, so a prompt change is visible as a change.
7. Cost control: token caps, caching, cheaper model routing.

### 3.3 What works and what does not

- **Formatting is not cosmetic (high).** Sclar et al. (ICLR 2024) found accuracy spreads of up to 76
  points on LLaMA-2-13B from semantically equivalent prompt formats. POML (2025) reports TableQA
  accuracy from 6% to 61.8% on GPT-3.5 Turbo by stylesheet alone, and the best style differed per
  model. Consequence: presentation (how data and schema are rendered) must be a separate, swappable,
  measurable layer, not baked into every prompt's text.
- **Strict formats can hurt reasoning (high for older models, medium for current).** Tam et al. (2024,
  "Let Me Speak Freely?") measured GSM8K drops such as Claude 3 Haiku from about 87% to 23% under a
  JSON schema, partly because answers came before reasoning. Their NL-to-format two-step recovered
  most of it. Consequence: the author needs control of generation order and a "think, then format"
  mode. Field order in the schema is semantics, not style.
- **Constrained decoding is uneven across schemas (high).** JSONSchemaBench (2025; 10K real schemas,
  six engines including Guidance, Outlines, llama.cpp, XGrammar, OpenAI, Gemini) shows coverage gaps
  that differ per engine. Consequence: the language must not promise constraints the handler cannot
  enforce; it should say which constraints are enforced at decode time and which by validation after.
- **Lenient parsing plus validation is practical (medium).** BAML's schema-aligned parsing and
  Instructor's retry-on-error both work without constrained decoding. Yan's `Wire` decoder is strict
  by design (17.4), so leniency must be a handler mode, never a type hole.
- **Repair rounds are routine (medium).** Instructor's main loop feeds the validation error back.
  Consequence: `repair n` belongs in the surface, recorded in the trace like any model call.
- **Example order and position matter (high).** Lu et al. (2022) showed few-shot order alone swings
  results; Liu et al. (TACL 2024) showed content in the middle of long contexts is used less.
  Consequence: shots are data with a declared order, and the renderer, not the author, decides layout
  consistently.
- **Prompts can be compiled (high).** DSPy optimizers and GEPA (2025, reflective evolution, reported to
  beat MIPROv2 and GRPO with far fewer rollouts) learn instructions and shots from a metric.
  Consequence: a `learn` slot whose value is produced offline and pinned, so builds stay deterministic.
- **Injection is solved by structure, not by prompting (high).** CaMeL (2025) separates control flow
  (from the trusted query) from data flow and enforces capability policies at tool calls: 77% of
  AgentDojo tasks with provable security versus 84% undefended. Beurer-Kellner et al. (2025) give six
  patterns (action-selector, plan-then-execute, LLM map-reduce, dual LLM, code-then-execute, context
  minimization) that all limit what untrusted text can reach. Spotlighting (delimiting or datamarking
  untrusted text) and instruction hierarchy training reduce but do not remove risk. Consequence: Yan's
  capabilities and `Untrusted[T]` can make these patterns checked properties of a prompt item.
- **Developers treat prompts as less rigorous code (medium).** Liang et al. (FSE 2025) found prompt
  edits are made quickly, tested informally, and drift. Consequence: tests and evals beside the prompt,
  in the same item, and versions by content hash.
- **Provider prompt caching rewards stable prefixes (high, provider docs).** Anthropic, OpenAI and
  Google bill cached prefix tokens at a fraction. Consequence: layout should put stable text first,
  and the compiler can check it.

### 3.4 Design constraints from Yan itself

- **Law 1 and R568.** RFC 0002 lists Yan Prompt as lowering to `generate_chat` and `validate`, with no
  engine. Every proposal below lowers to plain Yan plus std; where one needs a std addition, it says so.
- **Law 4.** `fail` and `uses` stay in the header in every proposal, including the brevity one.
- **Determinism (R305).** Model calls are recorded and replayable. Sampling, voting and learned slots
  must be deterministic under replay: seeds and tuned values are inputs, not ambient.
- **`Wire` is static (17.4).** A model never selects a type. Runtime-dependent choices (for example
  "cite only ids that exist") are constraints over a static type, not dynamic types.
- **`Untrusted[T]` (R451, R452).** Templates keep the label of interpolated untrusted values. The
  surface can use this to type lanes.

## 4. The five shared tasks

Shared types, plain Yan, used by every proposal:

```
type Sev = P1() | P2() | P3()
type Team = Billing() | Infra() | Account()
type Ticket = { subject: str, body: str }
type Triage = { severity: Sev, team: Team, summary: str where len(summary) <= 200 }
type Invoice = { vendor: str, total: Money, due: PlainDate? }
type Doc = { id: DocId, text: str }
type Answer = { text: str, cites: List[DocId] }
type Reply = { text: str, order: OrderId? }
```

- **T1 Triage.** Classify a ticket into `Triage`.
- **T2 Extract.** Extract an `Invoice` from document text with few-shot examples, reasoning first.
- **T3 Grounded answer.** Answer a question from retrieved documents (untrusted); every citation must
  be an id that was supplied.
- **T4 Robust triage.** T1 with five samples and a majority vote, two repair rounds on validation
  failure, and a fallback model.
- **T5 Tool use.** Answer a customer using a read-only `orders.lookup` tool, at most three calls.

## 5. The five proposals

### Proposal A: Signature (axis: brevity)

**Idea.** DSPy's insight taken to its end: the header is the prompt. The doc comment is the
instruction, parameter and field doc comments are descriptions, the result type is the schema, and a
deterministic renderer builds the messages. A body is optional and holds only clauses.

```
--- Route a support ticket to the team that owns it.
prompt triage(t: Ticket) -> Triage fail ModelError uses model
```

```
--- Extract invoice fields. Use none when a field is absent.
prompt extract(doc: str) -> think Invoice fail ModelError uses model
  shots fixtures.invoice_pairs
```

`think T` asks for free reasoning first, then `T` (NL-to-format); the reasoning goes to the trace, the
result is `T`.

```
--- Answer from the documents only. Cite every claim by document id.
prompt answer(q: str, docs: List[Doc]) -> Answer fail ModelError uses model
  check a -> a.cites.all(c -> c in docs.map(.id))
```

```
let r = vote(5, () -> triage(t) repair 2 else triage_large(t))?
```

T4 is plain Yan over the T1 function: `repair n` is a postfix on a prompt call, `vote` a std
combinator. `triage_large` is T1 redeclared with `model "large"`.

```
--- Answer the customer. Look orders up; never guess an order.
prompt support(q: str) -> Reply fail ModelError uses model, orders.read
  tools orders.lookup max 3
```

**Wins.** Smallest form by far (section 6). Schema and instruction cannot drift: both come from the
types. Agents write it well because it looks like an ordinary signature.

**Loses.** The rendered prompt is invisible in the source, so the format sensitivity finding (3.3)
bites: the renderer's choices move accuracy and the author cannot see them. Prose longer than a
paragraph is awkward in doc comments. No place for data versus instructions.

### Proposal B: Ledger (axis: safety and explicitness)

**Idea.** Every trust boundary is spelled. Lanes are typed: `system` takes only stage-0 text,
`task` takes trusted templates, `data` is the only lane that accepts `Untrusted` values and is always
spotlighted by the handler. Decoding mode, budget and result trust are stated. The checker computes the
lethal trifecta (untrusted input, private data, an outbound capability) per item and rejects it unless
the item carries an audited waiver.

```
prompt triage(t: Untrusted[Ticket]) -> Triage fail ModelError
    uses model.only(models=["fast"], max_tokens=300) =
  system "You route support tickets to a team."
  task   "Classify the ticket in the data block."
  data   t
  decode strict
  check  r -> r.summary != ""
```

```
prompt extract(doc: Untrusted[str]) -> Invoice fail ModelError
    uses model.only(models=["small"], max_tokens=800) =
  system "Extract invoice fields. Use none when a field is absent."
  shots  fixtures.invoice_pairs
  data   doc
  decode free then strict
  check  i -> i.total >= 0usd
```

`decode free then strict` is NL-to-format: one free generation, then one constrained conversion, two
recorded calls.

```
prompt answer(q: str, docs: List[Untrusted[Doc]]) -> Answer fail ModelError
    uses model.only(models=["fast"], max_tokens=600) =
  system "Answer from the documents only. Cite each claim by id."
  task   t"Question: {q}"
  data   docs
  decode strict with cites in docs.map(.id)
  check  a -> a.cites.all(c -> c in docs.map(.id))
```

`decode ... with cites in xs` constrains a field to runtime values at decode time where the handler
can, and `check` enforces it everywhere.

```
prompt triage_robust(t: Untrusted[Ticket]) -> Triage fail ModelError
    uses model.only(models=["fast", "large"], max_tokens=1500) =
  system "You route support tickets to a team."
  task   "Classify the ticket in the data block."
  data   t
  decode strict
  sample 5 seed 7 then vote
  repair 2
  fallback "large"
  check  r -> r.summary != ""
```

```
prompt support(q: Untrusted[str]) -> Reply fail ModelError
    uses model.only(models=["fast"], max_tokens=800), orders.read =
  system "Answer the customer. Look orders up; never guess an order."
  data   q
  tools  orders.lookup max 3
  decode strict
```

T5 passes the trifecta check: the only tool is read-only (`orders.read`), so no outbound channel. Adding
`mail.send` to `uses` would be an error until the item says `@accept_risk("...")`, which `yan audit`
lists.

**Wins.** Turns CaMeL and the dual-LLM pattern into checked rules. Reviewers see trust, budget and
decoding at a glance. Every model behavior is recorded and auditable.

**Loses.** Verbose: about twice A. `Untrusted[...]` in every parameter is noise when the caller already
holds untrusted values (the type carries it anyway). Many keywords to learn.

### Proposal C: Weave (axis: expressive power)

**Idea.** The LMQL, Guidance and SGLang line: prose with typed holes. A hole `{?name: T}` is a
generation point; holes fill in source order, so the author controls reasoning order exactly; the
result record is assembled from holes by name. Prompts are values, so they compose: `par` samples,
`fork` branches share a prefix, and `learn` marks text that an optimizer will fill offline.

```
prompt triage(t: Ticket) -> Triage fail ModelError uses model =
  system "You route support tickets."
  user t"Subject: {t.subject}\nBody: {t.body}"
  assistant t"""
    Severity: {?severity: Sev}
    Team: {?team: Team}
    Summary: {?summary: str stop "\n"}
    """
```

```
prompt extract(doc: str) -> Invoice fail ModelError uses model =
  system learn "Extract invoice fields." by fixtures.invoice_train metric exact
  shots learn 4 from fixtures.invoice_pairs
  user t"{doc}"
  assistant t"""
    Reasoning: {?_why: str stop "\n\n"}
    Vendor: {?vendor: str}
    Total: {?total: Money}
    Due: {?due: PlainDate?}
    """
```

A hole whose name starts with `_` is scratch: generated, recorded, not part of the result. `learn`
slots are filled by `yan tune` into a pinned file; the build reads the pinned value.

```
prompt answer(q: str, docs: List[Doc]) -> Answer fail ModelError uses model =
  system "Answer from the documents only."
  user t"{docs.render()}\n\nQuestion: {q}"
  assistant t"""
    {?text: str}
    Sources: {?cites: List[one of docs.map(.id)]}
    """
```

```
pub triage_robust(t: Ticket) -> Triage fail ModelError uses model =
  let rs = par for i in 0..5 -> triage(t) seed i repair 2
  vote(rs) else triage(t) on "large"
```

```
prompt support(q: str) -> Reply fail ModelError uses model, orders.read =
  system "Answer the customer. Look orders up; never guess an order."
  user t"{q}"
  loop max 3
    assistant t"{?call: Call[orders.lookup]?}"
    if call is None then break
    tool call
  assistant t"{?text: str}{?order: OrderId?}"
```

**Wins.** The most control: generation order, scratch reasoning, choice from runtime values, streaming
points, agent loops, learned text. Covers every task without new combinators.

**Loses.** Hole-by-hole generation needs providers that support prefill or interleaving; most hosted
APIs offer one structured call, not interleaved holes. The `loop` in T5 reinvents the agent loop inside
a prompt. Hardest to read and to round-trip. Holes plus loops are where LMQL and Guidance users report
the steepest learning curve (low confidence, community reports).

### Proposal D: Wire (axis: agent token cost and tooling)

**Idea.** Optimize for the agent writing and the tools reading. One canonical, compact, regular form
with short keywords; every item exports its grammar (GBNF, llguidance) and renders its exact bytes per
provider; layout is cache-ordered by rule; budgets are in the header in tokens.

```
prompt triage(t:Ticket)->Triage fail ModelError uses model~300tok=
  m fast
  s "You route support tickets."
  u t"Subject: {t.subject}\nBody: {t.body}"
  ok r.summary!=""
```

```
prompt extract(doc:str)->Invoice fail ModelError uses model~800tok=
  m small
  s "Extract invoice fields. Use none when absent."
  x fixtures.invoice_pairs
  --cache
  u t"{doc}"
  ok r.total>=0usd
```

`--cache` marks the provider cache breakpoint; the checker rejects interpolated text above it.

```
prompt answer(q:str,docs:List[Doc])->Answer fail ModelError uses model~600tok=
  m fast
  s "Answer from the documents only. Cite each claim by id."
  --cache
  u t"{docs}\nQ: {q}"
  ok r.cites.all(c->c in docs.map(.id))
```

```
prompt triage4(t:Ticket)->Triage fail ModelError uses model~1500tok=
  m fast|large
  s "You route support tickets."
  u t"Subject: {t.subject}\nBody: {t.body}"
  n 5 vote
  fix 2
  ok r.summary!=""
```

```
prompt support(q:str)->Reply fail ModelError uses model~800tok,orders.read=
  m fast
  s "Answer the customer. Look orders up; never guess."
  u t"{q}"
  t orders.lookup*3
```

Tooling, the real content of D: `yan prompt render triage --provider anthropic` prints the exact
request; `yan prompt grammar triage --gbnf` exports the constraint; `yan prompt cost` estimates tokens
per call from the rendered fixtures; the item hash covers the rendered template, model ids and decode
settings.

**Wins.** Smallest structured form after A; easy to generate; the tooling ideas (render, grammar,
cost, cache rule) are cheap and valuable in any proposal.

**Loses.** Single-letter keywords cost readability and, per the project's learnability work and
MultiPL-E results on low-resource languages, likely cost model accuracy too: models write familiar
words better than novel sigils (medium confidence). Saving ten tokens per prompt is negligible next
to the prompt's own prose.

### Proposal E: Page (axis: readability and craft)

**Idea.** A prompt is a document that happens to be typed. Prose is the default inside the body:
no quotes, `{expr}` interpolates, a role keyword on its own line starts a lane, examples are tables,
checks have names. Code is the exception, prose the rule, which is the opposite of every
string-in-code framework and the same direction as POML and dotprompt, but without leaving Yan's lexis.

```
prompt triage(t: Ticket) -> Triage fail ModelError uses model =
  system
    You route support tickets to the team that owns them.
    P1 means customers cannot pay or sign in.
  user
    Subject: {t.subject}
    Body: {t.body}
  check summary_present: r -> r.summary != ""
```

```
prompt extract(doc: str) -> Invoice fail ModelError uses model =
  system
    Extract invoice fields. Use none when a field is absent.
    Think about which figure is the grand total before you answer.
  examples
    | doc                         | vendor | total    | due        |
    | "ACME ... Total due 120.00" | "ACME" | 120usd   | 2026-12-01 |
    | "Bolt Ltd ... sum 9.50"     | "Bolt" | 9.50usd  | none       |
  user
    {doc}
  check non_negative: i -> i.total >= 0usd
```

```
prompt answer(q: str, docs: List[Doc]) -> Answer fail ModelError uses model =
  system
    Answer from the documents only. Cite each claim by document id.
  user
    {for d in docs}
    Document {d.id}:
    {d.text}
    {end}
    Question: {q}
  check cites_known: a -> a.cites.all(c -> c in docs.map(.id))
```

```
prompt triage_robust(t: Ticket) -> Triage fail ModelError uses model =
  system
    You route support tickets to the team that owns them.
  user
    Subject: {t.subject}
    Body: {t.body}
  strategy
    sample 5, vote
    repair twice
    fall back to "large"
  check summary_present: r -> r.summary != ""
```

```
prompt support(q: str) -> Reply fail ModelError uses model, orders.read =
  system
    Answer the customer. Look orders up; never guess an order.
  user
    {q}
  tools
    orders.lookup, at most 3 calls
```

**Wins.** Reads like the prompt it sends. Diffs are prose diffs. Examples as tables are both readable
and typed (each column is a field). Named checks give readable repair messages to the model.

**Loses.** Template control flow (`{for}`) inside prose reintroduces a template language. The
`strategy` block's English-like clauses ("repair twice") are cute and then annoying: a second syntax
for numbers. Nothing stops untrusted text from landing in `system`.

## 6. Scorecard and experiment

**Size experiment.** All five tasks per proposal, counted from the fenced blocks of each proposal
in section 5 by a short script (non-blank lines, characters, characters divided by four as a rough
token estimate; shared types excluded). The baseline is the RFC 0002 D3 sketch with its
`docs/surfaces.md` mock, written for the same five tasks in appendix 10. Treat the token column as
relative only: it is not tokenizer output.

| Proposal | Lines | Characters | Est. tokens | vs. baseline |
| --- | --- | --- | --- | --- |
| Baseline (RFC sketch) | 30 | 1211 | 303 | 1.00x |
| A Signature | 12 | 698 | 174 | 0.58x |
| B Ledger | 37 | 1492 | 373 | 1.23x |
| C Weave | 36 | 1348 | 337 | 1.11x |
| D Wire | 30 | 963 | 240 | 0.80x |
| E Page | 47 | 1572 | 393 | 1.30x |
| Mix (section 7.3, five tasks) | 30 | 1301 | 325 | 1.07x |

The baseline undercounts its own needs: T4 calls a `triage_large` it does not declare, its retry
does not feed the error back, and T5 cannot bound tool calls. The mix count includes a test, an eval
and doc comments that the others omit.

**Reading.** Size differences come mostly from prose and from strategy lines, not from keywords. D's
sigils save 20% over the keyword baseline; A saves 42% by deleting prose and letting the renderer
write it. So the token axis is won by defaults (A's renderer and header-as-instruction), not by short
spellings (D's sigils). The mix lands within 7% of baseline size while adding trust lanes, reasoning order,
bounded tools, tests and evals.

**Scorecard** (1 weak to 5 strong; judgment, not measurement):

| Axis | A | B | C | D | E |
| --- | --- | --- | --- | --- | --- |
| Brevity | 5 | 2 | 3 | 4 | 3 |
| Safety, explicitness | 2 | 5 | 2 | 2 | 2 |
| Expressive power | 2 | 3 | 5 | 3 | 3 |
| Agent writing (familiar, regular) | 5 | 3 | 2 | 3 | 4 |
| Human reading | 3 | 3 | 2 | 1 | 5 |
| Tooling, parsing, round-trip | 4 | 4 | 2 | 5 | 3 |
| Portability across providers | 5 | 4 | 2 | 4 | 4 |
| Fits law 1 (lowering only) | 5 | 5 | 3 | 5 | 4 |

**Checks against Yan constraints.**

- All five keep `fail` and `uses` in the header (law 4). D's `~300tok` budget is header sugar for
  `model.only(max_tokens=300)`.
- All five lower to plain Yan plus std, with three std gaps found: a `vote` combinator, a repair loop
  that feeds validation errors back (`repair n`), and decode-time constraints over runtime values
  (`cites in docs.map(.id)`). The first two are plain Yan library code. The third is a handler
  extension; the result type stays static, so 17.4's "a model never selects a type" holds.
- C's interleaved holes are not lowerable to one `generate_chat` call on most providers. They are
  lowerable when the holes are read as a schema order plus a presentation: one structured call whose
  record fields are the holes in source order, with the surrounding prose rendered as format
  guidance. That reading keeps C's main benefit (order control) and drops interleaving.
- C's `loop ... tool call` duplicates the agent loop (RFC 0014) inside a prompt. Tool use belongs in a
  single `tools` clause that lowers to the handler's `tool_call` loop, with the cap from the header.

## 7. The mix

### 7.1 Principles

1. **The header is the contract.** (A) Types, doc comments and field docs are the schema and its
   descriptions; no hand-written "reply in JSON" text, ever.
2. **Prose is the body.** (E) A lane keyword on its own line opens an indented prose block with
   `{expr}` interpolation; a one-line lane can stay on the keyword's line. No quotes, no dedent rules.
3. **Lanes are typed by trust.** (B) `system` and `user` accept only trusted values. `data` accepts
   anything, `Untrusted` included, and the handler always spotlights it (delimits and marks it as data,
   never instructions). Interpolating an untrusted value into `system` or `user` is an error that
   suggests `data`. This uses R452's existing label propagation; no new type machinery.
4. **Generation order is declared.** (C, reduced) The result record's field order is the generation
   order, and `think` before the result type asks for free reasoning first. Holes in the `assistant`
   lane are allowed and mean "these fields, in this order, presented like this"; they never mean
   interleaved calls.
5. **Strategy is a clause, not a framework.** (B, E) `check`, `repair n`, `sample n`, `vote`, `fallback`
   are one-line clauses with numbers, lowering to plain Yan combinators that anyone can call directly.
6. **Learned text is pinned.** (C) `learn` marks an instruction or shot list as tuned offline by
   `yan tune` against a metric. The tuned value is written to a checked-in file next to the source and
   folded into the item hash; builds read it, never tune.
7. **The checker owns injection safety.** (B) An item that sees untrusted data and holds an outbound
   capability is rejected unless it carries an audited waiver. Tools are capabilities, attenuated in
   the header.
8. **Layout is cache-ordered by rule.** (D) Lanes render in a fixed order: `system`, shots, then
   trusted `user` text, then `data`. Stable text is always before varying text; the checker warns when
   a `system` lane interpolates anything that is not stage-0.
9. **Presentation is a handler concern.** (3.3, POML) How a record, a table, a list of documents or a
   shot renders (JSON, XML-tagged, Markdown table) is a renderer setting on the handler or the package,
   not text in each prompt, so it can be measured and swapped per model.
10. **Show the bytes.** (D) `yan prompt render`, `grammar` and `cost` are first-class tools.

### 7.2 Grammar sketch (mock)

```
prompt_item = [doc] "prompt" NAME [tparams] "(" [params] ")" "->" ["think"] type
              ["fail" type] "uses" caps ["=" NEWLINE INDENT {clause} DEDENT]
clause      = lane | shots | tools | strategy | check | ex | eval
lane        = ("system" | "user" | "data" | "assistant") (prose_line | NEWLINE INDENT prose DEDENT)
prose       = { text_line }            -- text with {expr} interpolation; {?field} holes in assistant
shots       = ["learn" INT] "shots" expr NEWLINE
            | "shots" NEWLINE INDENT table DEDENT
tools       = "tools" path {"," path} ["max" INT] NEWLINE
strategy    = "repair" INT | "sample" INT ["seed" expr] | "vote" | "fallback" STRING
check       = "check" [NAME ":"] lambda NEWLINE
eval        = "eval" expr "by" expr ["budget" expr] NEWLINE
```

Lowering, in order: `model.only(...)` from the header's `uses` and `fallback`; messages for
`generate_chat[T]` from lanes in fixed order, with `data` rendered by the handler's spotlight
renderer; `think` becomes a two-step call (free, then constrained) or a leading scratch field,
handler's choice, both recorded; `sample n` becomes `par for i in 0..n` with seeds `i` (or `seed e`
plus `i`); `vote` is `std` majority with a stable tie break (first in index order); `repair n` retries
with the named check's failure message added as a `user` lane; `check` lambdas compose into one
`validate`; without `check` the result stays `Untrusted[T]`.

### 7.3 The five tasks in the mix

```
--- Route a support ticket to the team that owns it.
--- P1 means customers cannot pay or sign in.
prompt triage(t: Ticket) -> Triage fail ModelError uses model.only(models=["fast"]) =
  data t
  check summary: r -> r.summary != ""
  ex triage(fixtures.outage) == Ok(fixtures.outage_triage)
     with model = fake_model(fixtures.outage_reply)
```

T1 needs no `system` or `user` lane: the doc comment is the instruction and the ticket is data. This
is A's brevity with B's safety: the ticket text cannot act as an instruction, whoever wrote it.

```
--- Extract invoice fields. Use none when a field is absent.
prompt extract(doc: str) -> think Invoice fail ModelError uses model.only(models=["small"]) =
  learn 4 shots fixtures.invoice_pairs
  data doc
  check non_negative: i -> i.total >= 0usd
  eval fixtures.invoice_test by exact budget 2usd
```

`think` gives reasoning first (Tam et al.); `learn 4 shots` picks and orders four shots offline, pinned;
`eval` runs only under `yan eval` with a real model and a cost ceiling.

```
--- Answer from the documents only. Cite each claim by document id.
prompt answer(q: str, docs: List[Doc]) -> Answer fail ModelError uses model =
  user Question: {q}
  data docs
  check cited: a -> a.cites.all(c -> c in docs.map(.id))
```

`docs` may arrive as `List[Untrusted[Doc]]`; the `data` lane accepts it, and `user` would reject it.
The `check` doubles as a decode-time constraint where the handler supports enums over runtime values.

```
--- Route a support ticket to the team that owns it.
prompt triage_robust(t: Ticket) -> Triage fail ModelError uses model.only(models=["fast", "large"]) =
  data t
  sample 5
  vote
  repair 2
  fallback "large"
  check summary: r -> r.summary != ""
```

The same strategy is also plain Yan over T1, `vote(par for i in 0..5 -> triage(t) seed i repair 2)`;
both lower to the same code. The clause form exists for readers who want the strategy on the item.
`repair 2` sends the failing check's name and message back to the model, so named checks double as
repair instructions.

```
--- Answer the customer. Look orders up; never guess an order.
prompt support(q: str) -> Reply fail ModelError uses model, orders.read =
  data q
  tools orders.lookup max 3
```

The trifecta check passes: untrusted input yes, outbound capability no. With `mail.send` added, the
item needs `@accept_risk("reason")` and shows up in `yan audit`.

A longer prompt, to show prose lanes at length:

```
--- Write a release note for one merged change.
prompt note(c: Change, style: Style) -> Note fail ModelError uses model =
  system
    You write release notes for engineers who skim.
    Lead with what changed for the reader, then why. One paragraph, no marketing words.
  user
    Write in the {style.name} style. Keep it under {style.max_words} words.
  data c.title, c.description, c.diff_summary
  assistant
    Change: {?headline}
    {?body}
  check short: n -> n.body.words() <= style.max_words
```

`{?headline}` and `{?body}` are holes naming `Note`'s fields: they fix order and presentation; the call
is still one structured generation.

### 7.4 Why this mix

- **Concise where it is free:** T1 in the mix is seven lines including its doc comment and a test,
  against nine in the RFC sketch, and the ticket text can no longer act as an instruction.
- **Opinionated without being obnoxious:** one way to say each thing; strategy clauses take numbers,
  not English; the checker refuses injection-shaped code but offers the `data` fix and an audited
  waiver.
- **Powerful:** order control, learned slots, runtime-value constraints, strategies and tools, all
  lowering to plain Yan, so power never means a second semantics.
- **Crafted:** prose is prose, tables are tables, checks have names that become repair messages, and
  `yan prompt render` shows exactly what will be sent.
- **Vanguard, in three ideas no peer has together:** trust-typed lanes checked by the compiler, the
  lethal-trifecta check over capabilities, and tuned prompts pinned into a content-hashed,
  deterministic build.

## 8. Open questions

1. **Doc comment as instruction.** Is it acceptable that a `---` doc comment changes behavior? It is
   prompt text, so it is semantics. Options: allow it only on `prompt` items (proposed), or require a
   `system` lane always.
2. **Renderer as semantics.** Presentation moves accuracy (3.3), so the renderer is part of behavior.
   Should the renderer id be in the item hash and the trace header (proposed), and where is it chosen:
   handler, package manifest, or both?
3. **Runtime-value constraints.** `c in docs.map(.id)` at decode time needs a handler hook for enums
   over runtime values. Is that a std addition to `generate_chat`, or validation-only in the first
   slice?
4. **`think` lowering.** Two calls (NL-to-format) cost latency; a leading scratch field costs schema
   purity. Handler choice (proposed) or fixed?
5. **`learn` and determinism.** The tuned file must be an input to the build, like `yan.lock`. Does it
   live in `yan.lock`, a sibling `.ypm.tune` file, or the source itself (rewritten by `yan tune`)?
6. **Trifecta rule scope.** Which capabilities count as outbound (net, mail, fs writes, process)?
   Should it be a lint first and an error later?
7. **Spotlighting format.** Delimiting versus datamarking versus encoding have different costs; the
   handler picks, but should the item be able to request one?
8. **Tables in `shots`.** Each column is a field and each row a typed literal. Does the table syntax
   belong to Yan Prompt only, or to Yan (it would help tests in every surface)?
9. **Holes beyond `assistant`.** The mix restricts holes to the assistant lane and to result fields.
   Is interleaved generation (C) worth a later engine item under R568 for providers that support it?
10. **Evals.** `eval` with real models and budgets is run by `yan eval`, never by `yan test`. Is that
    split right, and do eval results get recorded traces for replay?
11. **Relation to the agent loop.** `tools` in a prompt overlaps with RFC 0014's loop. Proposed:
    a prompt's `tools` is a bounded inner loop; anything longer is an agent, not a prompt.

## 9. Sources

Peers:

- BAML: https://gradientflow.com/structured-prompt-engineering-made-easy/ and
  https://sebgnotes.substack.com/p/baml-bringing-software-engineering
- DSPy and GEPA: https://dspy.ai/tutorials/gepa_ai_program/ ; Agrawal et al., "GEPA: Reflective Prompt
  Evolution Can Outperform Reinforcement Learning", arXiv 2507.19457 (ICLR 2026)
- dotprompt: https://google.github.io/dotprompt/reference
- POML: Zhang et al., "Prompt Orchestration Markup Language", arXiv 2508.13948 (2025)
- PDL: Vaziri et al., "PDL: A Declarative Prompt Programming Language", arXiv 2410.19135 (2024);
  https://github.com/IBM/prompt-declaration-language
- APPL: Dong et al., arXiv 2406.13161 (ACL 2025)
- SGLang: Zheng et al., "SGLang: Efficient Execution of Structured Language Model Programs",
  arXiv 2312.07104
- LMQL, Guidance, Outlines, Instructor, TypeChat, Pydantic AI, Mirascope, ell, Prompty, Promptfoo:
  project documentation (not re-fetched for this study; descriptions from prior knowledge, medium
  confidence on details)

Research:

- Sclar et al., "Quantifying Language Models' Sensitivity to Spurious Features in Prompt Design",
  ICLR 2024, arXiv 2310.11324
- Tam et al., "Let Me Speak Freely? A Study on the Impact of Format Restrictions on Performance of
  Large Language Models", arXiv 2408.02442
- Geng et al., "Generating Structured Outputs from Language Models: Benchmark and Studies"
  (JSONSchemaBench), arXiv 2501.10868 (ICML 2025)
- Debenedetti et al., "Defeating Prompt Injections by Design" (CaMeL), arXiv 2503.18813
- Beurer-Kellner et al., "Design Patterns for Securing LLM Agents against Prompt Injections",
  arXiv 2506.08837; summary https://simonwillison.net/2025/Jun/13/prompt-injection-design-patterns/
- Liang et al., "Prompts Are Programs Too! Understanding How Developers Build Software Containing
  Prompts", FSE 2025, arXiv 2409.12447
- Lu et al., "Fantastically Ordered Prompts and Where to Find Them", ACL 2022 (from prior knowledge)
- Liu et al., "Lost in the Middle", TACL 2024 (from prior knowledge)
- Hines et al., "Defending Against Indirect Prompt Injection Attacks With Spotlighting", 2024; Wallace
  et al., "The Instruction Hierarchy", 2024 (from prior knowledge)

## 10. Appendix: baseline for the size experiment

The five tasks in the current RFC sketch style (`model`, quoted `system` and `user`, `shots`, `tools`,
`check`):

```
prompt triage(t: Ticket) -> Triage fail ModelError uses model =
  model "fast"
  system "You route support tickets to the team that owns them. Reply only in the schema."
  user t"""
    Subject: {t.subject}
    Body: {t.body}
    """
  check r -> r.summary != ""
prompt extract(doc: str) -> Invoice fail ModelError uses model =
  model "small"
  system "Extract invoice fields. Use null when a field is absent. Reason first, then reply in the schema."
  shots fixtures.invoice_pairs
  user t"{doc}"
  check r -> r.total >= 0usd
prompt answer(q: str, docs: List[Doc]) -> Answer fail ModelError uses model =
  model "fast"
  system "Answer from the documents only. Cite each claim by document id. Reply only in the schema."
  user t"""
    Documents: {docs}
    Question: {q}
    """
  check a -> a.cites.all(c -> c in docs.map(.id))
pub triage_robust(t: Ticket) -> Triage fail ModelError uses model =
  let rs = par for i in 0..5 -> retry(2, () -> triage(t))
  vote(rs) else triage_large(t)
prompt support(q: str) -> Reply fail ModelError uses model, orders.read =
  model "fast"
  system "Answer the customer. Look orders up; never guess an order. Reply only in the schema."
  tools orders.lookup
  user t"{q}"
```
