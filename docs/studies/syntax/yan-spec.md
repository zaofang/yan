# Yan Spec 言规 (`.ysp`): syntax and semantics study

Status: brainstorm study, 2026-11-04. Not a spec change. Every grammar and example here is **MOCK**:
non-normative, no row numbers, no diagnostic codes. A form becomes real only through an RFC and
accepted rows (`CLAUDE.md`). The only normative text today is spec 16.7 row R548 ("specs checked
against actors by trace refinement", lowers to `std.sim` models and replay checks, research track)
and the one example in RFC 0002 D13.

## Summary

- The market has moved from "write a spec, check it, forget it" to "the spec is an oracle for the
  running code". Trace validation (TLA+ 2024, CCF 2025, OmniLink 2026) and runtime monitors (AWS P
  and PObserve) are where the value is. Yan already has the hard parts in core: deterministic
  `std.sim`, `explore` with DPOR, record and replay, Lamport-stamped traces, `check_linearizable`.
- LLMs write syntactically valid TLA+ almost always, but semantically correct TLA+ rarely (best 8.6%
  on one 2026 benchmark; about 46% conformance on real systems in another). The bottleneck is
  modeling fidelity, not syntax. So the surface should make fidelity checkable: trace examples,
  reachability witnesses, per-step conformance and link coverage.
- Five proposals, one per axis: **Prime** (brevity), **Plain** (learnability), **Ledger** (agent
  tokens and tooling), **Proof** (explicitness and safety), **Score** (expressiveness, scenario
  based).
- The mix is a framework of five nouns, **state, steps, laws, worlds, links**, and one promise: one
  spec, four uses (check, simulate, monitor, generate). Steps are Yan statements run atomically,
  frames are inferred, laws are a closed set of property patterns, bounds live in worlds outside the
  spec, and links map an actor or a type onto the spec with a coverage report.
- Biggest open question for Antonio: should `.ysp` laws be protected targets like tests (agents
  propose, the lead approves)? This study recommends yes.

## 1. Methodology and process

1. **Read narrowly in the repo.** `docs/surfaces.md` section 17 (the RFC example and the current
   MOCK grammar), RFC 0002 D13 (idea, limits, prior art), spec 16.7 (admission rules N1 to N9, row
   R548), spec 8.4 to 8.7 (contracts, decidable fragment, prove mode, ghost code and lemmas), spec 6
   (the `explore` DPOR checker), spec 9 (reproducible actors, R264; `cluster` sim and
   `check_linearizable`, R417), spec 17.7 (`std.sim`), spec 18 (property tests, choice-sequence
   fuzzing, schedule exploration, protected targets). Plain Yan lexis from `docs/surfaces.md`
   section 3 and the shared mock rules.
2. **Searched** for trace validation, recent industrial reports, current spec languages and LLM
   results on spec writing (sources in section 2.4). Each claim below carries a confidence mark:
   **[H]** read this session from the source or its abstract, **[M]** well-known published work
   recalled and consistent with what was read, **[L]** recollection not re-checked.
3. **Fixed four tasks** that every proposal must express, chosen to cover the job of R548 and the
   classic failure points of spec languages:
   - **T1 Lease.** The RFC example: one holder at a time, with time.
   - **T2 Two-phase commit.** Several nodes, nondeterministic votes, a coordinator crash fault, a
     safety law, a bounded liveness law and a reachability check.
   - **T3 Bounded FIFO.** A sequential model with return values, used as the oracle for a concurrent
     queue (linearizability).
   - **T4 Link.** The lease spec refined by an actor: state mapping, message to step mapping, ignored
     messages.
4. **Wrote five proposals**, each optimizing one axis hard and accepting the cost on others.
5. **Experimented.** Counted characters and lexical tokens of each proposal's four samples with a
   small script (section 5.1), and reviewed each against the failure modes found in research (section
   2.3).
6. **Mixed.** Took the best mechanism per concern, checked it against N1 to N9 and the five facts,
   and wrote one consolidated MOCK grammar and the tasks again in it.

## 2. Findings

### 2.1 Peers and competitors

