# Yan Workflow 言序 (`.ywf`): syntax study

Status: brainstorm study, 2026-11-04. Not normative. No spec rows, no diagnostic codes. Every code
block below is **MOCK** syntax in the sense of `docs/surfaces.md`: it shows what the surface could look
like while staying inside the one core. Where anything here disagrees with `spec/`, `spec/` wins.

Inputs: RFC 0002 D6 (R541), spec 9.6.1 (R416 `@durable`, R453 resume and versions, R552 durable
timers), spec 9.2 to 9.5 (`par`, cancellation, deadlines, channels), spec 16.7 (admission rules N1 to
N9), `docs/surfaces.md` section 10.

## Summary

- The strongest idea Yan has here needs no syntax. Durable workflow engines check determinism only at
  replay time, in production. Yan can check it at compile time, because capabilities are typed and a
  workflow may use only journaling ones (E725 already exists). That makes `step` redundant: every
  journaled call is a step, and its binding name is the step name.
- Versioning is the top complaint about Temporal-style engines. Yan has content-addressed item hashes
  (3.6), so the default can be **hash-pinned runs**: a run finishes on the code it started on, and
  picks up new code only at `again` (continue-as-new). `version` blocks are needed only for workflows
  that opt into rolling upgrades.
- Compensation becomes a clause, `undo e`, a durable `errdefer`: it runs in reverse order when a
  failure leaves the workflow before `commit`.
- Waiting on time, messages and conditions is one form, `first`, with arms for inbox messages,
  `after d`, `at t` and `until c every d`. Each race outcome is journaled, so replay stays
  deterministic.
- Five proposals (Terse, Ledger, Score, Story, Chart) each push one axis. The consolidated mix adds
  seven keywords to Yan (`workflow`, `wait`, `first`, `after`, `undo`, `commit`, `again`) plus
  contextual clause words (`inbox`, `retry`, `backoff`, `within`, `until`, `every`, `at`, `start`,
  `done`, `version`), and keeps the rest plain Yan. It costs about 13 percent more tokens than the
  tersest proposal and a quarter fewer than the most explicit one.

## 1. Methodology and process

1. **Read narrowly.** `docs/surfaces.md` section 10 (the RFC example and the existing mock grammar),
   RFC 0002 D6, the spec rows it cites (R416, R453, R552, R305 trace format, R419 delivery plan),
   spec 9.2 to 9.5 for `par`, cancellation, deadlines and `select`, spec 16.7 for admission rules and
   design law 4 (five facts). No reading of conformance or crates.
2. **Market scan.** Fourteen systems in four families (section 2.1), from vendor docs and
   announcements and from what I already know of them. Five web searches and four fetched pages for
   the newest items: AWS Lambda durable functions (Dec 2025), Vercel Workflow DevKit, Serverless
   Workflow 1.0, a 2026 industry survey of durable execution, Temporal versioning complaints, and a
   2026 resume-semantics paper.
3. **Literature.** Classic papers on sagas, long-running transactions, workflow patterns,
   orchestration calculi and statecharts, plus systems papers on durable functions and
   log-based fault tolerance (section 2.3). Each source carries a confidence mark: **H** (I read it,
   or it is a standard reference I know well), **M** (summary or single source), **L** (recalled,
   not checked today).
4. **Fixed task set.** Five tasks (section 3) chosen to cover the workflow patterns users hit most:
   sequence, parallel split and join, durable sleep, deferred choice (race of time and message),
   sagas, fan-out with a limit, human-in-the-loop, an LLM step, a forever loop with continue-as-new,
   and a code change while runs are in flight.
5. **Five proposals.** Each optimizes one axis and is allowed to be extreme on it. All five keep
   design law 4 (the five facts stay visible) and Yan lexis (15.1), because any proposal that broke
   either would fail admission (N5, N9) and teach nothing.
6. **Experiment.** I wrote all 25 samples, then measured each proposal's five samples for lines,
   non-space characters and lexical tokens (a BPE tokenizer was planned, but its vocabulary download
   is blocked here, and the Anthropic token count waits for the API key). I also counted new words
   and the lines changed by the T5 code change. Results in section 5.
7. **Mix.** Scored the proposals per feature against the brief (elegant, concise, powerful,
   expressive, opinionated without being obnoxious) and against Yan's laws, took the winner per
   feature, and rewrote the five tasks in the mix. A mock grammar and a list of lowerings close it.

## 2. Findings

### 2.1 Peers and competitors

| Family | Systems | Authoring model | What users like | What users dislike |
| --- | --- | --- | --- | --- |
| Replay engines | Temporal, Cadence, Azure Durable Functions, durabletask | Code in a host language; the engine replays the function against an event history | Plain code, testing with time skipping, signals and queries, huge scale | Determinism rules enforced only at replay; versioning with `patched`/`getVersion`; history size limits and continue-as-new; workers to run |
| Journaled steps | Restate, Inngest, DBOS, Cloudflare Workflows, AWS Lambda durable functions, Vercel Workflow DevKit, Hatchet, Trigger.dev | Code with an explicit step API (`ctx.run`, `step.run`, `step.do`, `context.step`) or a directive (`"use step"`); results checkpointed | Code between steps may be anything; no worker fleet (serverless); cheap waits | The step boundary is manual and easy to get wrong; code outside steps re-runs; idempotency is the user's job |
| Declarative state machines | AWS Step Functions (ASL JSON), Serverless Workflow 1.0 (YAML), Argo, Kestra, GitHub Actions, Conductor | JSON or YAML documents | Visual graphs, no code to deploy, static validation | Programming in YAML: expressions in strings, weak types, poor reuse, painful loops and error handling |
| Process notations | BPMN 2.0 (Camunda, Zeebe), statecharts (XState) | Diagrams with an XML or JS form | Business readers, compensation events, timers and message events as first-class | Heavy tooling, round-trip between diagram and code is lossy |
| Data and agent orchestrators | Airflow, Dagster, Prefect, LangGraph | Python DAGs or graphs with checkpointers | Lineage, retries per task, scheduling | DAGs are a poor fit for long waits, branching and human input; agents forced into pipelines |

