# Yan Infra 言基 (`.yinf`): syntax and semantics study

Status: brainstorm study, 2026-11-04. Not normative. No spec rows, no diagnostic codes. Items are
labelled D1, D2 and so on; a later RFC would number them. Where this page and `spec/` differ, `spec/`
wins.

**One-line answer.** Yan Infra should stay ordinary Yan calls under a `stack` keyword, as RFC 0002 D7
says, and add five things nobody else combines: blast radius as an attenuated capability granted at
the deploy site, resource identity as a stable key path with a `was` clause for renames, lifecycle
clauses (`keep`, `replace via`) checked against the plan, policies as type-indexed pure `rule`
items, and tests that explore `Deferred` values instead of mocking them. The plan is a deterministic,
hashable value; apply runs exactly the plan you saw.

## 1. Methodology and process

1. **Read the current state narrowly.** `docs/surfaces.md` section 11 (status Partial, MOCK grammar),
   RFC 0002 D7 (job, subset, lowering, diagnostics ZHU-1 and ZHU-2), spec 16.7 rows R542 and R568
   (niche surfaces may own semantics behind a boundary signature), R554 (dry-run handler class), spec
   7.3 handlers and R672 (unforgeable host handlers, attenuation with `.only`), 7.6 record and replay,
   R451 `Untrusted[T]`, std `env.secret` returning `Secret[str]` (R686). Nothing else in `spec/` was
   read.
2. **Survey peers.** Terraform and OpenTofu (HCL), Pulumi, AWS CDK and CDKTF, CloudFormation and
   Bicep, Crossplane and kro on Kubernetes, configuration languages (CUE, Dhall, Nickel, Pkl, KCL,
   Jsonnet), Nix and NixOS, infrastructure-from-code (Wing, SST, Encore, Nitric), System Initiative.
   For each: the unit of identity, how unknown values are handled, how refactors avoid destroy, how
   policy and tests work, and what users complain about.
3. **Read the research.** Practitioner and empirical IaC studies, configuration-error studies,
   IaC testing (ProTI), LLM IaC generation benchmarks (IaC-Eval, the 2025 error-taxonomy study),
   convergence theory (Traugott and Brown), promise theory (Burgess), Google's Borg/Omega/Kubernetes
   lessons on config DSLs. Sources are in section 8 with a confidence mark.
4. **Pick five shared tasks** that exercise the hard parts: T1 a web stack (the RFC example plus a
   protected database and an output), T2 per-region fan-out with stable keys and an adopted resource,
   T3 a reusable component taking a secret, T4 a refactor that renames and moves resources without
   destroying them, T5 a policy and a plan test.
5. **Write five proposals, one axis each**: A brevity, B safety and explicitness, C composition power,
   D agent token cost and tooling regularity, E evolution over time. Each writes the same tasks.
6. **Experiment.** Count T1 in each proposal and in two baselines (HCL and Pulumi TypeScript against
   the same hypothetical `cloud` provider). The measure is bytes, non-blank lines, and lexical tokens
   (identifiers, numbers and single punctuation), a rough proxy for model tokens; no tokenizer was
   run. Samples were kept in the session scratchpad and are reproduced in section 6.
7. **Mix.** Take from each proposal what survives its own failure mode, check each pick against
   laws 1 to 7 and R568, and write the consolidated proposal with all five tasks.

What was not done: no parser was built, no provider schema was imported, no model was run on the
samples. Section 7 lists the experiments that would decide the open questions.

## 2. Findings

### 2.1 The market

| Tool | Unit of identity | Unknown values | Refactor without destroy | Policy and test | Main complaint |
| --- | --- | --- | --- | --- | --- |
| Terraform / OpenTofu (HCL) | `type.name[key]` address | `(known after apply)`, opaque | `moved`, `import`, `removed` blocks | Sentinel/OPA outside the language; `terraform test` | HCL is too weak for logic, `count` index churn, state files, slow plans |
| Pulumi (TS, Python, Go...) | URN from logical name and parent | `Output[T]`, `.apply` callbacks | `aliases` option | CrossGuard policy packs, unit tests with mocks | `apply` everywhere, mocks are hand-written, program order hides the graph |
| AWS CDK / CDKTF | construct path | tokens (lazy strings) | logical ID overrides | Aspects, assertions on synthesized template | synthesis indirection; CDKTF sunset by HashiCorp on 2025-12-10 |
| CloudFormation / Bicep | logical ID | intrinsic functions | limited | cfn-guard, what-if | Bicep good but single-cloud |
| Crossplane / kro | Kubernetes object name | reconcile loop, status fields; kro uses CEL expressions | rename is recreate | admission policies | YAML plus CEL plus controllers; debugging a reconcile is hard |
| CUE / Pkl / KCL / Dhall / Nickel | none (values) | n/a, total evaluation | n/a | constraints are the language | they produce config, not plans; no resource lifecycle |
| Nix / NixOS | content hash | none, everything is a derivation input | n/a | evaluation-time asserts | lazy untyped language, poor errors, steep learning |
| Wing, SST, Encore | inferred from app code | inferred | inferred | preflight/inflight checks | Wing Cloud closed in April 2025; inference hides cost and blast radius |