| Tool | Shape | What works | What does not |
| --- | --- | --- | --- |
| TLA+ with TLC and Apalache | math: sets, functions, primed variables, temporal logic | most expressive; industrial record (AWS, Azure, MongoDB) [M]; refinement is native | notation is the top adoption barrier; `UNCHANGED` boilerplate; untyped, errors late [M] |
| PlusCal | algorithm language translated to TLA+ | reads like pseudocode [M] | two languages to debug; labels as atomicity are subtle [M] |
| Quint | TLA+ semantics, typed, programmer syntax | types, REPL, simulator, tests in the same file [M] | still a separate world from the code |
| P (AWS) | communicating state machines | code-shaped; systematic testing; PObserve checks production logs against P specs [M] | models are big; liveness via "hot states" is unusual [M] |
| FizzBee | Python-like (Starlark) specs, atomic and serial blocks, `any`, `oneof` | very low entry cost; adopted by practitioners writing Kafka and lease specs in 2024 and 2025 [M] | dynamic typing; limited refinement story [L] |
| Alloy 6 | relational logic, now with temporal operators | small-scope hypothesis, instant counterexamples, great for structure [M] | weak on numbers and time; no link to code |
| Event-B (Rodin) | refinement chains with proof obligations | explicit refinement and gluing invariants [M] | heavy; proof effort dominates |
| Ivy | decidable fragment (EPR) for inductive invariants | proofs that finish; user guides invariant search [M] | the fragment is restrictive |
| Promela and SPIN | C-like processes, channels | mature LTL checking [M] | dated; weak data types |
| Stateright (Rust) | specs as Rust trait impls | same language as code, actor model built in [M] | verbose; spec and code blur |
| Hypothesis stateful, quickcheck-state-machine | model-based testing in the host language | runs in CI, shrinks counterexamples [M] | bounded random search only |
| Antithesis, FoundationDB simulation | deterministic simulation of the real binary | finds real bugs in the real code [M] | properties are ad hoc assertions |

Yan's position is unusual: it owns the simulator (`std.sim`), the schedule checker (`explore`), the
trace format and the code. No peer has a spec language in the same toolchain as a deterministic
runtime. That is the opening.

### 2.2 What users want (from industry reports)

- **Specs connected to the code.** The recurring complaint since Newcombe et al. (CACM 2015) is the
  gap between a checked spec and the code that drifts from it [M]. AWS's 2025 report describes P,
  PObserve on production logs, deterministic simulation and lightweight methods as the practices
  that stuck [M; the article page did not load this session].
- **Trace validation.** Cirstea, Kuppe, Merz et al. (2024) validate traces of distributed programs
  against TLA+ specs by treating the trace as a constraint on the spec's behaviors [H]. The CCF
  team's "smart casual verification" (NSDI 2025) combines model checking with trace validation of
  the real implementation and reports it found bugs before production [H]. OmniLink (2026) validates
  unmodified concurrent systems by solving for a total order of black-box events consistent with the
  spec, helped improve WiredTiger's TLA+ model and found two previously unknown bugs, in a lock-free data structure and a concurrent queue [H].
- **Familiar syntax.** Quint, FizzBee and P all bet that programmer syntax widens the audience; the
  bet is paying off in blog-level adoption [M]. Hillel Wayne's essays argue the barrier is less the
  math than the lack of a path from spec to everyday work [M].
- **Fast feedback.** Small-scope checking (Alloy) and seeded simulation (Quint, FizzBee) beat
  exhaustive runs for daily use [M].

### 2.3 Research on what goes wrong

- **LLM spec writing.** A 2026 evaluation of 30 models on 205 natural-language-to-TLA+ tasks found
  the best semantic correctness was 8.6%, and size did not predict quality [H]. The Specula team
  (SIGOPS 2026) found generated specs nearly always parse, but average conformance to the real system
  was about 46% and invariant score about 41%. Errors were textbook modeling: specs admit states the
  system never reaches, or merge several code steps into one guard that blocks reachable states [H].
  Their lessons: evaluate per action, model data as the code does, decompose actions the way the
  implementation does, make abstractions explicit, check conformance on traces and watch coverage
  [H].
- **Vacuity.** A spec whose guards are too strong passes every safety law. Classic model checking
  handles this with vacuity detection and reachability goals [M].
- **Property patterns.** Dwyer, Avrunin and Corbett (ICSE 1999) found that most real temporal
  properties fit a few patterns (absence, universality, existence, response, precedence) with a few
  scopes [M]. A closed set of patterns covers most needs without exposing raw LTL.
- **Finite traces.** LTL on finite traces (LTLf, De Giacomo and Vardi 2013) fits bounded exploration
  and replayed logs better than infinite-trace LTL [M]. Bounded response ("within 5s") turns liveness
  into safety, which matches R548's "no liveness proofs" limit.
- **Scenario-based specs.** Live sequence charts (Damm and Harel 2001) and behavioral programming
  (Harel, Marron, Weiss 2012) specify by composing scenarios that request, wait for and block events
  [M]. Expressive and modular, but unfamiliar.
- **Frames.** TLA+'s `UNCHANGED` and Event-B's explicit frames are a source of errors; languages with
  value semantics infer frames for free [M]. Yan's mutable value semantics (spec 8.7: "no heap model
  and no frame conditions") gives this directly.

### 2.4 Sources