Sources: Temporal versioning docs (H), Temporal community thread "Versioning difficulty" (H), AWS
Lambda durable functions announcement and InfoQ coverage, Dec 2025 (H, fetched), Vercel Workflow
DevKit docs on `"use workflow"` and `"use step"` (M), Serverless Workflow 1.0.0 release notes (M),
ZenML "Where durable execution is headed", 2026 (M, fetched), Restate, Inngest, DBOS, Cloudflare,
Step Functions, Camunda docs (H, known).

### 2.2 What users want, in order of how often it comes up

1. **Safe code changes while runs are in flight.** The single most cited pain. Temporal's
   `getVersion` and `patched` calls pile up and are removed by hand; one missing patch replays
   history against new code and breaks production (the "One line, one outage" post, M). Temporal's
   own answer, worker versioning with pinned and auto-upgrade workflows, moved the pin from code to
   deployment.
2. **Determinism without a rule book.** Users learn the rules (no clocks, no random, no direct I/O,
   no threads) by failing at replay. Journaled-step engines relax the rules by making code between
   steps re-runnable, at the cost of re-running it.
3. **Plain code, not YAML.** The move from Step Functions and Airflow to code-first engines is the
   decade's clearest trend; Lambda durable functions is AWS itself making the move.
4. **Waits that cost nothing**: sleeps of days or months, waits for an event or an approval, with a
   timeout.
5. **Human-in-the-loop and agents.** Approvals, review queues, LLM calls whose results must be cached
   because re-running them costs money and gives other answers.
6. **Sagas and idempotency** handled by the platform, not by hand.
7. **Visibility**: a timeline of a run, its current state as a query, a graph of the workflow.
8. **Fan-out with limits and partial failure.**
9. **Testing with simulated time**, and replay of production histories as a deploy check.

### 2.3 Research

| Source | Claim used here | Conf. |
| --- | --- | --- |
| Garcia-Molina and Salem, "Sagas", SIGMOD 1987 | A long transaction as steps with compensations run in reverse order on failure | H |
| Butler, Hoare and Ferreira, "A trace semantics for long-running transactions", 2004 (compensating CSP) | Compensation composes with sequence and parallel; compensations of parallel steps run in parallel | H |
| van der Aalst et al., "Workflow patterns", Distributed and Parallel Databases, 2003 | The control-flow pattern catalogue; deferred choice and milestone are the patterns code-first engines handle worst | H |
| Kitchin, Cook and Misra, "A language for task orchestration and its semantics" (Orc), CONCUR 2009 | Orchestration needs only four combinators (sequence, parallel, pruning, otherwise); timers are just sites | H |
| Harel, "Statecharts", 1987 | Hierarchical states and events; the source of "workflow as a state machine" | H |
| Burckhardt et al., "Durable functions: semantics for stateful serverless", OOPSLA 2021 | Formal model of replay-based orchestration; determinism as a correctness condition | H |
| Burckhardt et al., "Netherite", VLDB 2022 | Speculative commit makes journaled orchestration fast | M |
| Zhang et al., "Beldi", OSDI 2020; Jia and Witchel, "Boki", SOSP 2021 | Exactly-once semantics for serverless workflows from logged operations and idempotency keys | H |
| Skiadopoulos et al., "DBOS: a DBMS-oriented operating system", PVLDB 2021 | Workflow state in the database; steps as transactions | M |
| Khan, "Resume means resume", arXiv 2608.03836, 2026 | Six checkable resume properties (prefix continuation, effect exactly-once, fork determinism, checkpoint validity, consume-once, recovery determinism); five agent frameworks fail some | M (single author, unreviewed) |
| Unison language (content-addressed code) | Code identified by hash makes old and new versions coexist without conflict | H |
| Zig `errdefer` | Cleanup that runs only on the error path, written next to the acquisition | H |
| Montesi, choreographic programming; Choral, TOPLAS 2024 | One global description projected to participants; relevant to signals between workflows | M |

### 2.4 What Yan already gives a workflow surface

- **Static determinism.** An `@durable` function may use only journaling capabilities (E725).
  Clocks, randomness and I/O are capabilities, so the checker knows statically that a workflow is
  replayable. No mainstream engine has this; it is the headline.
- **Exact journal.** R305 entries carry op index, capability, operation, argument digest and result.
  Timer races and `nondet` choices are journaled where they occur, so a race between a message and a
  deadline replays exactly.
- **Idempotency keys** per operation (R453) and W850 for handlers without idempotency support.
- **Durable timers** (R552) that let the process exit while a run sleeps.
- **`par` semantics** with deterministic lowest-index failure (9.2).
- **Deploy check**: `yan replay --against REV` over stored journals.
- **Item hashes** (3.6): code identity is already content-addressed.
- **Session types** (R634) for typed protocols between workflows, later.