Takeaways, by confidence:

- **High.** The resource graph with stable addresses and a plan/apply split is the industry's settled
  model. Every successful tool has it; the infrastructure-from-code tools that hid it (Wing) did not
  survive commercially, though the cause was not shown to be the model.
- **High.** Position-based identity (`count`) is a known source of accidental destroys, and every tool
  grew a rename mechanism late (`moved` in Terraform 1.1, `aliases` in Pulumi). Identity has to be a
  first-class design decision, not an afterthought.
- **High.** Unknown-until-apply values are the hardest part of general-purpose IaC. Pulumi's
  `Output.apply` is the most cited friction; HCL avoids it by being too weak to branch. RFC 0002's
  `Deferred[T]` with "no branching on a Deferred" (ZHU-1) is the right middle: values flow, control
  does not.
- **High.** Codification is shallow in practice: Firefly's 2025 survey reports 89 percent IaC adoption
  but 6 percent full codification, and fewer than a third of organizations monitor drift
  continuously. Adoption of existing resources (`import`) and drift checks matter as much as writing
  new resources.
- **Medium.** Policy lives outside the language almost everywhere (Sentinel, OPA/Rego, cfn-guard,
  CrossGuard). That costs a second language, a second toolchain and a gap between what the program
  says and what is checked.
- **Medium.** Testing IaC is mostly integration testing against real clouds, slow and costly. ProTI
  (Sokolowski, Spielmann, Salvaneschi) shows that replacing unknown outputs with generated values
  finds bugs in Pulumi programs without a cloud. A language with a first-class `Deferred` can make
  this automatic.
- **Medium.** Models are weak at IaC: IaC-Eval (NeurIPS 2024 datasets track, 458 AWS Terraform
  scenarios) reports GPT-4 at 19.36 percent pass@1 against 86.6 percent on EvalPlus. A 2025 study
  (arXiv 2512.14792) reports 27.1 percent baseline success rising to 62.6 percent overall when
  provider configuration knowledge is injected, and names a "correctness-congruence gap": technical
  validity rises while matching the user's intent plateaus. Typed provider bindings are knowledge
  injection by construction; intent needs to be stated as checkable rules and plan asserts.

### 2.2 The research

- **Convergence (Traugott and Brown, LISA 2002, high).** Order of operations matters; declarative
  convergence is only safe when the tool controls the whole order. Yan's deterministic scheduler and
  a pinned plan give a congruent apply: the same plan, the same order, on any machine.
- **Promise theory (Burgess, medium).** Agents promise their own state; nothing can be imposed on
  another. Maps to capabilities: a stack can touch only what its grant allows, and a provider's
  response is a promise kept or broken (`Drift`), never trusted blindly (`Untrusted`, R451).
- **Borg, Omega and Kubernetes (Burns et al., ACM Queue 2016, high).** Google's config DSLs grew into
  Turing-complete languages that were hard to understand; the lesson drawn is to keep configuration
  as data and put logic in a real language with real tooling. Yan Infra already has a real language
  under it; the surface should keep logic thin and visible, not banned.
- **Configuration errors (Xu and Zhou, ACM CSUR 2015; Xu et al., FSE 2015 "too many knobs", high).**
  Most misconfigurations are wrong values of valid shape, and most knobs are never changed. Types and
  units catch the shape; defaults and rules catch the values.
- **IaC smells (Rahman et al., ICSE 2019; Saavedra and Ferreira, GLITCH, ASE 2022, high).** Hard-coded
  secrets, admin-by-default, missing encryption and HTTP without TLS dominate security smells. Each is
  a type (`Secret[T]`), a capability ceiling, or a type-indexed rule in the mix below.