- Cirstea, Kuppe, Merz et al., Validating Traces of Distributed Programs Against TLA+ Specifications,
  2024: https://arxiv.org/abs/2404.16075v2
- Howard et al., Smart Casual Verification of the Confidential Consortium Framework, NSDI 2025:
  https://www.usenix.org/conference/nsdi25/presentation/howard
- OmniLink, Trace Validation of Unmodified Concurrent Systems, 2026:
  https://arxiv.deeppaper.ai/papers/2601.11836v1
- Brooker and Desai, Systems Correctness Practices at Amazon Web Services, CACM 2025:
  https://cacm.acm.org/practice/systems-correctness-practices-at-amazon-web-services
- P language: https://github.com/p-org/P
- FizzBee: https://fizzbee.io/tutorials/getting-started ; Vanlightly, Kafka transactions in FizzBee:
  https://jack-vanlightly.com/analyses/2024/12/5/verifying-kafka-transactions-diary-entry-4-writing-an-initial-fizzbee-spec
  ; Hochstein, leases and fencing tokens in FizzBee:
  https://surfingcomplexity.blog/2025/03/03/locks-leases-fencing-tokens-fizzbee/
- Quint: https://github.com/informalsystems/quint
- Can LLMs Write Correct TLA+ Specifications?, 2026: https://arxiv.org/abs/2606.05792 (summary:
  https://ai4fm.cs.luc.edu/posts/llm-tla-evaluation-2025/)
- Cheng et al. (Specula), Can LLMs model real-world systems in TLA+?, SIGOPS 2026:
  https://sigops.org/2026/can-llms-model-real-world-systems-in-tla
- Recalled, not re-fetched: Lamport, Specifying Systems (2002); Newcombe et al., CACM 2015; Davis,
  Schvimer, Kuppe, eXtreme Modelling in Practice, VLDB 2020; Jackson, Software Abstractions (Alloy);
  Abrial, Modeling in Event-B (2010); Padon et al., Ivy, PLDI 2016; Dwyer et al., ICSE 1999; De
  Giacomo and Vardi, IJCAI 2013; Damm and Harel, LSCs, 2001; Harel et al., Behavioral Programming,
  CACM 2012.

### 2.5 Design constraints taken from the repo

- N1: each form either lowers to Yan Core or has its own semantics behind a boundary signature. R548
  says the spec lowers to `std.sim` models and replay checks.
- N2 to N9: decidable subset, one printing, same item hash, five facts visible, own crate, round
  trip, per-surface names and diagnostics, Yan lexis.
- Shared mock rules: Yan lexis, plain Yan statements and expressions inside items, one or two item
  keywords, `ex` lines allowed.
- Spec items never perform effects: they are `uses none`. Time, faults and nondeterminism come from
  the checker, not from capabilities.

## 3. The five proposals

Each proposal writes T1 to T4. The 2PC model is the same in all five: each node votes once (Yes or
No), the coordinator decides Commit only if all voted Yes, each node learns the decision, and the
coordinator may crash and recover.

### 3.1 Proposal A, Prime: brevity and math density

Axis: fewest characters per idea. TLA+ heritage: primed next-state variables, one-line steps,
quantifier one-liners, `[]` and `~>`. Unchanged variables are inferred.

T1:

```
spec Lease
  holder: Node? = None; expires: Instant = Instant.min
  acquire(n: Node): holder == None or now >= expires -> holder' = n, expires' = now + 10s
  release(n: Node): holder == n -> holder' = None
  [] one_holder: holder == None or now < expires + 1s
```

T2:

```
spec TwoPhase
  vote: Map[Node, Vote?] = none; decision: Decision? = None
  done: Map[Node, Decision?] = none; up = true
  cast(n, v: Vote): vote[n] == None -> vote'[n] = v
  decide: up and decision == None and all n: vote[n] != None
    -> decision' = if all n: vote[n] == Yes then Commit else Abort
  learn(n): up and decision != None and done[n] == None -> done'[n] = decision
  crash: up -> up' = false
  recover: not up -> up' = true
  [] agreement: not (some n, m: done[n] == Commit and done[m] == Abort)
  [] validity: all n: done[n] == Commit => all m: vote[m] == Yes
  decision != None ~> all n: done[n] != None
  fair recover, learn
```

T3:

```
spec Fifo[T](cap: int)
  items: List[T] = []
  push(x: T): items.len() < cap -> items' = items + [x]
  pop() -> T: items != [] -> items' = items.drop(1), result = items[0]
```

T4:

```
refines Lease by actor LeaseServer
  holder = state.owner; expires = state.deadline
  Acquire(n) => acquire(n); Release(n) => release(n); Ping => skip
```