What is missing in the core or std (named here, not proposed as rows): continue-as-new
(`durable.restart`), a journaled inbox capability for external messages, queryable run state,
and hash-pinned resume (keeping an old item's code reachable for its in-flight runs).

## 3. The task set

Every proposal writes the same five tasks.

- **T1 Onboard.** Create an account, start a 14-day trial, in parallel send a welcome mail and book a
  call; after 13 days send a reminder; wait up to 2 days for conversion, else downgrade.
- **T2 Order saga.** Reserve stock, charge the card, book shipping. If any step fails, undo the
  earlier ones in reverse order. Once shipping is booked there is no going back; then mail the
  customer.
- **T3 Expense approval.** Wait up to 48 hours for a decision; on timeout escalate to the manager and
  wait 24 hours more; then reject. Expose the current stage to callers.
- **T4 Document digest.** For each document, at most 8 at a time, extract facts with an LLM; keep
  partial failures; summarize the good ones with an LLM; if any failed, wait up to 3 days for a
  human review.
- **T5 Subscription.** Charge monthly until a cancel message; start a fresh journal each cycle. In a
  later deploy, send a receipt after each charge.

## 4. Five proposals

### P1 Terse: brevity first

Axis: fewest tokens for a reader who already knows the surface. Steps are implicit, bindings drop
`let`, races and undos are operators. Keeps `?`, `par` and the `uses` header, since law 4 forbids
dropping them.

New forms: `x = e` binding (workflow bodies only), `wait d`, `wait c | d` (true if `c` held before
`d`), `e ~ u` (undo `u` if a later failure escapes), `e !` (commit), `<- name: T` (inbox in the
header), `wait name | d` (message or timeout, an Option), `again f(args)`.

T1:

```
workflow onboard(user: UserId) -> Done fail OnboardError uses crm, billing, mail, timer =
  acct = crm.create_account(user)?
  billing.start_trial(acct, 14d)?
  par
    mail.send(t"Welcome, {user}")?
    crm.schedule_call(acct)?
  wait 13d
  mail.send(t"Your trial ends tomorrow")?
  if not wait billing.converted(acct)? | 2d
    billing.downgrade(acct)?
  Done
```

T2:

```
workflow order(o: Order) -> Shipped fail OrderError uses stock, pay, ship, mail =
  hold = stock.reserve(o.items)? ~ stock.release(hold)
  paid = pay.charge(o.card, o.total)? ~ pay.refund(paid)
  label = ship.book(o.address, o.items)? !
  mail.send(t"Order {o.id} ships: {label.tracking}")?
  Shipped(label)
```

T3:

```
workflow expense(e: Expense) -> Verdict fail ExpenseError uses people, mail, timer
    <- decide: Verdict =
  pub var stage = Stage.waiting
  v = wait decide | 48h else
    stage = Stage.escalated
    mail.send(people.manager_of(e.owner)?, t"Expense {e.id} waits for you")?
    wait decide | 24h else Verdict.rejected("no answer in 72h")
  stage = Stage.done
  v
```

T4:

```
workflow digest(docs: List[DocId]) -> Digest fail DigestError uses store, model, review, timer
    <- reviewed: List[Fix] =
  rs = par for d in docs limit: 8
    try extract(d)
  sum = model.generate(summarize(rs.oks()))?
  if rs.errs().is_empty()
    return Digest(sum, fixes: [])
  review.open(rs.errs())?
  fixes = wait reviewed | 3d else fail DigestError.no_review
  Digest(sum, fixes:)

workflow extract(d: DocId) -> Facts fail DigestError uses store, model =
  parse_facts(model.generate(extract_prompt(store.read(d)?))?) else fail DigestError.bad(d)
```

T5:

```
workflow subscription(s: Sub, n: int) -> Ended fail BillingError uses pay, mail, timer
    <- cancel: unit =
  if wait cancel | s.next_bill(n)
    return Ended(cycles: n)
  r = pay.charge(s.card, s.price)?
  mail.send(s.owner, receipt(r))?        -- added in the later deploy
  again subscription(s, n + 1)
```

Verdict: among the two smallest (section 5), with the fewest new words. `~` and `!` are cryptic, `wait c | d` overloads one
form for three meanings, and dropping `let` breaks rule 2 of the shared mock shape (statements are
plain Yan). Keep: implicit steps, `again`, the inbox in the header.

### P2 Ledger: safety and explicitness first

Axis: nothing implicit that could surprise an operator at 3 a.m. Every step is named, and the
journal keys steps by name, not op index, so reordering code never mis-replays. Every step states its
retry policy (no default). Sagas are explicit blocks with a `pivot` step after which compensation
stops. Messages, queries and timers are declared and named.

New forms: `step NAME: stmt`, `policy NAME = ...`, `using POLICY`, `saga`, `pivot`, `compensate`,
`signal`, `query`, `timer NAME: wait d`, `poll e every d for d`, `select NAME`, `version`,
`continue_as_new`, `at_most_once by e`.

T1:

```
policy net = retry 3, backoff 1s..1m
policy mailer = retry 5, backoff 1m..1h

workflow onboard(user: UserId) -> Done fail OnboardError
    uses crm, billing, mail, timer =
  step create_account: let account = crm.create_account(user)? using net
  step start_trial: billing.start_trial(account, 14d)? using net
  par
    step welcome: mail.send(t"Welcome, {user}")? using mailer
    step book_call: crm.schedule_call(account)? using net
  timer trial_end: wait 13d
  step nudge: mail.send(t"Your trial ends tomorrow")? using mailer
  step converted: let ok = poll billing.converted(account)? every 1h for 2d using net
  if not ok
    step downgrade: billing.downgrade(account)? using net
  Done
```

T2:

```
workflow order(o: Order) -> Shipped fail OrderError uses stock, pay, ship, mail =
  saga
    step reserve: let hold = stock.reserve(o.items)? using net
      compensate stock.release(hold)
    step charge: let paid = pay.charge(o.card, o.total)? retry 3, at_most_once by o.id
      compensate pay.refund(paid)
    pivot book: let label = ship.book(o.address, o.items)? using net, timeout 30s
  step notify: mail.send(t"Order {o.id} ships: {label.tracking}")? using mailer
  Shipped(label)
```

T3:

```
workflow expense(e: Expense) -> Verdict fail ExpenseError
    uses people, mail, timer
    signal decide: Verdict
    query stage: Stage =
  var stage = Stage.waiting
  let v = select first_window
    signal decide(v) -> v
    timer 48h ->
      stage = Stage.escalated
      step manager: let boss = people.manager_of(e.owner)? using net
      step escalate: mail.send(boss, t"Expense {e.id} waits for you")? using mailer
      select second_window
        signal decide(v) -> v
        timer 24h -> Verdict.rejected("no answer in 72h")
  stage = Stage.done
  v
```

T4:

```
policy llm = retry 3, backoff 5s..2m, timeout 2m

workflow digest(docs: List[DocId]) -> Digest fail DigestError
    uses store, model, review, timer
    signal reviewed: List[Fix] =
  step extract_all: let rs = par for d in docs limit: 8
    try child extract(d)
  step summarize: let sum = model.generate(summarize(rs.oks()))? using llm
  if rs.errs().is_empty()
    return Digest(sum, fixes: [])
  step open_review: review.open(rs.errs())? using net
  let fixes = select review_window
    signal reviewed(f) -> f
    timer 3d -> fail DigestError.no_review
  Digest(sum, fixes:)

workflow extract(d: DocId) -> Facts fail DigestError uses store, model =
  step read: let text = store.read(d)? using net
  step ask: let raw = model.generate(extract_prompt(text))? using llm
  parse_facts(raw) else fail DigestError.bad(d)
```

T5:

```
workflow subscription(s: Sub, n: int) -> Ended fail BillingError
    uses pay, mail, timer
    signal cancel: unit =
  select cycle
    signal cancel -> return Ended(cycles: n)
    timer until s.next_bill(n) -> ()
  step charge: let r = pay.charge(s.card, s.price)? retry 5, backoff 1h..1d, at_most_once by (s.id, n)
  version "2026-12-receipts"
    step receipt: mail.send(s.owner, receipt(r))? using mailer
  continue_as_new subscription(s, n + 1)
```

Verdict: the most auditable, and name-keyed steps are a genuinely better journal key than op index
(refactors stop being dangerous). But it is twice the length of Terse, `policy`/`using` on every line
is ceremony, and `step NAME:` duplicates the `let` name in most lines. Keep: name-keyed journal (with
the binding name as the default key), `at_most_once by`, explicit pivot (as `commit`), declared
messages and queries.

### P3 Score: expressiveness and power first

Axis: orchestration as an algebra, after Orc. A small set of combinators (`all`, `race`, `each`,
`hedge`, `saga`) compose, and handlers (`on`) run beside the main body, so signals, updates and
queries are code, not configuration. Combinators are ordinary std functions over workflow closures
where possible, so users can write their own (`quorum`, `hedge`, `retrying`).

New forms: `all`, `race`, `each ... limit`, `hedge d`, `saga`, `e <~ u` (undo), `sleep`,
`on signal`, `on query`, `on update`, `finish e` (end the run from a handler), `again`.

T1:

```
workflow onboard(user: UserId) -> Done fail OnboardError uses crm, billing, mail, timer =
  let account = crm.create_account(user)?
  billing.start_trial(account, 14d)?
  all
    mail.send(t"Welcome, {user}")?
    crm.schedule_call(account)?
  sleep 13d
  mail.send(t"Your trial ends tomorrow")?
  race
    eventually(every: 1h, billing.converted(account)?)
    sleep 2d then billing.downgrade(account)?
  Done
```

T2:

```
workflow order(o: Order) -> Shipped fail OrderError uses stock, pay, ship, mail, sms =
  let label = saga
    let hold = stock.reserve(o.items)? <~ stock.release(hold)
    let paid = pay.charge(o.card, o.total)? <~ pay.refund(paid)
    ship.book(o.address, o.items)?
  hedge 10m
    mail.send(t"Order {o.id} ships: {label.tracking}")?
    sms.send(o.phone, t"Shipped: {label.tracking}")?
  Shipped(label)
```

T3:

```
workflow expense(e: Expense) -> Verdict fail ExpenseError uses people, mail, timer =
  var stage = Stage.waiting
  on query status() -> Stage = stage
  on signal decide(v: Verdict) = finish v
  sleep 48h
  stage = Stage.escalated
  mail.send(people.manager_of(e.owner)?, t"Expense {e.id} waits for you")?
  sleep 24h
  Verdict.rejected("no answer in 72h")
```

T4:

```
workflow digest(docs: List[DocId]) -> Digest fail DigestError uses store, model, review, timer =
  let rs = each d in docs limit 8
    try extract(d)
  let sum = model.generate(summarize(rs.oks()))?
  if rs.errs().is_empty()
    return Digest(sum, fixes: [])
  review.open(rs.errs())?
  on signal reviewed(f: List[Fix]) = finish Digest(sum, fixes: f)
  sleep 3d
  fail DigestError.no_review

workflow extract(d: DocId) -> Facts fail DigestError uses store, model =
  let raw = model.generate(extract_prompt(store.read(d)?))?
  parse_facts(raw) else fail DigestError.bad(d)
```

T5:

```
workflow subscription(s: Sub, n: int) -> Ended fail BillingError uses pay, mail, timer =
  on signal cancel() = finish Ended(cycles: n)
  sleep until s.next_bill(n)
  let r = pay.charge(s.card, s.price)?
  mail.send(s.owner, receipt(r))?        -- added in the later deploy
  again subscription(s, n + 1)
```

Verdict: the most expressive, and T3 is the most elegant sample in the study: handlers plus a
straight-line main body read better than nested races. The cost is reasoning: an `on` handler that
mutates state runs between any two steps of the main body, which weakens local reasoning (law 6),
and `finish` from a handler is a non-local exit. `hedge` and `each` are library material, not
syntax. Keep: `on query` (read-only, no mutation, so law 6 holds), `again`, combinators as std
functions. Reject: mutating `on signal` handlers and `finish`.

### P4 Story: readability for reviewers who do not write Yan

Axis: an operations lead or a product owner can read the workflow aloud and approve it. Words over
symbols, time phrases up front, one verb per line. Steps start with `do`, names come after `as`, and
branches read as sentences.

New forms: `do e [as name]`, `meanwhile`, `after d`, `when c within d ... otherwise`,
`undo with e`, `from here on, no undo`, `on message name within d ... otherwise`, `show name`,
`then start over with`.

T1:

```
workflow onboard(user: UserId) -> Done fail OnboardError uses crm, billing, mail, timer =
  do crm.create_account(user)? as account
  do billing.start_trial(account, 14d)?
  meanwhile
    do mail.send(t"Welcome, {user}")?
    do crm.schedule_call(account)?
  after 13d
    do mail.send(t"Your trial ends tomorrow")?
  when billing.converted(account)? within 2d
    Done
  otherwise
    do billing.downgrade(account)?
    Done
```

T2:

```
workflow order(o: Order) -> Shipped fail OrderError uses stock, pay, ship, mail =
  do stock.reserve(o.items)? as hold, undo with stock.release(hold)
  do pay.charge(o.card, o.total)? as paid, undo with pay.refund(paid)
  do ship.book(o.address, o.items)? as label
  from here on, no undo
  do mail.send(t"Order {o.id} ships: {label.tracking}")?
  Shipped(label)
```

T3:

```
workflow expense(e: Expense) -> Verdict fail ExpenseError uses people, mail, timer
    hears decide: Verdict =
  show stage = Stage.waiting
  on message decide as v within 48h
    v
  otherwise
    stage = Stage.escalated
    do people.manager_of(e.owner)? as boss
    do mail.send(boss, t"Expense {e.id} waits for you")?
    on message decide as v within 24h
      v
    otherwise
      Verdict.rejected("no answer in 72h")
```

T4:

```
workflow digest(docs: List[DocId]) -> Digest fail DigestError uses store, model, review, timer
    hears reviewed: List[Fix] =
  for each d in docs, 8 at a time
    do try extract(d) as r
  collected as rs
  do model.generate(summarize(rs.oks()))? as sum
  when rs.errs().is_empty()
    Digest(sum, fixes: [])
  otherwise
    do review.open(rs.errs())?
    on message reviewed as fixes within 3d
      Digest(sum, fixes:)
    otherwise
      fail DigestError.no_review
```

T5:

```
workflow subscription(s: Sub, n: int) -> Ended fail BillingError uses pay, mail, timer
    hears cancel: unit =
  on message cancel before s.next_bill(n)
    Ended(cycles: n)
  otherwise
    do pay.charge(s.card, s.price)? as r
    do mail.send(s.owner, receipt(r))?   -- added in the later deploy
    then start over with subscription(s, n + 1)
```

Verdict: pleasant to read and close to how BPMN users talk, and `after 13d` with a block is clearer
than a bare `wait`. But `do` on every line is noise once you know the surface, `for each ... collected
as` is a second loop syntax that fights Yan's `par for`, and multi-word keywords (`from here on, no
undo`) are hard to parse, format and generate. It is the most obnoxious in the brief's sense:
charming once, tiring by the tenth file. Keep: `after` as the time-arm word, `undo` as the word,
`otherwise`-style `else` on waits.

### P5 Chart: agent token cost and tooling first

Axis: easiest for an LLM to generate correctly and for tools to analyze and draw. A workflow is a
flat table of named nodes. Each node is one line: name, expression, and optional clauses. Order comes
from data (a node runs when the names it uses are done) and from `after`; parallelism is implicit.
No nesting, so constrained decoding needs a tiny grammar, edits are line-local, and the graph is the
text.

New forms: `NAME: e` nodes, `after names`, `at time`, `when c`, `every d for d`, `undo e`,
`retry n`, `on name within d` (message node), `limit n` on a mapped node, `=> e` result,
`=> again f(args)`.

T1:

```
workflow onboard(user: UserId) -> Done fail OnboardError uses crm, billing, mail, timer =
  account: crm.create_account(user)?
  trial:   billing.start_trial(account, 14d)?
  welcome: mail.send(t"Welcome, {user}")?            after account
  call:    crm.schedule_call(account)?
  nudge:   mail.send(t"Your trial ends tomorrow")?   at trial.at + 13d
  paid:    billing.converted(account)?               after nudge, every 1h for 2d
  down:    billing.downgrade(account)?               when not paid
  => Done                                            after welcome, call, down
```

T2:

```
workflow order(o: Order) -> Shipped fail OrderError uses stock, pay, ship, mail =
  hold:  stock.reserve(o.items)?                     undo stock.release(hold)
  paid:  pay.charge(o.card, o.total)?                after hold, undo pay.refund(paid)
  label: ship.book(o.address, o.items)?              after paid, commit
  note:  mail.send(t"Order {o.id} ships: {label.tracking}")?
  => Shipped(label)                                  after note
```

T3:

```
workflow expense(e: Expense) -> Verdict fail ExpenseError uses people, mail, timer
    inbox decide: Verdict =
  first:  on decide within 48h
  boss:   people.manager_of(e.owner)?                when first.is_none()
  esc:    mail.send(boss, t"Expense {e.id} waits for you")?
  second: on decide within 24h                       after esc
  => first ?? second ?? Verdict.rejected("no answer in 72h")
```

T4:

```
workflow digest(docs: List[DocId]) -> Digest fail DigestError uses store, model, review, timer
    inbox reviewed: List[Fix] =
  rs:    try extract(d) for d in docs                limit 8
  sum:   model.generate(summarize(rs.oks()))?        retry 3
  open:  review.open(rs.errs())?                     when not rs.errs().is_empty()
  fixes: on reviewed within 3d                       after open
  => Digest(sum, fixes: fixes else [])

workflow extract(d: DocId) -> Facts fail DigestError uses store, model =
  text: store.read(d)?
  raw:  model.generate(extract_prompt(text))?        retry 3
  => parse_facts(raw) else fail DigestError.bad(d)
```

T5:

```
workflow subscription(s: Sub, n: int) -> Ended fail BillingError uses pay, mail, timer
    inbox cancel: unit =
  stop:   on cancel before s.next_bill(n)
  r:      pay.charge(s.card, s.price)?               when stop.is_none(), retry 5
  sent:   mail.send(s.owner, receipt(r))?            -- added in the later deploy
  => if stop.is_some() then Ended(cycles: n) else again subscription(s, n + 1)
```

Verdict: excellent for tools (the DAG is explicit, every edit is one line) and probably for agents,
but implicit parallelism from data dependencies hides the concurrency fact unless the reader traces
names, which strains law 4; `when` guards make every downstream node an Option and spread `??`
through the result; T3 and T5 show that deferred choice and loops are awkward as a table, which is
the known weakness of every DAG orchestrator (Airflow, Step Functions). Keep: one step per line in
the canonical printing, the graph as a derived view, `every d` polling as a clause.

## 5. Experiment: size and change cost

Measured over each proposal's five samples (including helper workflows, excluding comments and the
mix's `@rolling` variant and test). The plan was `o200k_base` BPE tokens as a public proxy, but the
container's network policy blocks the vocabulary download, so the token column is **lexical tokens**:
each run of letters, each run of digits and each other non-space character counts one. BPE counts
track this closely for code but weigh long words less; treat ratios, not absolute values, as the
result. New words counts reserved or contextual words the proposal adds to Yan, plus new operators.

| Proposal | Lines | Non-space chars | Lexical tokens | New words + operators |
| --- | --- | --- | --- | --- |
| P1 Terse | 45 | 1540 | 556 | 3 + 4 (`~`, `!`, `<-`, `\|`) |
| P2 Ledger | 68 | 2433 | 830 | 18 + 0 |
| P3 Score | 49 | 1674 | 600 | 13 + 1 (`<~`) |
| P4 Story | 55 | 1595 | 542 | 16 + 0 |
| P5 Chart | 39 | 1773 | 630 | 13 + 1 (`=>`) |
| Mix | 61 | 1749 | 630 | 7 + 10 contextual |

Reading the table: Terse and Story are the smallest by tokens (Story's words are long but few, so a
BPE count would likely put it above Terse); Ledger is 1.5 times the others, mostly from `step NAME:`
and `using`; Chart has the fewest lines but long ones. The mix costs about 13 percent more tokens
than Terse, almost all from `let` (kept for rule 2) and from wrapping headers onto a second line,
and stays a quarter smaller than Ledger. The T5 change (add a
receipt) is one added line in every proposal, but in Ledger it is three lines, because a rolling
workflow needs a `version` block. Under the hash pinning of the mix (section 6.2), it is one line
again, because a fresh journal starts on the new code at `again`.

## 6. The mix

### 6.1 What came from where

| Feature | Winner | Why |
| --- | --- | --- |
| Implicit steps: every journaled call is a step, the binding name is its journal key | P1, with P2's name keying | E725 already proves every capability call in a workflow is journaled; a `step` keyword marks nothing the checker does not know. Name keys make reordering safe |
| `let` and all statements plain Yan | rule 2 of the mock shape | One language to learn; Terse's bare `x = e` buys little |
| `wait d`, `wait until t` | RFC 0002, P4 | Reads aloud; durations and instants are already literals |
| `first` race with arms: `name(p)`, `after d`, `at t`, `until c every d` | P2's `select`, P4's words | One form for deferred choice; arms read as cases; nesting stays shallow |
| `inbox name: T` in the header | P1, P5 | Messages are capabilities in all but name; the header is where Yan puts what a function may touch |
| `pub var` as queryable state | P1, P3's `on query` | Read-only from outside; mutation still marked by `var`; no handler code |
| `undo e` clause, `commit` statement | P1, P2, P4 | A durable `errdefer`: next to the acquisition, reverse order, stops at `commit` (the saga pivot) |
| `retry n [backoff a..b]`, `within d` clauses | P2 | Policies stay visible per call without `policy`/`using` ceremony; a std default applies when absent |
| `again f(args)` | P1, P3 | Continue-as-new as a tail call; also the natural upgrade point |
| `start w(args)` for a child run, plain call for inline | P2's `child` | Inline calls share the parent's journal; `start` gives the child its own journal and returns a handle |
| Hash-pinned runs by default, `@rolling` plus `version` blocks to opt out | new | Removes the top user complaint for most workflows |
| Graph and timeline as `yan view` outputs, not syntax | P5 | One printing (law 8); tools draw the DAG from the tree |
| Mutating signal handlers, `finish`, implicit data-driven parallelism | rejected (P3, P5) | They weaken law 6 or hide the concurrency fact |

### 6.2 Frameworks

These are the ideas worth keeping even if every keyword changes.

1. **The journal is a type.** A workflow is an `@durable` function whose capabilities all journal;
   the checker proves replay safety before deploy. Steps are not an API the user calls, they are
   what capability calls are. This is the inversion of Restate, Inngest and Lambda's step APIs, and
   it is only possible because Yan's effects are typed.
2. **Code identity is the version.** A run records the item hash of its workflow at start and resumes
   on that hash. The toolchain keeps every hash that has a live run reachable (a package keeps old
   items until their last run ends; `yan durable gc` drops them). New code takes effect for new runs
   and at `again`. A workflow that must change mid-run opts into `@rolling`, uses `version` blocks
   (R453 `durable.version`), and must pass `yan replay --against REV`. Most workflows never see a
   version marker.
3. **Compensation is a durable `errdefer`.** `undo e` registers `e` with the current scope; a failure
   that leaves the scope before `commit` runs the pending undos in reverse order (parallel ones in
   parallel, after compensating CSP); success or `commit` drops them. Undo expressions must be
   infallible after retries, or the run parks for an operator.
4. **Every wait is a race, and every race is journaled.** `wait d` is `first` with one arm. The
   outcome of a `first` is a journal entry, so it replays exactly, and the static view of a workflow
   lists every point where outside events can steer it.
5. **One text, many views.** The canonical printing puts one journaled call per line, so a run's
   timeline lines up with source lines, and `yan view --as graph` draws the DAG (Mermaid or BPMN
   export) from the same tree.
6. **State is visible, not reachable.** `pub var` fields are journaled cells that outside callers can
   read but not write; writes from outside arrive only as inbox messages the body chooses to receive.

### 6.3 The five tasks in the mix

T1:

```
workflow onboard(user: UserId) -> Done fail OnboardError
    uses crm, billing, mail, timer =
  let account = crm.create_account(user)?
  billing.start_trial(account, 14d)?
  par
    mail.send(t"Welcome, {user}")?
    crm.schedule_call(account)?
  wait 13d
  mail.send(t"Your trial ends tomorrow")?
  first
    until billing.converted(account)? every 1h -> Done
    after 2d ->
      billing.downgrade(account)?
      Done
```

T2:

```
workflow order(o: Order) -> Shipped fail OrderError uses stock, pay, ship, mail =
  let hold = stock.reserve(o.items)? undo stock.release(hold)
  let paid = pay.charge(o.card, o.total)? retry 3 undo pay.refund(paid)
  let label = ship.book(o.address, o.items)? within 30s
  commit
  mail.send(t"Order {o.id} ships: {label.tracking}")? retry 10 backoff 1m..1h
  Shipped(label)
```

T3:

```
workflow expense(e: Expense) -> Verdict fail ExpenseError
    uses people, mail, timer
    inbox decide: Verdict =
  pub var stage = Stage.waiting
  let v = first
    decide(v) -> v
    after 48h ->
      stage = Stage.escalated
      let boss = people.manager_of(e.owner)?
      mail.send(boss, t"Expense {e.id} waits for you")?
      first
        decide(v) -> v
        after 24h -> Verdict.rejected("no answer in 72h")
  stage = Stage.done
  v
```

T4:

```
workflow digest(docs: List[DocId]) -> Digest fail DigestError
    uses store, model, review, timer
    inbox reviewed: List[Fix] =
  let rs = par for d in docs limit: 8
    try extract(d)
  let sum = model.generate(summarize(rs.oks()))? retry 3 within 2m
  if rs.errs().is_empty()
    return Digest(sum, fixes: [])
  review.open(rs.errs())?
  first
    reviewed(fixes) -> Digest(sum, fixes:)
    after 3d -> fail DigestError.no_review

workflow extract(d: DocId) -> Facts fail DigestError uses store, model =
  let text = store.read(d)?
  let raw = model.generate(extract_prompt(text))? retry 3
  parse_facts(raw) else fail DigestError.bad(d)
```

`extract` is called inline, so its steps land in the parent's journal under the path
`rs[i].text`, `rs[i].raw`. Writing `start extract(d)` would give each document its own run.

T5:

```
workflow subscription(s: Sub, n: int) -> Ended fail BillingError
    uses pay, mail, timer
    inbox cancel: unit =
  first
    cancel -> Ended(cycles: n)
    at s.next_bill(n) ->
      let r = pay.charge(s.card, s.price)? retry 5 backoff 1h..1d
      mail.send(s.owner, receipt(r))?        -- added in the later deploy
      again subscription(s, n + 1)
```

The added line needs no `version` block: runs pinned to the old hash finish their current cycle on
old code and pick up the new code at `again`. Under `@rolling` the same change is written:

```
@rolling
workflow subscription(s: Sub, n: int) -> Ended fail BillingError
    uses pay, mail, timer
    inbox cancel: unit =
  first
    cancel -> Ended(cycles: n)
    at s.next_bill(n) ->
      let r = pay.charge(s.card, s.price)? retry 5 backoff 1h..1d
      version "2026-12-receipts"
        mail.send(s.owner, receipt(r))?
      again subscription(s, n + 1)
```

A test, with simulated time and fake handlers (`std.test` style; names illustrative):

```
test_expense_escalates() uses none =
  let run = durable.sim(expense(sample_expense()), people: fake_people, mail: outbox())
  run.advance(48h)
  assert run.view.stage == Stage.escalated
  run.send(decide, Verdict.approved)
  assert run.result == Ok(Verdict.approved)
```

### 6.4 Mock grammar

Everything not listed is plain Yan (15.2). Indentation and lexis as 15.1.

```
workflow   = [attrs] "workflow" NAME "(" [params] ")" "->" type ["fail" type]
             "uses" caps {inbox} "=" NEWLINE INDENT {wstmt} DEDENT
inbox      = NEWLINE INDENT? "inbox" NAME ":" type {"," NAME ":" type}
wstmt      = yan-stmt [clauses]                 -- a journaled call gains clauses
           | "wait" duration-expr
           | "wait" "until" instant-expr
           | first
           | "commit"
           | "again" call
           | "version" STRING NEWLINE INDENT {wstmt} DEDENT     -- only under @rolling
           | "pub" "var" NAME "=" expr
clauses    = {"retry" INT ["backoff" expr ".." expr] | "within" expr | "undo" expr}
first      = "first" NEWLINE INDENT arm {arm} DEDENT
arm        = arm-head "->" (expr | NEWLINE INDENT {wstmt} DEDENT)
arm-head   = NAME ["(" pattern ")"]            -- an inbox message
           | "after" expr                      -- a Duration
           | "at" expr                         -- an Instant
           | "until" expr "every" expr         -- poll a journaled call
           | "done" NAME ["(" pattern ")"]     -- a child run started with start
call-expr  = ... | "start" call                -- child run; returns Run[T]
```

`first` is also an expression (T3). Clause order is free but printed as `retry`, `backoff`,
`within`, `undo`.

### 6.5 Lowering sketch

| Form | Lowers to |
| --- | --- |
| `workflow` | `@durable` function (R416); the header's inbox list becomes an extra journaling capability parameter |
| journaled call with binding `x` | the call as an R305 entry whose key is the step path (`x`, or callee name plus index when unbound) instead of the bare op index |
| `retry n backoff a..b` | a std retry loop around the call; each attempt is a journal entry; the R453 idempotency key is shared across attempts |
| `within d` | `with timeout(d)` (R472), so `Timeout` joins the step's failure type |
| `undo e`, `commit` | a scope-local stack of journaled closures; on failure exit, pop and run in reverse; `commit` clears it |
| `wait d`, `wait until t`, `after d`, `at t` | `timer.sleep(d)`, `timer.sleep_until(t)` (R552) |
| `first` | `std.chan.select` over the inbox, timers and pollers; the chosen arm is journaled. Open question 1 asks whether this needs `uses nondet` |
| `until c every d` | a journaled poll loop with the given period |
| `pub var` | a journaled cell plus a generated read-only query endpoint |
| `again f(a)` | `durable.restart(f, a)`: end this journal, carry unread inbox messages, start a new journal on the newest hash of `f` |
| `start w(a)` | a child run with its own journal; `Run[T]` handle; the start is a parent journal entry |
| pinning | the run's journal header records the root item hash (R305 header already has root item hashes); resume loads that hash |

### 6.6 Diagnostics the surface would need (no codes)

- An error for a `step` on a handler without idempotency support (RFC 0002 already promotes W850 to
  an error in this surface).
- An error for `undo` on a call whose binding is unused by the undo expression (usually a bug).
- An error for `commit` outside any scope with pending undos.
- An error for `version` without `@rolling`, and a warning for `@rolling` without a `yan replay`
  check in CI.
- A warning for a `first` with no time arm (a run that can wait forever).
- A note on `again` inside `par` (it must be in tail position of the workflow body).

## 7. Open questions

1. **Does a journaled race need `nondet`?** `select` requires `uses nondet` in Yan (9.5). In a
   workflow the outcome is journaled, so replay is exact. Either the durable journal discharges
   `nondet` for `first`, or every workflow with a `first` lists `nondet`. The first is cleaner; it
   needs a rule in 7.7.
2. **Name-keyed journal entries** change the R305 entry key from op index to a path. That is a core
   change to the trace format, not only a surface one. Is it worth it for `@durable` in Yan too?
3. **Keeping old hashes alive.** Hash pinning needs the package and deploy story (RFC 0020) to retain
   items with live runs. What is the retention unit, and who runs `gc`?
4. **Undo failures.** Park the run, panic, or let undo expressions have their own failure type
   handled by an `undo ... else` clause?
5. **Queries beyond `pub var`.** Is a derived query (a pure function of the `pub var` cells) needed,
   or is that a client-side concern?
6. **Updates** (a message that returns a value to the sender) are absent. Model them as an inbox of
   `(T, Reply[U])` or add an arm form?
7. **Choreographies.** Signals between two workflows are a protocol; session types (R634) could type
   them. Worth a later study with `@yan/choreo`.
8. **Agent generation.** The Plan A harness (R567) could add `.ywf` tasks to measure whether implicit
   steps and `first` are as easy for models as the Chart table looked by size.
9. **Name.** `first` competes with `race`, `select` and `when`. `first` was chosen because its result
   is the first arm to fire and it does not collide with Yan's `match` or `when`.

## 8. Sources

- Temporal versioning docs: https://docs.temporal.io/develop/go/versioning
- Temporal community, "Versioning difficulty": https://community.temporal.io/t/versioning-difficulty/3016
- "One line, one outage: why Temporal's replay broke production":
  https://hashnode.com/posts/temporal-replay-broke-production-workflow-getversion/69f91ce446610fd60636e7b0
- AWS, "Lambda announces durable functions":
  https://aws.amazon.com/about-aws/whats-new/2025/12/lambda-durable-multi-step-applications-ai-workflows/
- InfoQ, AWS Lambda durable functions: https://infoq.com/news/2025/12/aws-lambda-durable-functions/
- Vercel Workflow DevKit: https://useworkflow.dev/docs/getting-started/vite
- Serverless Workflow 1.0.0 release: https://serverlessworkflow.netlify.app/blog/releases/release-100
- ZenML, "Where durable execution is headed": https://www.zenml.io/blog/where-durable-execution-is-headed
- Khan, "Resume means resume", arXiv 2608.03836: https://arxiv.org/pdf/2608.03836
- Papers in section 2.3 are cited by title, venue and year.