- **Rehearsal (Shambaugh, Weiss, Guha, PLDI 2016, medium).** Determinism and idempotence of Puppet
  manifests can be checked statically. In Yan they come from the core: the plan is a pure function of
  program, inputs and state snapshot.

### 2.3 What users want, distilled

1. Read the plan and trust it: the diff shown is the diff applied, nothing destroyed by surprise.
2. Refactor freely: rename, split into components, move between stacks without downtime.
3. One language for resources, logic, policy and tests.
4. Fast feedback without a cloud: types, rules and tests that run in seconds.
5. Adopt what already exists, and see drift.
6. Small blast radius: a stack for one service cannot delete another team's database.
7. Secrets that never land in state or logs in clear text.

## 3. Constraints from the core

- **Law 1 and R568.** Yan Infra is a niche surface, so it may own semantics, but the mix below needs
  none: every form lowers to checked Yan Core with handlers (R554 dry-run, provider handlers). That is
  a design goal: the plan engine is a handler, not an engine crate.
- **Law 4, five facts.** `fail` and `uses` stay in the stack header.
- **Recovery.** RFC 0002 says "exact: ordinary call syntax under a `stack` keyword". Every sugar in
  the mix must print back from Yan via spelling notes (16.5).
- **Determinism.** Same program, inputs and state snapshot give the same plan, byte for byte, and the
  same plan hash. The cloud is outside the guarantee; `Drift` reports where it moved.
- **Capabilities.** Host handlers are unforgeable (R672); attenuation with `.only` is the existing way
  to narrow a grant. Provider responses are `Untrusted` until validated against the provider schema.

## 4. The five proposals

Each proposal writes T1 in full; T2 to T5 appear where they show the axis. Section 5 writes all five
tasks for the mix.

### A. Brevity: "Terse"

Optimizes keystrokes and reading speed for experts. Header facts are implied by the `stack` keyword
(`uses cloud fail CloudError`), calls use juxtaposition, enum cases resolve from the expected type,
bindings drop `let`, fan-out keys default to the element when it is a key type.

```
stack web(env: Env)
  net = vpc "10.0.0.0/16"
  db  = postgres net, size: small, backups: 7d, keep
  app = service net, image: "ghcr.io/acme/web:1.4", replicas: env.prod ? 3 : 1, env: {DB_URL: db.url}
  dns "acme.example", app.lb
  out url = app.lb.url
```

```
stack workers(env: Env, regions: [Region])
  net = adopt vpc "vpc-0a1b"
  each r in regions
    q = queue net, region: r, retention: 4d
    service net, region: r, image: "ghcr.io/acme/worker:2.0", env: {QUEUE: q.url}
  out dashboard = dashboard net, title: "workers \(env)"
```

- **Wins.** Shortest by every measure except D. Reads like a bill of materials.
- **Loses.** Implied header facts break law 4. Juxtaposition and `?:` are foreign to plain Yan, so
  recovery is lossy and `yan view --as yn` looks like a different language. Positional first
  arguments hide which field a value fills.
- **Keep for the mix.** Expected-type enum shorthand (`.small`), default keys for key types, `out`.

### B. Safety and explicitness: "Ledger"

Optimizes for "nothing happens that the file does not say". Every binding is typed, every literal is
wrapped in its refinement type, lifecycle is spelled per event, the header carries a plan contract,
and destructive changes need an approval value at apply.

```
stack web(env: Env) -> Web
    fail CloudError
    uses cloud.only(services: [Vpc, Postgres, Service, Dns])
    ensures plan.deletes.isEmpty() unless approved =
  let net: Vpc = cloud.vpc(cidr: Cidr("10.0.0.0/16"))
  let db: Postgres = cloud.postgres(net, size: Db.small, backups: 7d)
    on delete: refuse
    on replace: create_before_destroy
  let app: Service = cloud.service(net,
    image: Image("ghcr.io/acme/web:1.4"),
    replicas: if env == Env.prod then 3 else 1,
    env: {"DB_URL": db.url},
  )
  let dns: Dns = cloud.dns(Domain("acme.example"), app.lb)
  return Web(url: app.lb.url)
```

```
apply(p, approvals: [Approve.delete(web.db, by: "antonio", reason: "migrating to aurora")])
```

