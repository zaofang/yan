# 0002: Niche surfaces

- Status: accepted (the lead accepted every decision below on 2026-11-01: "Accept everything into rfcs")
- Research: [Surface shortlist, revision 2](https://claude.ai/artifact/3cSMv6uiyc9DgCXBDGsmhD),
  which supersedes [Niche surfaces](https://claude.ai/artifact/KeTJFKjzLUGkq1DXDCTSdk) and
  [Surface mock-ups with names](https://claude.ai/artifact/PH4PixndFSBKFfsRKzZrbn); background in
  [Positioning and market gaps](https://claude.ai/artifact/5HW3yvtnC4CcdwREm5Byz3) and
  [Novelty audit](https://claude.ai/artifact/UGAdAaZBTP4kFekXPszC1M)
- Rows: R536 to R550

## Summary

Yan gains a family of planned niche surfaces: small, domain-shaped spellings of a subset of Yan,
each serving one market that today uses its own language (protobuf, BAML, PRQL, Terraform, Temporal,
bash, Kaitai, Lustre, Catala and others). A niche surface adds syntax only. It lowers into
`yan-hir` like Yan, prints back exactly, keeps the item hash, and shows the five facts. Thirteen
surfaces are accepted: eight practical (Yan Prompt, Yan Schema, Yan Infra, Yan Query Language, Yan Workflow, Yan Shell, Yan Binary Format, Yan Dataflow) and five research
(Yan Delegate, Yan Probabilistic, Yan Protocol, Yan Spec, Yan Law). None is built before 1.0. Each starts as a print-only view and gets its
reader, grammar, diagnostic codes and conformance column in its own later RFC, the way Yan Meta's grammar
moves from RFC 0001 into section 16.6.

The spec gets the admission rules and a table of planned surfaces (new section 16.7) and a post-1.0
note in section 23. This file stays the reference for each surface until its own RFC. The Yan
changes these surfaces need are in RFC 0003.

Programs below are design mocks; none has been parsed or checked. Diagnostics carry study-local
labels (XUN-1 and so on); codes are assigned in each surface's own RFC.

## Decisions

### D1: Admission rules and shared machinery

A niche surface is admitted only if it meets all nine rules (N1 to N9). Yan Meta's K1 to K5 (R517) are
the model; the difference is that a niche surface prints only a subset of Yan and has exactly one
printing per tree, so it needs no spelling notes.

- **N1 Syntax only.** The surface lowers to Yan Core that Yan can write, or to a Yan Core extension already
  accepted. It has no private semantics (law 1).
  Superseded 2026-11-01 by R568 (spec 16.7): a niche surface may define semantics of its own behind
  a boundary signature.
- **N2 Subset with a checker.** A decidable predicate over Yan Core items says whether an item prints in
  the surface. Outside it, `yan view --as <name>` reports the first excluding construct, as 16.4 does
  for Yan Agent.
- **N3 One printing.** Law 8 holds as for Yan Agent and Yan: one canonical printing per tree. Sugar is
  recovered from the Yan Core tree alone, from stage-0 metadata attached by the lowering, or from
  hash-neutral trivia already defined in 16.5. Each surface states which.
- **N4 Same item hash.** A lowering never creates or drops a hashed name (K2). Generated names follow a
  fixed, documented rule.
- **N5 Five facts visible.** Each surface names its marks for mutation, failure, compile time,
  concurrency and capability use (law 4).
- **N6 Crate.** `yan-<name>` holds the lexer, parser, AST and lowering into `yan-hir`. No surface
  touches `yan-elab` or anything below it.
- **N7 Round-trip laws.** For surface S: `S(elaborate(F)) == F` byte for byte for canonical F; and
  `elaborate(S(W)) == elaborate(W)` by item hash for any Yan W in the subset. Conformance gains one
  column per surface.
- **N8 Names and diagnostics.** Each surface adds a column to the projection table (16.3) for names it
  spells differently. Messages render per surface from the shared registry (R297); a surface adds
  codes only for its own syntax.
- **N9 Files and lexis.** Extension `.<name>`, one surface per file, any mix of surfaces per package,
  calls across surfaces by name. Lexical rules as 13.1: UTF-8, ASCII identifiers, the lexical safety
  rules, snake_case values and PascalCase types.

- Decision: accepted, 2026-11-01
- Row: R536

### D2: Yan Schema 言契, schemas

**Job.** Declare wire types and remote interfaces once, with their version history. **Users:** API
and platform teams on protobuf, OpenAPI or JSON Schema.

```
package acme.orders

record Order since v1
  id: OrderId
  sym: str
  side: Side
  qty: i32 where qty > 0
  limit: Money?
  tif: Tif = Tif.day since v2
  account: AccountId was owner since v3

enum Side
  buy
  sell

service Book
  place(o: Order) -> Ack fail Rejected
  cancel(id: OrderId) -> Ack fail NotFound
```

- **Subset.** Record and sum types deriving `Wire`, refinement fields, and `@remote` traits whose
  methods have `Wire` parameters, results and failures. No bodies.
- **Grammar sketch.** `record = "record" NAME [version] INDENT field+ DEDENT`;
  `field = NAME ":" type ["where" pred] ["=" const] ["was" NAME] [version]`;
  `version = "since" VERSION`; `service = "service" NAME INDENT method+ DEDENT`.
- **Lowering.** `record` is `type T = {...}` with `derive(Wire)`; `where` is a refinement
  `{q: i32 | q > 0}` (3.5), checked at construction (8.5); `service` is a `@remote` trait and its
  `Remote` handle (R401, R407). `since`, defaults and `was` feed the stage-0 converter generator
  (R406); `yan schema pin` records each version in `.yan/schemas/`. There are no field tags:
  schema identity is the structural hash.
- **Recovery.** Types and refinements are exact. Version annotations are read back from the pinned
  history, so printing needs `.yan/schemas/` beside the source.
- **Five facts.** Failure in each method signature; no other fact occurs.
- **Diagnostics.** QI-1 a field added in a later version without a default (no converter possible);
  QI-2 a removed field still read by a pinned converter; QI-3 a non-`Wire` type in a service
  signature (E720 territory).
- **Interop.** `@proto(n)` on a field exports `.proto` through `@yan/protobuf` and `@yan/grpc`; tags
  never affect the schema hash.
- **Needs.** Streams over `Remote` (RFC 0003). Until then a stream lowers to a subscription over
  `Group` or a callback actor.
- **First slice.** Records, sums, refinements, `since`, `was`, services, `yan schema check`. Later:
  OpenAPI and JSON Schema import and export.

- Decision: accepted, 2026-11-01. First after 1.0 (D15).
- Row: R537

### D3: Yan Prompt 言询, typed model programs

**Job.** Write model calls as typed functions: prompt, schema, validation and examples in one item.
**Users:** teams building LLM features (the BAML and DSPy audience), and agents writing their own
tools.

```
type Triage = { severity: Sev, team: Team, summary: str where len(summary) <= 200 }

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

pub route(t: Ticket) -> Team fail ModelError uses model, pager =
  let r = triage(t)?
  if r.severity == Sev.p1 then pager.page(r.team, r.summary)
  r.team
```

- **Subset.** Functions whose body is one model generation over role sections, followed by an
  optional `validate`. Everything else in the file is ordinary Yan-shaped code.
- **Grammar sketch.** `prompt_item = "prompt" NAME params "->" type ["fail" type] "uses" caps "="
  INDENT [model_line] role_section+ [check_line] {ex_line} DEDENT`;
  `role_section = ("system" | "user" | "assistant") (STRING | TSTRING)`;
  `check_line = "check" lambda`.
- **Lowering.** Role sections become the message list of `model.generate_chat[T]` (RFC 0003), then
  `.validate(check)`, whose result is `T`, not `Untrusted[T]`, because validation removes the label
  (R451). `model "fast"` wraps the call in `model.only(models=["fast"])`. Without `check` the function
  returns `Untrusted[T]`. Model calls are recorded under R305.
- **Recovery.** Exact: each role section is one message.
- **Five facts.** `uses model`; `?` at call sites; failure in the header.
- **Diagnostics.** XUN-1 the result type is not `Wire`; XUN-2 a plain `str` (not `t"..."`)
  interpolates an untrusted value; XUN-3 the result is used before `check` when the header promises
  `T`.
- **Depends on.** 17.4 (R450, R451, R452) and `generate_chat` (RFC 0003).
- **First slice.** System and user roles, `check`, `ex` with `fake_model`. Later: tool calls
  (`tool_call`, R450), streaming, few-shot blocks.

- Decision: accepted, 2026-11-01. Second after 1.0 (D15).
- Row: R538

### D4: Yan Query Language 言查, queries

**Job.** Typed, compiled, deterministic table pipelines. **Users:** analysts, data engineers, dbt
users, and agents fluent in SQL.

```
query movers(trades: Table[Trade], since: PlainDate) -> Table[Mover] =
  from trades
  where day >= since and qty > 0
  derive notional = px * qty
  group sector, sym (notional = sum(notional), vwap = sum(px * qty) / sum(qty))
  sort sector, -notional
  take 5 per sector
  ex movers(fixtures.trades, 2026-10-01).len() == 15
```

- **Subset.** Functions whose body is one `LazyFrame[R]` pipeline over `Table` inputs, `uses none`.
  Reading from storage stays in a Yan or Yan Agent function.
- **Grammar sketch.** PRQL-ordered stages, one per line: `from`, `where`, `derive`, `select`,
  `group KEYS (AGGS)`, `join T on PRED`, `sort KEYS`, `take N [per KEYS]`. Bare column names resolve
  against the row type at that stage.
- **Lowering.** `from x` is `x.lazy()`; `where` is `filter` with a `ColExpr` (5.6); `derive` is
  `with_columns`; `group` is `group_by` and `agg`; `sort` is the stable `sort` (11.9); `take` is
  `head`; `take N per K` is a window rank filter (RFC 0003). Release builds compile constant-shape
  plans at stage 0 (11.9).
- **Recovery.** Exact: each stage is one plan node.
- **Five facts.** Arithmetic failures surface as `fail QueryError` (11.9) in the header; no other
  fact occurs inside a query.
- **Diagnostics.** CHA-1 a column not in the row type at that stage, listing the available columns;
  CHA-2 an aggregate outside `group`.
- **Relation to existing work.** `@yan/frame` SQL text yields an untyped `LazyFrame[Dyn]`; Yan Query Language is the
  typed alternative. Yan Meta's query comprehensions (RFC 0001, M3) lower to the same operations. `@yan/rel`
  (Datalog) may become a second Yan Query Language mode.
- **First slice.** `from`, `where`, `derive`, `select`, `group`, `sort`, `join`. Later: windows,
  `take per`, incremental maintenance (DBSP-style).

- Decision: accepted, 2026-11-01
- Row: R539

### D5: Yan Shell 言行, capability-safe shell

**Job.** Replace bash for scripts, CI steps and agent actions, with authority stated up front.
**Users:** ops and CI authors, and agents.

```
#!/usr/bin/env -S yan run
main(args: Args) uses proc, fs, net =
  let logs = fs.root("./logs")
  let out = fs.root("./out", write)
  let backup = net.only(connect = ["backup.example.com:22"])
  for f in ls(logs, "*.log")
    if f.age > 1d
      lines(f) | grep("ERROR") | sort | uniq(count) > out / "\(f.stem).errors"
      gzip(f)?
  run t"rsync -a {out.path} backup.example.com:/srv/errors/" with net = backup ?
```

- **Subset.** Functions whose statements are pipelines, redirections, `run` commands and ordinary
  control flow.
- **Grammar sketch.** `pipeline = stage {"|" stage} [(">" | ">>") expr]`; a stage is a call to a Yan Shell
  built-in (`lines`, `grep`, `sort`, `uniq`, `head`, `cut`, `wc`, ...) or `run TSTRING`. Bare words are
  never programs; only `run` starts a process.
- **Lowering.** Built-ins are std functions over line iterators; `|` composes iterators; `>` writes
  through a rooted handle (17.3); `run t"..."` builds a `proc.Command` whose argv is the template's
  literal parts split on spaces at stage 0, each interpolation exactly one argument, never a shell
  string (R448). A non-zero exit is a typed `ProcError`.
- **Recovery.** Exact for pipelines; `run` prints back from the Command's template.
- **Five facts.** `uses proc, fs, net` in the header; `?` at each fallible stage; rooted handles name
  the reachable paths.
- **Diagnostics.** LING-1 an interpolation inside a quoted literal part (`t"'{x}'"`), which would not
  be one argument; LING-2 writing through a read-only root.
- **Open.** A ceiling-level `fs.only` (RFC 0003) would let package ceilings name paths.
- **First slice.** Ten built-ins, pipes, redirection, `run`, rooted handles. Later: globbing rules,
  job control.

- Decision: accepted, 2026-11-01
- Row: R540

### D6: Yan Workflow 言序, durable workflows

**Job.** Long-running business processes that survive crashes and deploys. **Users:** backend teams
on Temporal, Step Functions or job queues.

```
workflow onboard(user: UserId) -> Done fail OnboardError
    uses crm, billing, mail, timer =
  step account = crm.create_account(user)
  step billing.start_trial(account, 14d)
  par
    step mail.send(t"Welcome, {user}")
    step crm.schedule_call(account)
  wait 13d
  step mail.send(t"Your trial ends tomorrow")
  wait until billing.converted(account) or 2d
  Done
```

- **Subset.** `@durable` functions (R416) whose bodies are steps, `par` blocks, waits and pure code.
- **Lowering.** `workflow` is an `@durable` function; `step e` is a journaled capability operation
  with the R453 idempotency key; `wait d` is a durable timer (RFC 0003); `wait until c or d` polls `c`
  as a journaled step with backoff until it holds or `d` passes; code changes inside a running
  workflow use `durable.version` (R453).
- **Recovery.** Exact: every journaled operation is a capability call.
- **Five facts.** Capabilities in the header; `par` blocks; failure in the header.
- **Diagnostics.** E725 already covers non-journaling capabilities; CHENG-1 a `step` whose handler
  declares no idempotency (W850 in Yan; an error in Yan Workflow).
- **Depends on.** `@yan/durable` (M4, R419) and durable timers (RFC 0003). Yan Workflow cannot ship first.
- **First slice.** Steps, `par`, `wait d`, versioning, `yan replay --against REV` as the deploy check.

- Decision: accepted, 2026-11-01
- Row: R541

### D7: Yan Infra 言基, infrastructure

**Job.** Declare cloud resources as a typed program with plan and apply. **Users:** platform teams on
Terraform or Pulumi.

```
stack web(env: Env) -> Stack fail CloudError uses cloud =
  let net = cloud.vpc(cidr: "10.0.0.0/16")
  let db = cloud.postgres(net, size: Db.small, backups: 7d)
  let app = cloud.service(net,
    image: "ghcr.io/acme/web:1.4",
    replicas: if env == Env.prod then 3 else 1,
    env: {"DB_URL": db.url},
  )
  cloud.dns("acme.example", app.lb)

test_web_plan() uses none =
  let p = plan(web(Env.prod), state = empty())
  assert p.creates == 5
```

- **Subset.** Functions whose only capability is a resource capability (a user `@capability` trait
  per provider) and whose body is resource calls and pure expressions.
- **Lowering.** Each resource call is a capability operation returning a handle whose attributes are
  `Deferred[T]`, known only after apply. A `Deferred` may flow into another resource's arguments;
  branching on one is an error (ZHU-1). `plan` binds a dry-run handler (RFC 0003) that records the
  operation graph against a state snapshot. `apply` binds the provider handler, re-reads state, and
  fails with `Drift` where a read differs from the plan: determinism covers the program, not the
  cloud.
- **Recovery.** Exact: ordinary call syntax under a `stack` keyword.
- **Five facts.** `uses cloud`; failure in the header.
- **Diagnostics.** ZHU-1 control flow depends on a `Deferred` value; ZHU-2 a resource created in a loop
  with no stable key.
- **Depends on.** Handlers (7.3), recording (7.6), `Untrusted` provider responses (R451), and the
  dry-run handler class (RFC 0003).
- **First slice.** One provider (local Docker, or a fake for tests), plan, apply, drift check. Provider
  bindings are packages.

- Decision: accepted, 2026-11-01
- Row: R542

### D8: Yan Binary Format 言码, binary formats

**Job.** Declare packet, file and register layouts and get checked zero-copy parsers and writers.
**Users:** networking, embedded, file-format and security engineers.

```
layout Ipv4Header big_endian msb_first
  version: u4 = 4
  ihl: u4 where ihl >= 5
  dscp: u6
  ecn: u2
  total_len: u16 where total_len >= ihl * 4
  id: u16
  flags: u3
  frag_off: u13
  ttl: u8
  proto: IpProto as u8
  checksum: u16 = ones_complement(header)
  src: Ipv4
  dst: Ipv4
  options: bytes[(ihl - 5) * 4]
```

- **Subset.** A record type with its generated `parse` and `write` functions.
- **Lowering.** A view-backed record whose accessors read bits from a `View[u8]` in the declared byte
  and bit order. `parse(View[u8]) -> Ipv4Header from buf fail ParseError` checks length, refinements
  and computed fields; `write` fills computed fields. Fixed fields (`= 4`) are checked on parse and
  written on write. Dependent lengths are view slices whose bounds the solver proves from the
  refinements (8.6). LSB-first register layouts lower to `@layout(bits(uN))` instead (5.2).
- **Recovery.** Exact, from stage-0 metadata the lowering attaches to the record.
- **Five facts.** Failure in `parse`; views borrow from the buffer.
- **Diagnostics.** WEI-1 field widths in a byte group do not end on a byte boundary; WEI-2 a dependent
  length the solver cannot bound; WEI-3 a computed field that reads itself.
- **Prior art.** Kaitai Struct, EverParse, Wuffs.
- **First slice.** Fixed and dependent lengths, both byte orders, refinements, computed fields. Later:
  tagged unions, variable-length integers, streaming parse.

- Decision: accepted, 2026-11-01
- Row: R543

### D9: Yan Dataflow 言川, synchronous dataflow

**Job.** Control laws and signal processing in the Lustre and SCADE style, on the embedded profile.
**Users:** avionics, automotive, rail, robotics and audio DSP engineers.

```
const dt: f32 = 0.001

node pid(err: f32, g: Gains) -> f32 =
  let i = 0.0 -> clamp(last(i) + err * dt, -g.windup, g.windup)
  let d = 0.0 -> (err - last(err)) / dt
  g.kp * err + g.ki * i + g.kd * d

@task(priority = 2)
pub node attitude(target: Angles, imu: Imu, g: Gains3) -> Motors every 1ms =
  let e = target - imu.angles
  mix(pid(e.roll, g.roll), pid(e.pitch, g.pitch), pid(e.yaw, g.yaw))
```

- **Subset.** A state record and a step function `step(var s: S, inputs) -> outputs`, `uses none`, no
  allocation, as generated from a node.
- **Spelling.** Lustre's `pre` is `last`, because `pre` is the contract keyword (8.4).
- **Lowering.** Each node is a state record (one field per `last` use and per sub-node instance) and a
  step function; `a -> b` reads a first-tick flag; `every 1ms` binds the step to a periodic task under
  the embedded `executor` or `srp` handler (17.1, R323). Static memory and the stack bound follow from
  the embedded profile (R320 to R329).
- **Recovery.** Exact, from the state record's generated attribute.
- **Five facts.** `var s` on the step function; `uses none`; the task attribute.
- **Diagnostics.** LU-1 a causality cycle within one tick; LU-2 `last` of a value with no first-tick
  value.
- **Limits.** One clock. Sub-clocks (`when`, `merge`) need a clock calculus; a WCET bound would make
  it certifiable.
- **First slice.** Nodes, `last`, `->`, sub-node instances, `every`. Later: sub-clocks, automata, WCET.

- Decision: accepted, 2026-11-01
- Row: R544

### D10: Yan Delegate 言代, contracts with generated bodies (research)

```
dream dedupe_emails(xs: List[str]) -> List[str] =
  post len(out) <= len(xs)
  post all i in 0..len(out): all j in 0..len(out): i == j or lower(out[i]) != lower(out[j])
  ex dedupe_emails(["A@x.io", "a@x.io", "b@x.io"]) == ["A@x.io", "b@x.io"]
  ex dedupe_emails([]) == []
  body 3f9a...c21
```

- **Idea.** The source is the contract; the body is a pinned, checked artifact. `yan dream` runs the
  agent patch loop (section 21) to fill the body, writes it as checked-in Yan Agent in a sibling file, and
  records its item hash on the `body` line. `yan build` never calls a model; it checks that the pinned
  body exists, matches its hash and meets the contracts (proved, or runtime-checked under R211).
- **Lowering.** A `dream` item is an ordinary function whose body is the pinned item; with no body it
  is a Yan Agent hole (R212). Related: synthesis from examples (R529).
- **Questions.** Regeneration stability when the header changes; whether review of the body is
  required; how strong contracts must be; how `out` reads in quantified posts.
- **First slice.** One command, one model handler, pinned bodies, a CI check that every `dream` has a
  matching body.

- Decision: accepted for research, 2026-11-01
- Row: R545

### D11: Yan Probabilistic 言随, probabilistic programming (research)

```
model coin_bias(flips: List[bool]) -> f64 uses prob =
  let p = sample beta(1.0, 1.0)
  for f in flips
    observe bernoulli(p) == f
  p

main() uses prob, io =
  let post = infer(coin_bias([True, True, False, True]), importance(draws = 2000, seed = 7))
  io.print(f"mean {post.mean():.3f}")
```

- **Idea.** `sample` and `observe` are operations of a user capability `@capability trait Prob` in
  `@yan/prob`; inference algorithms are handlers, so one model runs under any of them. Randomness
  comes from a seeded `rand` handler (7.1), so posteriors are identical on every run.
- **Constraints.** A user capability is exclusive by default (7.1), so parallel chains run one handler
  each. NUTS and HMC need `grad`, an M4 preview (11.5); the first slice uses importance sampling and
  SMC. Distributions come from `@yan/rand_distr`.
- **Questions.** The spelling of `observe` on continuous data; enumeration of discrete variables; one
  library for Yan Probabilistic and Yan Meta's probabilistic models (RFC 0001, M4, which passes a `Trace` parameter), so
  the two must agree on capability versus parameter in Yan Probabilistic's own RFC.

- Decision: accepted for research, 2026-11-01
- Row: R546

### D12: Yan Protocol 言礼, session-typed protocols (research)

```
protocol Checkout between Client, Shop
  Client -> Shop: Start(cart: Cart)
  Shop -> Client: Quote(total: Money)
  choice by Client
    accept:
      Client -> Shop: Pay(token: Token)
      Shop -> Client: Receipt or Declined
    cancel:
      Client -> Shop: Cancel
```

- **Idea.** Each role and step is a `@linear` state type; each send is a `sink` function that consumes
  the current state and returns the next, so skipping or reordering a message, or dropping a session
  unfinished, is a compile error (E412, R443).
- **Lowering.** Local sessions run over `std.chan` pairs; sessions over `Remote` need `Wire` messages
  (E720). Generated state names follow a fixed rule (`CheckoutClient2` = protocol, role, step index).
- **Questions.** Recursive protocols; release of the linear state on failure mid-session (R443 allows
  release on a failing path).

- Decision: accepted for research, 2026-11-01
- Row: R547

### D13: Yan Spec 言规, specs checked against code (research)

```
spec Lease
  var holder: Node? = None
  var expires: Instant = Instant.min
  var now: Instant = Instant.min

  action acquire(n: Node) when holder == None or now >= expires
    holder <- n
    expires <- now + 10s
  action tick(d: Duration) when d > 0s
    now <- now + d

  always one_holder: holder == None or now < expires + 1s

refines actor LeaseServer
  holder = state.owner
  expires = state.deadline
```

- **Idea.** A spec lives beside the code, is explored with `std.sim` and `explore` (6.9, 17.7), and is
  checked against the actor: under replay each handled message must map to one spec action whose guard
  held, through the refinement mapping read at message boundaries.
- **Limits.** Bounded exploration; no liveness proofs.
- **Prior art.** TLA+, Quint, TLA+ trace validation.

- Decision: accepted for research, 2026-11-01
- Row: R548

### D14: Yan Law 言法, law as code (research)

```
scope IncomeTax
  input income: Money
  input dependents: int
  output tax: Money

  definition allowance equals 12000 EUR plus 2000 EUR times dependents
  definition taxable equals max(income minus allowance, 0 EUR)

  rule tax equals taxable times 20pct
  exception when taxable is more than 50000 EUR
    rule tax equals 10000 EUR plus (taxable minus 50000 EUR) times 40pct
```

- **Idea.** Statute-shaped text lowered to `uses none` functions over `Money` (11.7) and `pct` (17.3).
  Exceptions follow default logic: the most specific applicable rule wins, and two applicable rules at
  one level fail with `Conflict`. Each output records the rule that produced it.
- **Questions.** Rounding points (`quantize` with an explicit `RoundingMode`, 11.7) must be stated in
  the rules.
- **Prior art.** Catala.

- Decision: accepted for research, 2026-11-01
- Row: R549

### D15: Order and gating

1. No niche surface is built before 1.0.
2. After 1.0: Yan Schema first (types only; it proves the shared machinery), then Yan Prompt.
3. Yan Query Language and Yan Shell next. Yan Workflow waits for `@yan/durable` (M4) and durable timers; Yan Infra waits for the
   dry-run handler class (RFC 0003).
4. Every surface starts as a print-only view (`yan view --as <name>`, as Yan Meta is until M4, 16.6) before it gets a reader.
5. Each surface enters the normative spec through its own RFC: grammar, subset predicate, projection
   column, diagnostic codes and conformance column.

- Decision: accepted, 2026-11-01
- Row: R550

## Not admitted

Recorded so the question is not reopened without new evidence. No rows.

| Candidate | Why |
| --- | --- |
| He 核 (GPU kernels) | Yan `@kernel` covers it. |
| Yan 验 (tests) | Property tests, fuzzing and `ex` exist (R301, R303). |
| Yun 允 (policy) | Small market; Yan Agent with `uses none` and `@@prove` covers it. |
| Zheng 证 (proofs) | Lemma bodies exist (R458). |
| Fa 法 (grammars) | A std combinator library suffices; see R527. |
| Yuan 圆 (S-expressions) | Internal tool, no users. |
| Hua 画, Chuan 串, Xi 戏, Qin 琴, Sheng 声 | Toys or tools, no market. |
| Suan 算 (formula), Zhang 账 (ledger) | Need a host or a chain. |
| Xian 线 (hardware) | Simulation fits a package; synthesis is a backend. |
| Xing 形, Wu 无 | A type feature and a tool, not surfaces. |

## Spec changes

- 16.7 (new): admission rules N1 to N9 (R536), the table of planned surfaces (R537 to R549), order and
  gating (R550).
- 23: a post-1.0 note pointing at 16.7 (R550).