Strengths: shortest by far; familiar to TLA+ users; simultaneous assignment makes a step a relation.
Costs: primes and `;` packing are a second notation next to Yan; long guards wrap badly; the
simultaneous semantics differ from Yan's sequential statements, so code and spec read differently.
`~>` with `fair` is unbounded liveness, which R548 cannot check.

### 3.2 Proposal B, Plain: learnability and code shape

Axis: a Yan programmer reads it on day one. FizzBee and P style: steps are Yan blocks run
atomically and sequentially, `require` is a guard, `oneof` is a nondeterministic branch, roles group
per-node state the way the implementation does (the Specula lesson).

T1:

```
spec Lease
  var holder: Node? = None
  var expires = Instant.min

  step acquire(n: Node)
    require holder == None or now >= expires
    holder = n
    expires = now + 10s

  step release(n: Node)
    require holder == n
    holder = None

  always one_holder
    holder == None or now < expires + 1s
```

T2:

```
spec TwoPhase
  role Rm(n: Node)
    var vote: Vote? = None
    var done: Decision? = None

    step cast
      require vote == None
      oneof
        vote = Yes
        vote = No

    step learn
      require Tm.up and Tm.decision != None and done == None
      done = Tm.decision

  role Tm
    var decision: Decision? = None
    var up = true

    step decide
      require up and decision == None and Rm.all(r -> r.vote != None)
      decision = if Rm.all(r -> r.vote == Yes) then Commit else Abort

    fault step crash
      require up
      up = false

    step recover
      require not up
      up = true

  always agreement
    not Rm.any(r -> r.done == Commit) or not Rm.any(r -> r.done == Abort)

  always validity
    not Rm.any(r -> r.done == Commit) or Rm.all(r -> r.vote == Yes)

  eventually all_done within 12 steps
    Rm.all(r -> r.done != None)
```

T3:

```
spec Fifo[T](cap: int)
  var items: List[T] = []

  step push(x: T)
    require items.len() < cap
    items.push(x)

  step pop() -> T
    require items.len() > 0
    items.remove(0)
```

T4:

```
refines Lease with actor LeaseServer
  map holder = state.owner
  map expires = state.deadline
  on Acquire(n) do acquire(n)
  on Release(n) do release(n)
  on Ping do nothing
```

Strengths: zero new notation for state changes; roles mirror code; `oneof` reads well. Costs: the
longest; `require` inside the body hides the guard from the header, so diagnostics and tables cannot
show it without reading the body; `role` and `Rm.all` invent a second module system.

### 3.3 Proposal C, Ledger: agent token cost and tooling

Axis: smallest closed grammar, one form per concept, every line checkable on its own. Steps are
rows of a table with three columns (name and parameters, guard, effects). Laws are one line each.
Trace examples are first-class. Diffs touch one row; an agent's patch to one step cannot disturb
another; the checker reports per row, which is what Specula recommends for evaluation.

T1:

```
spec Lease
  state holder: Node? = None
  state expires: Instant = Instant.min
  step acquire(n: Node) | holder == None or now >= expires | holder = n; expires = now + 10s
  step release(n: Node) | holder == n | holder = None
  law always holder == None or now < expires + 1s
  ex ok acquire(a); release(a); acquire(b)
  ex bad acquire(a); acquire(b)
```

T2:

```
spec TwoPhase
  state vote: Map[Node, Vote?] = Map.fill(None)
  state decision: Decision? = None
  state done: Map[Node, Decision?] = Map.fill(None)
  state up: bool = true
  step cast(n: Node, v: Vote) | vote[n] == None | vote[n] = v
  step decide() | up and decision == None and vote.all(_ != None) | decision = if vote.all(_ == Yes) then Commit else Abort
  step learn(n: Node) | up and decision != None and done[n] == None | done[n] = decision
  fault crash() | up | up = false
  step recover() | not up | up = true
  law always not (done.any(_ == Commit) and done.any(_ == Abort))
  law always not done.any(_ == Commit) or vote.all(_ == Yes)
  law after decide() expect done.all(_ != None) within 12 steps
  law can done.all(_ == Commit)
  ex ok cast(a, Yes); cast(b, Yes); decide(); learn(a); learn(b)
```

T3:

```
spec Fifo[T](cap: int)
  state items: List[T] = []
  step push(x: T) | items.len() < cap | items.push(x)
  step pop() -> T | items.len() > 0 | items.remove(0)
```

T4:

```
link Lease actor LeaseServer
  state holder = state.owner
  state expires = state.deadline
  msg Acquire(n) | acquire(n)
  msg Release(n) | release(n)
  msg Ping | quiet
```

Strengths: about a dozen keywords; trivially parsed and grepped; one line per fact; examples catch
vacuity. Costs: long rows wrap and stop being a table; `|` collides with pattern alternation in
Yan; complex steps (loops, `match`) do not fit a row; it reads like a log, not like craft.