- **Wins.** Blast radius in the header (`cloud.only(services: ...)`), a plan contract the checker can
  enforce, approvals that `yan audit` can list. Closest to the security research findings.
- **Loses.** Twice the size of the RFC example. Wrapping literals in `Cidr(...)` repeats what the
  provider schema already knows. A blast radius written in the stack is the wrong place: the same
  stack deploys to dev and prod with different grants.
- **Keep for the mix.** Attenuated grants, moved to the deploy site; `keep` as the one-word form of
  `on delete: refuse`; approvals as values; literals checked against the schema at compile time
  without wrappers.

### C. Composition power: "Graph algebra"

Optimizes for reuse at scale. A stack is a value of type `Graph`; combinators transform graphs.
`&` overlays a partial configuration (CUE-style unification), `each` applies an aspect to every
resource of a type, environments are a product of parameters.

```
stack web(env: Env) fail CloudError uses cloud =
  let net = cloud.vpc(cidr: "10.0.0.0/16")
  let db = cloud.postgres(net, size: .small, backups: 7d) & keep
  let app = cloud.service(net, image: "ghcr.io/acme/web:1.4", env: {"DB_URL": db.url})
    & (env == .prod ? replicas(3) : replicas(1))
  cloud.dns("acme.example", app.lb)
  out url = app.lb.url

let hardened = web(.prod)
  |> each(Service, s => s & {replicas: max(s.replicas, 2)})
  |> each(Bucket, b => b & {encryption: .kms})
  & tags({"team": "web", "cost": "web-prod"})

deploy for env in [.dev, .prod], region in [.us_east_1, .eu_west_1] = web(env) @ region
```

```
rule every_service_has_an_alarm(g: Graph) =
  for s in g.all(Service)
    require g.any(Alarm, a => a.target == s), "\(s.name) has no alarm"
```

- **Wins.** Org-wide defaults and hardening in a few lines; graph queries make rules about
  relationships ("every service has an alarm") expressible, which per-resource policy engines cannot.
- **Loses.** Overlays and aspects are action at a distance: the value of `app.replicas` depends on
  code elsewhere. This is the Kustomize and CDK Aspects complaint, and the Borg lesson. Unification
  needs a lattice the core does not have.
- **Keep for the mix.** Graph queries inside rules, read-only. Stacks calling stacks as the only reuse
  form. Decline overlays and aspects.

### D. Agent token cost and tooling: "Regular"

Optimizes for generation by models and for tools. One resource per block, every line `key value`,
no infix expressions outside `match`, canonical order taken from the provider schema, LL(1). The
grammar of a stack is generated from provider schemas, so a constrained decoder (GBNF or llguidance)
can only emit fields that exist with values of the right shape.

```
stack web
  param env Env
  vpc net
    cidr "10.0.0.0/16"
  postgres db
    vpc net
    size small
    backups 7d
    keep
  service app
    vpc net
    image "ghcr.io/acme/web:1.4"
    replicas match env prod 3 _ 1
    env DB_URL db.url
  dns dns
    name "acme.example"
    target app.lb
  out url app.lb.url
```

- **Wins.** Fewest lexical tokens (78 for T1, section 6). Trivial to diff, merge and sort; a plan diff
  can print in the same format. Constrained decoding from schemas attacks the "technical validation"
  half of the IaC-Eval failure mode directly.
- **Loses.** Not Yan: logic is crippled (`match` only), every helper must live in another surface,
  and the file does not print back from plain Yan. Most lines, so the most vertical scrolling.
- **Keep for the mix.** Schema-derived grammar for constrained decoding, canonical printing order,
  one field per line in argument blocks, a plan diff format that reads like source.

### E. Evolution over time: "Timeline"

Optimizes for change, not state. The file is the desired state plus the changes that matter for
getting there: identity history (`was`), how a forced replacement is done (`replace via`), and how a
change rolls out (`roll via`). The plan becomes "which transitions run", and a transition that would
destroy a `keep` resource without a `via` strategy is a plan error.

```
stack web(env: Env) fail CloudError uses cloud =
  let net = cloud.vpc(cidr: "10.0.0.0/16")
  let db = cloud.postgres(net, size: Db.small, backups: 7d)
    keep
    replace via snapshot_restore
    since 2026-11-04
  let app = cloud.service(net, image: "ghcr.io/acme/web:1.4",
    replicas: if env == Env.prod then 3 else 1, env: {"DB_URL": db.url})
    roll via canary(10%, 10m)
  cloud.dns("acme.example", app.lb)
  out url = app.lb.url
```

```
  let api = api(net, image: "ghcr.io/acme/web:1.4", db_password: pw)
    was {db: db, svc: app}          -- api.db was web.db, api.svc was web.app
```

- **Wins.** Refactors and replacements become reviewable source, not tribal knowledge. `was` sits on
  the resource it renames, unlike Terraform's detached `moved` blocks. Replacement strategies make
  the destroy-recreate decision visible.
- **Loses.** `roll via canary(...)` is orchestration over time, with waits and health checks; it
  drags a workflow engine into the surface. `since` dates are metadata with no semantics.
- **Keep for the mix.** `was`, `replace via`, the plan error for unguarded replacement of `keep`
  resources. Move rollouts to Yan Workflow (`.ywf`), which calls `plan` and `apply` as operations:
  one surface per job.

## 5. The mix: proposed Yan Infra

Base: RFC 0002 D7 (ordinary Yan calls under `stack`, recovery exact). From A: `.case` shorthand,
default keys, `out`. From B: attenuated grants and approvals, without wrappers. From C: graph
queries in rules, stacks calling stacks. From D: argument blocks one field per line, schema-derived
grammar and canonical order. From E: `was`, `replace via`, rollouts delegated to Yan Workflow. Plus
two forms none of the five had: `deploy` items and Deferred-exploring tests.

### 5.1 Forms

- **D1 `stack` item.** `stack name(params) fail E uses caps =` then an indented body. The result type
  is the record of its `out` lines, named `name.Out`; no `->` needed. A stack called from another
  stack is a component: its resources get addresses under the caller's binding name. That is the only
  reuse form.
- **D2 Argument block.** A call followed by deeper-indented `field: value` lines continues its named
  arguments, one per line, no commas or closing parenthesis. It lowers to the same call. The printer
  uses it when a call has more than two named arguments, in the provider schema's field order.
- **D3 Identity.** A resource's address is its key path: stack binding, then `let` name, then `by`
  key in loops. A resource call with no `let` takes the callee's short name, allowed only when that
  kind appears once in its scope. Index keys do not exist (ZHU-2 already rejects a loop resource with
  no stable key). `for x in xs` uses `x` itself as the key when its type is a key type (enums,
  strings, integers); otherwise `by expr` is required.
- **D4 Lifecycle clauses** in the argument block, as bare words: `keep` (a plan that deletes or
  replaces this resource fails unless an approval for it is passed to apply), `replace via strategy`
  (how a change to a replacement-forcing field is carried out; a `keep` resource with such a change
  and no `via` fails the plan), `adopt "id"` (import an existing resource; the first plan reads it and
  reports any difference as an update), `was old` (this resource was at the old address; the plan
  moves state instead of destroying).
- **D5 Schema-checked literals.** A string or number literal passed to a field of a refined provider
  type (`Cidr`, `Image`, `Domain`, `Arn`) is parsed at compile time. A bad literal is a compile error
  with the schema's message. No wrapper calls.
- **D6 `Deferred` discipline.** Attribute reads return `Deferred[T]`. Field access, f-string
  interpolation and pure `.map` keep `Deferred`; passing one into a resource argument is the normal
  case. `if`, `match`, loop bounds and `by` keys on a `Deferred` are ZHU-1. Values known at plan time
  (params, literals, adopted resources' read attributes) are plain `T`.
- **D7 Secrets and ephemera.** A `Secret[T]` argument is stored in state as a digest and printed as
  `(secret)` in every plan, trace (7.6 redaction hook) and log. `ephemeral` before a call marks a
  read-only resource (a token, a looked-up password) whose value is fetched at apply and never
  written to state.
- **D8 `deploy` item.** Binds one stack call to a state backend and a grant:
  `deploy prod = web(.prod)` then an argument block with `state:` and `grant:`. The grant is an
  attenuated provider handle (`aws.only(account: ..., regions: [...])`), built from the program's
  world grants (R672). Blast radius is therefore set where the stack is deployed, and checked: a plan
  operation outside the grant is a plan error, before any API call. `yan infra plan prod` and
  `yan infra apply prod` take a deploy name.
- **D9 Plan as a value.** `plan(stack_call, state:)` returns a `Plan`: operations, a dependency graph,
  and a content hash over canonical bytes. `apply(p)` refuses a plan whose hash differs from the one
  reviewed (`yan infra apply prod --plan 3f9c...`), re-reads state, and fails with `Drift` on any
  changed read (R554). Apply order is the deterministic scheduler's topological order, parallel where
  the graph allows, with logs in task-tree order.
- **D10 `rule` items.** `rule name(x: T) =` a pure body that fails with a `Violation`. A rule over a
  resource type runs for every planned resource of that type; a rule over `Plan` or `Graph` runs once,
  and may query relationships read-only. Rules run at plan time, in the checker's sandbox (`uses none`
  plus the plan), so policy is Yan, tested like Yan, and reported with source spans.