### 3.4 Proposal D, Proof: explicitness and safety

Axis: nothing implicit. Event-B and Ivy style: each step states what it reads and changes; laws say
whether they are checked by exploration or proved inductive; fairness and bounds are explicit;
refinement has an abstraction function and a gluing invariant. Laws in the 8.6 decidable fragment
can go to the solver under `@prove`, giving unbounded safety proofs where exploration is bounded.

T1:

```
spec Lease
  var holder: Node? = None
  var expires: Instant = Instant.min

  action acquire(n: Node)
    reads holder, expires, now
    changes holder, expires
    when holder == None or now >= expires
    then
      holder = n
      expires = now + 10s

  action release(n: Node)
    reads holder
    changes holder
    when holder == n
    then holder = None

  @prove inductive one_holder: holder == None or now < expires + 1s
```

T2:

```
spec TwoPhase
  var vote: Map[Node, Vote?] = Map.fill(None)
  var decision: Decision? = None
  var done: Map[Node, Decision?] = Map.fill(None)
  var up: bool = true

  action cast(n: Node, v: Vote)
    changes vote[n]
    when vote[n] == None
    then vote[n] = v

  action decide()
    changes decision
    when up and decision == None and vote.all(_ != None)
    then decision = if vote.all(_ == Yes) then Commit else Abort

  action learn(n: Node)
    changes done[n]
    when up and decision != None and done[n] == None
    then done[n] = decision

  fault action crash()
    changes up
    when up
    then up = false

  weak fair action recover()
    changes up
    when not up
    then up = true

  @prove inductive agreement: not (done.any(_ == Commit) and done.any(_ == Abort))
    strengthened by: all n: done[n] == None or done[n] == decision
  checked validity: not done.any(_ == Commit) or vote.all(_ == Yes)
  checked response: decision != None leads to done.all(_ != None) within 12 steps
    assuming weak fair learn, recover
```

T3:

```
spec Fifo[T](cap: int)
  var items: List[T] = []

  action push(x: T)
    changes items
    when items.len() < cap
    then items.push(x)

  action pop() -> T
    changes items
    when items.len() > 0
    then items.remove(0)

  @prove inductive bounded: items.len() <= cap
```

T4:

```
refinement LeaseServer refines Lease
  abstraction
    holder = state.owner
    expires = state.deadline
  gluing invariant state.owner == None or state.deadline != Instant.min
  message Acquire(n) implements acquire(n)
  message Release(n) implements release(n)
  message Ping implements stutter
  coverage required: acquire, release
```

Strengths: reviewers see every assumption; proofs reach beyond bounds; coverage is a stated
requirement. Costs: `reads` and `changes` duplicate what the compiler can infer from value
semantics and can go stale; three levels of law (`checked`, `inductive`, `@prove`) is a lot to
learn; the verbosity punishes small specs, which are most specs.

### 3.5 Proposal E, Score: expressiveness through scenarios

Axis: say what matters in the vocabulary of events, not states. Behavioral programming: a spec is a
set of threads, each a short scenario that **requests**, **waits for** or **blocks** events. The
behaviors are all event sequences the threads jointly allow. Laws are trace patterns (finite-trace
regular expressions with bounded time). State can still exist, but many specs need little.

T1:

```
spec Lease
  event acquire(n: Node), release(n: Node)

  thread grant
    loop
      wait acquire(n)
      block acquire(_) until release(n) or 10s

  law never acquire(a) then acquire(b) within 10s unless release(a) between
```

T2:

```
spec TwoPhase
  event cast(n: Node, v: Vote), decide(d: Decision), learn(n: Node, d: Decision)
  event crash(), recover()

  thread voter(n: Node)
    request one of cast(n, Yes), cast(n, No)

  thread coordinator
    wait cast(n, _) for every n
    request decide(if every cast(_, Yes) then Commit else Abort)

  thread outage
    loop
      wait crash()
      block decide(_), learn(_, _) until recover()

  thread learner(n: Node)
    wait decide(d)
    request learn(n, d)

  law never learn(_, Commit) and learn(_, Abort)
  law never learn(_, Commit) unless every cast(_, Yes) before
  law after decide(_) expect learn(n, _) for every n within 12 steps
  law can cast(a, Yes) then cast(b, Yes) then decide(Commit)
```

T3:

```
spec Fifo[T](cap: int)
  event push(x: T), pop() -> T
  var items: List[T] = []

  thread queue
    loop
      select
        on push(x) when items.len() < cap
          items.push(x)
        on pop() when items.len() > 0
          reply items.remove(0)
```

T4:

```
link Lease to actor LeaseServer
  on Acquire(n) emit acquire(n)
  on Release(n) emit release(n)
  on Ping emit nothing
```