- **D11 Exploring tests.** In a test, `explore d in deferred(stack_call)` runs the stack with every
  `Deferred` replaced by values generated from the provider schema's types, under the seeded test
  handlers, and checks rules and asserts on each run (the ProTI idea, made automatic by the type). A
  failure replays from its seed.
- **D12 Rollouts belong to Yan Workflow.** Canary, blue-green and wait-for-health are workflow steps
  that call `plan` and `apply`. Yan Infra stays a description of resources.

### 5.2 Tasks in the mix

T1, a web stack:

```
stack web(env: Env) fail CloudError uses cloud =
  let net = cloud.vpc(cidr: "10.0.0.0/16")
  let db = cloud.postgres(net)
    size: .small
    backups: 7d
    keep
  let app = cloud.service(net)
    image: "ghcr.io/acme/web:1.4"
    replicas: if env == .prod then 3 else 1
    env: {"DB_URL": db.url}
  cloud.dns("acme.example", app.lb)
  out url = app.lb.url

deploy prod = web(.prod)
  state: s3_state("acme-state", "web/prod")
  grant: aws.only(account: "111122223333", regions: [.us_east_1])
```

T2, fan-out with stable keys and an adopted network:

```
stack workers(env: Env, regions: List[Region]) fail CloudError uses cloud =
  let net = cloud.vpc()
    adopt "vpc-0a1b"
  for r in regions                         -- Region is an enum, so r is the key
    let q = cloud.queue(net, region: r, retention: 4d)
    let w = cloud.service(net, region: r)
      image: "ghcr.io/acme/worker:2.0"
      env: {"QUEUE": q.url}
  out dashboard = cloud.dashboard(net, title: "workers \(env)").url
```

T3, a component taking a secret:

```
stack api(net: Vpc, image: Image, db_password: Secret[str]) fail CloudError uses cloud =
  let db = cloud.postgres(net, password: db_password)
  let svc = cloud.service(net)
    image: image
    env: {"DB_URL": db.url}
    secrets: {"DB_PASSWORD": db_password}
  out url = svc.lb.url

stack web(env: Env) fail CloudError uses cloud, vault =
  let net = cloud.vpc(cidr: "10.0.0.0/16")
  let pw = ephemeral vault.secret("web/db")
  let api = api(net, image: "ghcr.io/acme/web:1.4", db_password: pw)
  out url = api.url
```

T4, refactor: move `db` and `app` into the `api` component, upgrade the engine with a guarded
replacement:

```
  let api = api(net, image: "ghcr.io/acme/web:1.5", db_password: pw)
    was {db: db, svc: app}

-- inside stack api
  let db = cloud.postgres(net, password: db_password)
    engine: "17"
    keep
    replace via snapshot_restore
```

The plan for T4 shows two moves, one replacement by snapshot restore, one in-place image update, and
no destroy. `yan infra tidy prod` deletes a `was` clause once no state under any deploy still holds
the old address.

T5, policy and tests:

```
rule private_buckets(b: Bucket) =
  require not b.public, "bucket \(b.name) is public"

rule services_have_alarms(g: Graph) =
  for s in g.all(Service)
    require g.any(Alarm, a => a.target == s), "\(s.name) has no alarm"

test web_prod_plan() uses none =
  let p = plan(web(.prod), state: empty())
  assert p.creates == 4
  assert p.deletes == 0

test web_outputs_any_cloud() uses none =
  explore d in deferred(web(.prod))
    assert d.out.url.scheme == "https"
```

### 5.3 What the mix declines

- Overlays, aspects and unification (C): action at a distance.
- Implied header facts and juxtaposition calls (A): break law 4 and recovery.
- Per-event lifecycle spelling and literal wrappers (B): the schema already knows.
- A data-only dialect (D): Yan Infra is Yan; a data profile for literal-only files can come later
  from the general data-profile idea, not from this surface.
- Rollouts and `since` metadata in the surface (E): Yan Workflow's job.
- A surface engine (R568): not needed; everything lowers to handlers.

### 5.4 Why this is the right bet

- It is the only design where the blast radius is a type the checker enforces before an API call.
  Terraform and Pulumi learn the radius when IAM says no.
- Identity, renames and replacement strategy live on the resource, in source, reviewed like code.
- Policy, tests and resources are one language and one toolchain; `yan audit` lists every `keep`
  override, approval and `trust` in one report.
- Testing without a cloud is free: `Deferred` is a type, so exploring it is a library call.
- For agents: typed provider bindings plus a schema-derived grammar fix most "technically invalid"
  generations, and rules plus plan asserts give intent a checkable form, which is the gap the 2025
  study found knowledge injection alone does not close.

## 6. Experiment: T1 size

Same task, same hypothetical `cloud` provider. Baselines include a protected database and an output,
as the proposals do; the RFC 0002 example lacks both, so it is slightly short. Lexical tokens are
identifiers, numbers and single punctuation characters; model tokens will differ but rank similarly.

| Version | Bytes | Lines | Lexical tokens |
| --- | --- | --- | --- |
| HCL (Terraform) | 565 | 27 | 153 |
| Pulumi TypeScript | 495 | 13 | 191 |
| RFC 0002 D7 example (no `keep`, no output) | 344 | 9 | 122 |
| A Terse | 253 | 6 | 94 |
| B Ledger | 597 | 15 | 186 |
| C Graph algebra | 352 | 7 | 131 |
| D Regular | 312 | 18 | 78 |
| E Timeline (with rollout and replacement) | 439 | 11 | 146 |
| Mix | 361 | 12 | 120 |

Reading: the mix costs about the same as the RFC example while saying more (`keep`, an output), is
36 percent smaller than HCL in bytes and 37 percent smaller than Pulumi in tokens. D is cheapest in
tokens because it has almost no punctuation; that is the cost of keeping Yan expressions. A is the
lower bound for a Yan-shaped spelling.

The HCL baseline:

```
variable "env" { type = string }

resource "cloud_vpc" "net" {
  cidr = "10.0.0.0/16"
}

resource "cloud_postgres" "db" {
  vpc_id  = cloud_vpc.net.id
  size    = "small"
  backups = 7
  lifecycle {
    prevent_destroy = true
  }
}

resource "cloud_service" "app" {
  vpc_id   = cloud_vpc.net.id
  image    = "ghcr.io/acme/web:1.4"
  replicas = var.env == "prod" ? 3 : 1
  env = {
    DB_URL = cloud_postgres.db.url
  }
}

resource "cloud_dns" "dns" {
  name   = "acme.example"
  target = cloud_service.app.lb
}

output "url" {
  value = cloud_service.app.lb_url
}
```

The Pulumi baseline:

```
import * as cloud from "@acme/cloud";
const env = new pulumi.Config().require("env");
const net = new cloud.Vpc("net", { cidr: "10.0.0.0/16" });
const db = new cloud.Postgres("db", { vpc: net.id, size: "small", backups: 7 },
  { protect: true });
const app = new cloud.Service("app", {
  vpc: net.id,
  image: "ghcr.io/acme/web:1.4",
  replicas: env === "prod" ? 3 : 1,
  env: { DB_URL: db.url },
});
new cloud.Dns("dns", { name: "acme.example", target: app.lb });
export const url = app.lbUrl;
```

## 7. Open questions

1. **Argument blocks in plain Yan.** D2 is surface sugar here. If plain Yan wants it too, it belongs
   in the HIR (R558: two surfaces spell it). Does `.yn` already accept expected-type `.case`
   shorthand, or is that new for both?
2. **Unnamed resources.** D3 allows a callee-named resource when its kind is unique in scope. Simpler
   alternative: every resource needs `let`. Measure which one agents get wrong more.
3. **Grant at deploy versus header.** D8 puts the radius at `deploy`. Should the stack header also be
   allowed a ceiling (`uses cloud.only(services: ...)`), as a package-level ceiling (7.4) is?