Strengths: modular (add a thread, add a constraint); faults are just another thread; event laws match
how people state requirements ("after X, Y within Z"); links need no state mapping when the spec is
event-only, so they work on black-box logs (the OmniLink idea). Costs: unfamiliar; blocking
semantics surprise people; state invariants become awkward; a rich pattern algebra is a second
language to learn.

## 4. Comparison

### 4.1 Size experiment

Counted over the four samples of each proposal (fenced blocks only). Lexical tokens split on
identifiers, numbers, and runs of punctuation; a rough proxy for model tokens.

| Proposal | Non-blank lines | Characters (no spaces) | Lexical tokens |
| --- | --- | --- | --- |
| Prime | 25 | 964 | 358 |
| Plain | 56 | 1081 | 360 |
| Ledger | 33 | 1178 | 433 |
| Proof | 66 | 1577 | 477 |
| Score | 40 | 996 | 345 |
| Mix (section 5.4, worlds excluded) | 47 | 1233 | 449 |

Comment lines are excluded. The samples are not equal in content: the mix and Ledger carry `ex`
traces and `can` witnesses that Prime and Plain lack, and the mix adds a T3 link. Even so, by
token proxy Prime, Plain and Score cost the same, so brevity of notation buys little; what changes
cost is how many checks a spec carries. Proof costs the most: about a quarter more characters than
the mix while carrying fewer checks. Dropping the `step`, `var` and `require` words (Plain to mix)
saves two words per step. A real tokenizer count is a follow-up for the R567 harness.

### 4.2 Scores per concern

Scale 1 (poor) to 5 (strong), judged against the findings in section 2.

| Concern | Prime | Plain | Ledger | Proof | Score |
| --- | --- | --- | --- | --- | --- |
| Brevity | 5 | 2 | 4 | 1 | 3 |
| Day-one readability for a Yan user | 2 | 5 | 3 | 3 | 2 |
| Agent writing (closed grammar, local edits) | 3 | 3 | 5 | 3 | 2 |
| Fidelity checks (vacuity, examples, coverage) | 1 | 2 | 4 | 4 | 4 |
| Explicit assumptions (bounds, fairness, abstraction) | 2 | 2 | 2 | 5 | 3 |
| Temporal expressiveness | 3 | 2 | 3 | 3 | 5 |
| Fit with Yan lexis and statements | 2 | 5 | 3 | 4 | 3 |
| Fit with R548 (bounded, trace refinement) | 2 | 4 | 4 | 3 | 5 |

## 5. The mix

### 5.1 The framework: five nouns, four uses

A Yan Spec has five parts, each a noun the reader can point at:

| Noun | What it is | Taken from |
| --- | --- | --- |
| **state** | typed fields with initial values, written like record fields | Plain, Yan records |
| **steps** | named atomic transitions: header with parameters and a `when` guard, body of plain Yan statements | Plain body, Ledger header |
| **laws** | a closed set of property patterns: `always`, `never`, `after ... expect ... within`, `can`, plus `ex` traces | Score patterns, Dwyer et al. |
| **worlds** | bounds, symmetry, time and fault budgets, kept outside the spec so one spec runs in many worlds | Proof's explicitness, Alloy scopes |
| **links** | the map from an actor, a type or a log onto the spec, with coverage | Proof, Score, trace validation |

And the same spec has four uses, all run by tools Yan already specifies:

1. **Check**: bounded exhaustive exploration of the steps in a world (`explore`, DPOR).
2. **Simulate**: seeded random walks with swarm settings (`std.sim`), replayable by seed.
3. **Monitor**: the linked actor's recorded or live trace is checked step by step against the spec
   (trace refinement at message boundaries, R548). A failure names the message, the step it should
   have matched and the guard or state field that disagreed.
4. **Generate**: the spec drives the linked actor as a model-based test, choosing messages from
   enabled steps and comparing state after each one.

### 5.2 Opinions (the house style)

- **Steps are sequential Yan, applied atomically.** A step body is ordinary Yan statements on the
  state, as in FizzBee and P. No primes, no simultaneous assignment. Reason: one reading of `x = e`
  across code and spec; Specula's evidence that the dominant error is modeling steps unlike the code.
- **Frames are inferred, never written.** Mutable value semantics gives each step's write set; the
  canonical printer can show it on request (`yan view --frames`). The write and read sets also give
  the independence relation for partial-order reduction for free.
- **Guards live in the header.** `acquire(n: Node) when holder == None`. The guard is visible in
  outlines, diagnostics and coverage tables without reading the body.
- **No step keyword.** As in plain Yan, a name with parameters starts a step; a name with a type
  starts a state field. The spec header already says what the block is.