4. **Cost rules.** `rule budget(p: Plan) = require p.monthly_cost <= 400 USD/month` needs price data,
   a `Deferred` cost model and units (spec 11). Valuable, but where does price data come from
   deterministically? A pinned price package, like `std.tzdb`?
5. **Provider bindings.** Generate Yan types from Terraform/OpenTofu provider schemas (largest
   ecosystem, plugin protocol) or write native providers? Generating reuses thousands of providers
   but inherits their semantics, including unknown-value rules.
6. **State format and backend.** The state is the canonical `Wire` encoding of the last applied graph
   plus per-resource read hashes. Is it compatible with Terraform state for migration, or only
   importable?
7. **Experiments that would decide these.** Build a parser for D1 to D4 and run the R567-style
   harness on 50 IaC-Eval scenarios rewritten for a fake provider: pass@1 with and without the
   schema-derived grammar, repair rounds, and how often the model branches on a `Deferred`. Count
   PIPr-dataset programs (Pulumi, CDK) that would hit ZHU-1, to size how much real code needs
   `.map` rather than `if`.

## 8. Sources

Confidence: high = primary source read or long-established; medium = abstract or secondary source
read; low = from memory, not checked this session.

- HashiCorp, terraform-cdk README, sunset of CDK for Terraform on 2025-12-10; env0, "What the CDKTF
  deprecation means for you". https://github.com/hashicorp/terraform-cdk (medium)
- Kon et al., "IaC-Eval: A Code Generation Benchmark for Cloud Infrastructure-as-Code Programs",
  NeurIPS 2024 Datasets and Benchmarks.
  https://proceedings.neurips.cc/paper_files/paper/2024/hash/f26b29298ae8acd94bd7e839688e329b-Abstract.html
  (medium, abstract)
- "IaC Generation with LLMs: An Error Taxonomy and A Study on Configuration Knowledge Injection",
  arXiv 2512.14792, 2025. https://arxiv.org/abs/2512.14792v1 (medium, abstract)
- Sokolowski, Spielmann, Salvaneschi, ProTI: automated unit testing of Pulumi programs, and "Towards
  Reliable Infrastructure as Code" (2023). https://proti-iac.github.io (medium)
- Sokolowski et al., "The PIPr Dataset of Public Infrastructure as Code Programs", MSR 2024.
  https://2024.msrconf.org/details/msr-2024-data-and-tool-showcase-track/26/The-PIPr-Dataset-of-Public-Infrastructure-as-Code-Programs
  (medium; dataset only, no findings cited)
- Firefly, "The State of IaC 2025". https://firefly.ai/state-of-iac-2025 (medium, vendor survey)
- The New Stack, "Wing: the startup failed, but the language has potential", April 2025.
  https://thenewstack.io/wing-the-startup-failed-but-the-language-has-potential/ (medium)
- kro documentation, ResourceGraphDefinition and CEL. https://kro.run/docs/overview (medium)
- HashiCorp, "Terraform Stacks explained". https://hashicorp.com/blog/terraform-stacks-explained
  (medium)
- Traugott and Brown, "Why Order Matters: Turing Equivalence in Automated Systems Administration",
  LISA 2002. (high, long-established; not re-read)
- Burgess, promise theory and CFEngine. (low)
- Burns, Grant, Oppenheimer, Brewer, Wilkes, "Borg, Omega, and Kubernetes", ACM Queue 2016. (high,
  long-established; not re-read)
- Xu and Zhou, "A Survey on Misconfiguration of Software Systems", ACM CSUR 2015; Xu et al., "Hey, You
  Have Given Me Too Many Knobs!", FSE 2015. (high, not re-read)
- Rahman, Parnin, Williams, "The Seven Sins: Security Smells in Infrastructure as Code Scripts", ICSE
  2019; Saavedra and Ferreira, "GLITCH", ASE 2022. (high, not re-read)
- Shambaugh, Weiss, Guha, "Rehearsal: A Configuration Verification Tool for Puppet", PLDI 2016.
  (medium, not re-read)
- Terraform `moved` (1.1), `import` blocks (1.5), `removed` blocks (1.7), ephemeral resources (1.10);
  Pulumi `aliases`, `Output.apply`, CrossGuard; AWS CDK Aspects; CUE, Pkl, KCL, Dhall, Nickel, Nix.
  (low to medium, from documentation knowledge, versions not re-checked)