- **`now` is ambient.** A spec that reads `now` gets a clock; the world says how time advances. No
  hand-written `tick` step.
- **Nondeterminism is `pick`.** Step parameters are picked by the checker from their (bounded)
  types. Inside a body, `pick x in s` or `pick x: T` adds a choice point. `pick` lowers onto the same
  choice sequence that property tests and the fuzzer already read, so specs are fuzzable and their
  counterexamples shrink.
- **Laws are patterns, not formulas.** `always P`, `never P`, `after E expect P within D`, `can P`.
  `within` takes a duration (virtual time) or a step count. Unbounded liveness is out (R548). A
  state predicate or a step event can appear where a pattern expects one.
- **Every spec proves it is not vacuous.** Each step must fire at least once in some world, each
  `can` must be reached, each `ex` trace must replay, and each `ex!` trace must be rejected.
  Otherwise the check fails. This is the cheapest defense against over-strong guards.
- **Faults are steps marked `fault`.** Worlds budget them (`crash up to 1`). Specs stay readable;
  fault intensity is a world setting.
- **Bounds never live in the spec.** Sets of nodes, depth, time jumps and symmetry are world
  settings. A spec is a theory; a world is an experiment.
- **Links are total or say why.** Every message the actor handles maps to a step or to `quiet`
  (stutter: mapped state must not change). The monitor reports step coverage after each run.

### 5.3 MOCK grammar

```
file       = {spec | world | link | yan-item}
spec       = "spec" TYPENAME [tparams] ["(" params ")"] NEWLINE INDENT {member} DEDENT
member     = field | step | law | ex | yan-fn
field      = NAME [":" type] "=" expr NEWLINE
step       = ["fault"] NAME "(" [params] ")" ["->" type] ["when" expr] NEWLINE INDENT block DEDENT
block      = yan statements, plus: "pick" NAME ("in" expr | ":" type)
law        = "always" NAME ":" pred NEWLINE
           | "never" NAME ":" pred NEWLINE
           | "after" NAME ":" event "expect" pred "within" bound NEWLINE
           | "can" NAME ":" (pred | trace) NEWLINE
ex         = ("ex" | "ex!") trace NEWLINE
trace      = event {"," event} | event ".." event
event      = NAME "(" [pattern {"," pattern}] ")"
pred       = yan expression of type bool over state fields and now
bound      = DURATION | INT "steps"
world      = "world" NAME "for" TYPENAME NEWLINE INDENT {setting} DEDENT
setting    = TYPENAME "=" set-literal ["sym"] NEWLINE
           | "time" DURATION "to" DURATION NEWLINE
           | "depth" INT NEWLINE
           | NAME "up to" INT NEWLINE                -- fault budget
link       = "link" TYPENAME [targs] "to" ("actor" | "type" | "log") TYPENAME NEWLINE
             INDENT {NAME "=" expr NEWLINE} {route} DEDENT
route      = "on" pattern "->" (NAME "(" [args] ")" ["==" expr] | "quiet") NEWLINE
```

Keywords added to Yan: `spec`, `fault`, `when`, `pick`, `always`, `never`, `after`, `expect`,
`within`, `can`, `world`, `link`, `quiet`, `sym`. `ex` and `on` are already Yan words or mock-shared.

### 5.4 The four tasks in the mix

T1:

```
--- A lease: at most one holder, and a lease expires after 10s.
spec Lease
  holder: Node? = None
  expires = Instant.min

  acquire(n: Node) when holder == None or now >= expires
    holder = n
    expires = now + 10s

  release(n: Node) when holder == n
    holder = None

  always one_holder: holder == None or now < expires + 1s
  can handoff: acquire(a) .. acquire(b)
  ex acquire(a), release(a), acquire(b)
  ex! acquire(a), acquire(b)

world small for Lease
  Node = {a, b, c} sym
  time 1s to 15s
  depth 12
```

T2:

```
--- Two-phase commit with a coordinator that can crash and recover.
spec TwoPhase
  vote: Map[Node, Vote?] = Map.fill(None)
  decision: Decision? = None
  done: Map[Node, Decision?] = Map.fill(None)
  up = true

  cast(n: Node, v: Vote) when vote[n] == None
    vote[n] = v

  decide() when up and decision == None and vote.all(_ != None)
    decision = if vote.all(_ == Yes) then Commit else Abort

  learn(n: Node) when up and decision != None and done[n] == None
    done[n] = decision

  fault crash() when up
    up = false

  recover() when not up
    up = true

  never split: done.any(_ == Commit) and done.any(_ == Abort)
  always valid: not done.any(_ == Commit) or vote.all(_ == Yes)
  after finish: decide() expect done.all(_ != None) within 12 steps
  can all_commit: done.all(_ == Commit)
  ex cast(a, Yes), cast(b, No), decide(), learn(a), learn(b)

world three for TwoPhase
  Node = {a, b, c} sym
  crash up to 2
  depth 20
```

T3:

```
--- Sequential model of a bounded FIFO, the oracle for a concurrent queue.
spec Fifo[T](cap: int)
  items: List[T] = []

  push(x: T) when items.len() < cap
    items.push(x)

  pop() -> T when items.len() > 0
    items.remove(0)

link Fifo[int](cap: 4) to type MpmcQueue[int]
  on push(x) -> push(x)
  on pop() -> pop() == result
```

T4:

```
link Lease to actor LeaseServer
  holder = state.owner
  expires = state.deadline
  on Acquire(n) -> acquire(n)
  on Release(n) -> release(n)
  on Ping -> quiet
```

The T3 link to a `type` checks linearizability: `check_linearizable` (18.4) gets the spec as the
sequential model, and `explore` drives concurrent callers. The T4 link to an `actor` checks trace
refinement at message boundaries. A link to a `log` (not shown) takes a recorded trace with no
state mapping and checks only event order, which covers black-box systems.

### 5.5 Five facts (N5)

| Fact | Mark in Yan Spec |
| --- | --- |
| failure | steps cannot fail; a false `when` means the step is not enabled. Yan helper functions keep `fail E`. |
| capability use | none by construction: a `spec` header means `uses none`. Time, faults and choice come from the world. |
| mutation | state assignments inside a step body; the write set is printable with `--frames`. |
| concurrency | interleaving of enabled steps; each step is atomic. `par` is not allowed in a step. |
| compile-time evaluation | worlds are stage-0 literals; spec parameters such as `cap` are fixed per world. |

### 5.6 Lowering sketch (N1)

- `spec S` lowers to a record type `S.State` with the fields and their initial values.
- Each step lowers to a `uses none` function `(State, params) -> State?` (or `(State, T)?` with an
  output) that returns `None` when the guard is false. `pick` reads the std choice sequence.
- `always`, `never` and `can` lower to `State -> bool` predicates; `after ... within` lowers to a
  small monitor whose state is added to the explored state; `ex` traces lower to tests.
- A world lowers to a `std.sim` harness configuration; `sym` sets enable symmetry reduction.
- A link to an actor lowers to an observer handler at the actor's message boundary that records the
  mapped state before and after each message and asks the spec for a matching step. This is the one
  form with semantics of its own (R568): the checker engine, at the runtime layer.

### 5.7 What was tried and dropped

- **Unicode operators** (`□`, `◇`, `∀`, `′`): beautiful in print, slower to type, costlier for
  models, and the ASCII forms already read well. Dropped.
- **Primed simultaneous assignment** (Prime): dropped for one meaning of `=` across Yan.
- **Explicit `reads` and `changes`** (Proof): dropped for inferred frames, shown on demand.
- **Step tables with `|`** (Ledger): dropped; guards in the header give the same locality without
  wrapping problems or a clash with pattern alternation.
- **Full behavioral threads** (Score): kept only the event patterns in laws and the event-only `log`
  link. Request, wait and block threads could return later as an experiment in Yan Meta.
- **Unbounded liveness and fairness keywords**: out of scope per R548; `within` covers practice.
- **`@prove inductive` laws** (Proof): not dropped, deferred. Laws in the 8.6 decidable fragment
  could go to the solver later with no syntax change beyond `@prove` on the spec.

## 6. Open questions

1. **Protected laws.** Laws and `ex` traces act like tests. Should `.ysp` files be protected targets
   (agents propose, the lead approves), as tests are under the patch protocol? Recommendation: yes
   for laws, worlds and `ex`; steps may be agent-edited when a link's monitor still passes.
2. **Assignment spelling.** The RFC example uses `<-`; the mix uses `=` because step bodies are
   sequential Yan. Keep `<-` only if spec state must look different from program state.
3. **Step outputs.** `pop() -> T` with the last expression as the output matches Yan. Is `result`
   the right name in a link route, or should routes bind the reply by pattern?
4. **Event patterns in laws.** How far to go: sequences (`..`), `unless`, counting? The mix stops at
   response, existence and sequence; more needs evidence from real specs.
5. **Link to Yan Protocol.** A `.ypc` protocol could generate a spec skeleton, or a spec could
   check that an actor pair follows a protocol. Which direction, if any?
6. **Mining.** A tool could propose state fields and invariants from recorded traces (Daikon
   style) or draft a spec from an actor's handlers. Useful for agents, but drafts must pass the
   vacuity checks and go through review like any law.
7. **Proofs.** When the formal track and prove mode mature, do inductive laws come to `.ysp` or stay
   in plain Yan with `ghost` state and lemmas?
