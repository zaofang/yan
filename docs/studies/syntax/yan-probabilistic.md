# Yan Probabilistic 言随 (`.ypb`): syntax and semantics study

Status: brainstorm study, 2026-11-04. Not normative. No spec rows, no diagnostic codes. Every code
block below is a mock that stays inside the one core; it becomes real only through an RFC and
accepted rows (see `CLAUDE.md`). The current spec anchor is R546 (research surface: "probabilistic
models with inference as handlers", a `Prob` capability in `@yan/prob` over seeded `rand`, `grad`
for gradient samplers) and RFC 0002 D11.

Contents: 1 methodology, 2 constraints from the core, 3 findings, 4 the five shared tasks, 5 the
five proposals, 6 the mix, 7 frameworks worth keeping, 8 open questions, 9 sources.

## 1. Methodology and process

1. **Read narrowly.** `docs/surfaces.md` section 15 (the D11 example and its mock grammar); RFC 0002
   D11 (idea, constraints, questions); spec 16.7 (R546 and admission rules N1 to N9); spec 7.1 and 7.3
   (user capabilities, handler binding, `rand` handlers and task-path streams, "no general
   continuations"); spec 11.5 (`grad` applies to pure functions only); spec 15.6 (plain Yan style);
   RFC 0001 M4 (Yan Meta's `x ~ d` with a `~t: Trace` parameter) and RFC 0029 O6 (index notation).
2. **Survey peers.** Stan, PyMC, Pyro and NumPyro, Turing.jl, Gen and GenJAX, WebPPL and Anglican,
   Birch, Infer.NET, BUGS and JAGS, brms and Bambi, Dice and ProbLog, LazyPPL, and the LLM-steering
   PPLs (LLaMPPL, GenLM). Web searches were used to confirm recent items (GenJAX at POPL 2026, Stan
   2.36 and 2.37, Turing's conditioning operator, SMC control of LLMs at ICLR 2025); the rest is from
   prior knowledge and is marked with a confidence level.
3. **Read the research.** Semantics (Staton; quasi-Borel spaces), typing (trace types, guide types),
   programmable inference (Gen), effect-handler PPLs (Pyro's Poutine), automatic marginalization
   (Funsor, tensor variable elimination), delayed sampling (Birch), workflow (Bayesian workflow, SBC),
   and the usability literature (Blackwell et al., PPIG 2019).
4. **Prototype.** A 60-line Python model of the semantics the mix relies on (models as functions over
   a `prob` handler, conditioning by address at the call site, randomness keyed by seed and address).
   Results in 3.5.
5. **Fix five tasks** that stress different parts of the design (section 4) and write each of them in
   all five proposals, so the proposals are compared on the same programs.
6. **Write five proposals**, each optimizing one axis and accepting its cost: brevity, explicitness,
   static analyzability, inference power, agent token cost.
7. **Measure** lexer-token counts of the same task across proposals and against Stan and PyMC (3.6),
   as a rough proxy for agent cost.
8. **Mix** by keeping, per design question, the answer from whichever proposal won it, then rewrite the
   five tasks in the mix to check the parts fit.

## 2. Constraints from the core

These are not choices; any proposal must live inside them.

- **One core (N1).** Every form lowers to Yan Core. The preferred route is lowering, not an engine.
  A model is therefore ordinary Yan code plus calls into `@yan/prob`.
- **Capabilities.** `sample` is an operation of a user capability `Prob` (`uses prob`). Built-in
  capability operations are tail-resumptive, and there are **no general continuations**; generators
  are the only control handlers (7.3, 10.5). Multi-shot continuations, which WebPPL and Anglican use
  for enumeration and SMC, are not available. R522 (one-shot lexical handlers) is research only.
- **Determinism.** Randomness comes from `rand`, seeded ChaCha8 with streams keyed by task path (7.7).
  Parallel chains are tasks, so four chains with seed 7 give the same draws on every machine.
- **`grad` needs purity.** `grad` applies to functions with no capability besides `sched` (11.5). A
  model that `uses prob` is not differentiable as written; HMC needs a separate pure log-density.
- **Five facts visible (N5)** and **one printing (N3)**: the effect must show in the text; the surface
  needs one canonical spelling per form.
- **Yan Meta M4** already spells `x ~ d` with a `~t: Trace` parameter, sampling when `x` is unbound and
  observing when bound. D11 asks that Yan Probabilistic and Yan Meta share one library and agree on
  capability versus parameter.

## 3. Findings

### 3.1 Peers

| System | Model of a model | What works | What does not | Confidence |
| --- | --- | --- | --- | --- |
| Stan | Declarative blocks (`data`, `parameters`, `transformed`, `model`, `generated quantities`) compiled to a log density | NUTS that just works on continuous models; constrained types (`real` with a `lower=0` bound, `simplex`, and since 2.36 `sum_to_zero_vector`, stochastic matrices); best diagnostics culture | No discrete parameters (marginalize by hand); verbose blocks; data and parameters fixed in the program, so prior predictive needs a second program; C++ compile times | High |
| PyMC | Python context manager builds a graph | `dims`/`coords` named dimensions are loved; ArviZ InferenceData; samplers (nutpie, NumPyro backends) | Shape errors; `observed=` inside the model fixes what is data; graph versus Python confusion | High |
| Pyro, NumPyro | Python functions with effect handlers (`sample`, `plate`, `condition`, `trace`, `replay`) | Inference as composable handlers; `plate` for conditional independence and subsampling; enumeration of discrete sites | `plate` dim bookkeeping (negative dims) is a top complaint; addresses are strings, so typos are silent | High |
| Turing.jl | `@model` macro, `~` statements | `model \| (x = data,)` conditions at the call site, so one model gives prior, posterior and predictive; Gibbs composition of samplers | Macro magic; type instability costs speed; whether `x ~ d` observes depends on whether `x` is an argument | High |
| Gen, GenJAX | Generative functions with addressed choices (`{:x} ~ normal(0, 1)`); choice maps | Programmable inference: custom proposals, MH/SMC kernels, `update`; GenJAX vectorizes inference (POPL 2026) | High learning cost; inference code longer than the model | High |
| WebPPL, Church, Anglican | Functional, continuation-based | Exact enumeration, SMC and nested inference via continuations | Needs multi-shot continuations (not in Yan); slow | High |
| Birch | Object-oriented, delayed sampling | Automatic Rao-Blackwellization of conjugate pieces in SMC | Small community | Medium |
| BUGS, JAGS | Declarative graphical model, order-free | Statisticians read it at once | Gibbs only; no general programs | High |
| brms, Bambi | Formula strings `y ~ x + (1 \| g)` compiled to Stan or PyMC | The most productive interface for regression users | Only regression-shaped models | High |
| Dice, ProbLog | Discrete, exact inference by knowledge compilation | Exact answers for discrete programs | Discrete only | Medium |
| LazyPPL | Haskell, lazy infinite structures | Nonparametrics (Dirichlet processes) as plain lazy values | Research | Medium |
| LLaMPPL, GenLM | LLM token streams as distributions, SMC steering | Constraint satisfaction at generation time beats rejection and plain constrained decoding on several tasks (Loula et al., ICLR 2025) | Young; cost model unclear | Medium |

### 3.2 What users want

From the usability literature and the recurring themes on the Stan and PyMC forums (medium
confidence; the forum themes are an aggregate impression, not a counted survey):

1. **One model, many questions.** Prior predictive, posterior, posterior predictive, and simulation
   from the same text. This is the core of the Bayesian workflow (Gelman et al. 2020). Stan forces
   copies; Turing's `|` is the most praised answer.
2. **Named dimensions.** PyMC's `dims` and xarray-backed results. Shape bugs are the most common bug
   class in tensor PPLs.
3. **Discrete latents without hand marginalization.** Mixtures and HMMs in Stan need `log_sum_exp`
   by hand.
4. **Reparameterization help.** Divergences in hierarchical models are fixed by the non-centered form;
   users want it to be one word, not a rewrite.
5. **Diagnostics that stop you.** R-hat, ESS and divergences reported loudly, and simulation-based
   calibration (SBC) as a test.
6. **Speed and short compile times.**
7. **Reproducibility.** Same seed, same draws, across machines and thread counts.
8. **Formula notation** for the regression majority.

### 3.3 What the research says

- **Semantics.** Programs with `sample` and `score` denote s-finite measures (Staton 2016); higher-order
  programs fit quasi-Borel spaces (Heunen et al. 2017). The practical lesson: conditioning is a
  weight, and a model is a measure-valued function, so `given` and `factor` are enough primitives.
  Confidence high.
- **Trace types** (Lew et al., POPL 2020) give each model a type of its trace (which addresses exist,
  with what supports) and use it to check that an inference program is sound, for example that a
  proposal covers the model's support. **Guide types** (Wang, Hoffmann, Reps, PLDI 2021) do the same
  for variational guides. Confidence high. This is the most underused idea in production PPLs.
- **Programmable inference** (Gen, Cusumano-Towner et al. PLDI 2019; GenJAX, POPL 2026): separate
  model, inference program and proposals; vectorize inference. Confidence high.
- **Effect handlers for PPLs** (Pyro's Poutine, Bingham et al. JMLR 2019): `condition`, `trace`,
  `replay`, `block`, `scale` are all handlers, which matches Yan's capability design exactly.
- **Automatic marginalization** (Funsor and tensor variable elimination, Obermeyer et al. ICML 2019)
  makes finite discrete latents work with HMC. **Delayed sampling** (Murray et al. AISTATS 2018)
  marginalizes conjugate pairs automatically in SMC. Confidence high.
- **Workflow.** Bayesian workflow (Gelman et al. 2020); SBC (Talts et al. 2018; Modrák et al. 2023);
  Pathfinder for initialization (Zhang et al. JMLR 2022). Confidence high.
- **Compiling declarative to generative.** Baudart et al. (PLDI 2021) compile Stan to Pyro and show
  that Stan's blocks are a subset of generative models, with care around improper priors and `target
  +=`. So a generative surface can cover Stan's models. Confidence high.
- **Splittable randomness** (Claessen and Pałka 2013; JAX keys): keys split by path give reproducible
  parallel randomness. Yan already has this by task path.
- **Usability** (Blackwell et al., PPIG 2019): users' mental model is the generative story; notations
  that read as "data is generated by" are learned fastest. Confidence medium.

### 3.4 What works and what does not (synthesis)

- `~` as "is distributed as" is the notation every statistician already reads. Keep it.
- Deciding observe versus sample from **binding** (Turing arguments, Yan Meta M4) is the worst
  ambiguity in the field: the same line changes meaning when a caller passes a value.
- Deciding it **inside the model** (`observed=` in PyMC, `data` in Stan) welds the data to the model
  and kills prior predictive checks.
- Deciding it **at the call site** (`model | data` in Turing, `condition` in Pyro) is the best design,
  and it is weak in every system only because the data map is untyped: a misspelled key silently
  turns an observation into a latent.
- Plates and vectorization are a shape problem. Yan already has labelled axes (spec 11), so plates can
  be axes, not dimension indices.
- Supports and constraints (positive, simplex, ordered) are types in Stan and runtime arguments
  elsewhere. Yan has refinements and a solver (8.6), so supports can be types.

### 3.5 Prototype

[`proto.py`](yan-probabilistic-proto/proto.py) (60 lines) models a `prob` handler with `sample(address, dist)`; a
`given` map turns a sample into an observation that adds `logp` to the weight; each draw's
randomness is keyed by hash(seed, address). Results:

- Importance sampling of the coin model with 4000 draws, seed 7: posterior mean 0.6755 against exact
  4/6 = 0.6667, and the rerun is identical bit for bit.
- The prior draw of `p` is the same with and without conditioning, because its stream is keyed by its
  address. This gives common random numbers across prior and posterior runs and makes trace-replay
  MH cheap: changing one address does not reshuffle the others.
- A misspelled key (`flip[0]` for `flips[0]`) is ignored silently, as in every dictionary-based PPL.
  A trace type rejects it at compile time; this is the case for typed conditioning in the mix.

### 3.6 Token counts

Lexer tokens for task T3 (eight schools, non-centered), counted by
[`count.py`](yan-probabilistic-proto/count.py): identifiers, numbers and punctuation are one token each; comments and print lines are dropped;
the inference call is included. A rough proxy for agent cost: a model tokenizer gives different
absolute numbers.

| Version | Tokens | Note |
| --- | --- | --- |
| Stan | 102 | model only; the CmdStanPy call adds about 15 |
| PyMC | 122 | includes `pm.sample` |
| P1 Tilde | 93 | |
| P2 Ledger | 141 | |
| P3 Graph | 91 | |
| P4 Loom | 87 | |
| P5 Lean | 59 | |
| Mix | 89 | |

The Stan and PyMC versions are the standard ones from their documentation, rewritten from memory.
Two lessons. First, the winner by far is P5, and the gap comes from dropping parentheses and commas
(juxtaposition), which the mix rejects for N3 and N8 reasons; nothing else in P5 is cheaper. Second,
the mix is about 25 percent smaller than Stan plus its call and 27 percent smaller than PyMC, almost
all of it from removing declarations (`data`, `parameters`, types and bounds), which the mix infers.
P1's suffix plates cost more than they save.

## 4. The five shared tasks

- **T1 Coin.** Beta-Bernoulli bias from flips. The smallest model; checks the observe spelling.
- **T2 Regression with units.** Stopping distance against speed, Student-t noise, with physical units.
  Checks covariates versus outcomes, units, and robust likelihoods.
- **T3 Eight schools.** Hierarchical, non-centered. Checks plates, named axes, reparameterization.
- **T4 Mixture.** Two-component Gaussian mixture with a discrete assignment per point. Checks discrete
  latents with HMC.
- **T5 Tracking.** Local-level state-space model filtered online with SMC. Checks loops with dynamic
  addresses, streaming inference, and generator lowering.

Each task ends with the inference call and one use of the result.

## 5. Five proposals

### P1 Tilde: brevity (math on the page)

Axis: fewest characters, closest to the blackboard. Statements are `name ~ dist`; observed or not is
decided by binding (Yan Meta M4's rule); plates are comprehension-like `for` suffixes. Cost: the
binding rule is the ambiguity 3.4 warns about, and the meaning of a line depends on the caller.

```
model coin(flips: List[bool]) -> float =
  p ~ beta(1, 1)
  flips[i] ~ bernoulli(p) for i
  p

let post = coin(data) |> is(draws: 2000, seed: 7)
io.print("\(post.p.mean())")
```

```
model stop(v: Array[Obs, km/h], d: Array[Obs, m]) =
  a ~ normal(0 m, 10 m)
  b ~ normal(0 m*h/km, 1 m*h/km)
  s ~ half_normal(5 m)
  d[i] ~ student_t(4, a + b * v[i], s) for i

let post = stop(v, d) |> nuts(seed: 7)
```

```
model schools(y: Array[School, float], sigma: Array[School, float]) =
  mu ~ normal(0, 5)
  tau ~ half_cauchy(5)
  eta[s] ~ normal(0, 1) for s
  y[s] ~ normal(mu + tau * eta[s], sigma[s]) for s

let post = schools(y, sigma) |> nuts(chains: 4, seed: 7)
```

```
model mix(y: Array[Obs, float]) =
  w ~ dirichlet([1, 1])
  mu ~ ordered_normal(0, 10, 2)
  s ~ half_normal(2)
  z[n] ~ categorical(w) for n
  y[n] ~ normal(mu[z[n]], s) for n

let post = mix(y) |> nuts(seed: 7)
```

```
model level(y: List[float]) =
  q ~ half_normal(0.5)
  x[0] ~ normal(0, 10)
  x[t] ~ normal(x[t-1], q) for t in 1..y.len()
  y[t] ~ normal(x[t], 1) for t

for b in level(y) |> smc(particles: 2000, seed: 7)
  io.print("\(b.x.last().mean())")
```

Won: `~` plus index plates read like a paper. Lost: observe-by-binding; `|>` is new syntax for one
use; the `for s` suffixes made it no shorter than P3 (3.6).

### P2 Ledger: explicitness and safety

Axis: nothing implicit. Every random choice is `sample` or `observe` with a written address string;
supports are written in the type; the capability is in the signature; inference names its handler.
This is the D11 mock grammar taken to its end. Cost: about 50 percent more tokens than P1, and data
is still welded into the model.

```
model coin(flips: List[bool]) -> float in [0.0, 1.0] uses prob =
  let p: float in [0.0, 1.0] = sample beta(1.0, 1.0) as "p"
  for (i, f) in flips.enumerate()
    observe bernoulli(p) == f as "flips[\(i)]"
  p

let post = with prob = importance(draws: 2000, seed: 7)
  posterior(coin(data))
```

```
model stop(v: Array[Obs, km/h], d: Array[Obs, m]) -> (m, m*h/km) uses prob =
  let a: m = sample normal(0.0 m, 10.0 m) as "a"
  let b: m*h/km = sample normal(0.0 m*h/km, 1.0 m*h/km) as "b"
  let s: m in (0.0 m, inf) = sample half_normal(5.0 m) as "s"
  for i in v.axis(Obs)
    observe student_t(4.0, a + b * v[i], s) == d[i] as "d[\(i)]"
  (a, b)
```

```
model schools(y: Array[School, float], sigma: Array[School, float]) uses prob =
  let mu: float = sample normal(0.0, 5.0) as "mu"
  let tau: float in (0.0, inf) = sample half_cauchy(5.0) as "tau"
  for s in School
    let eta: float = sample normal(0.0, 1.0) as "eta[\(s)]"
    observe normal(mu + tau * eta, sigma[s]) == y[s] as "y[\(s)]"

let post = with prob = nuts(chains: 4, seed: 7)
  posterior(schools(y, sigma))
```

```
model mix(y: Array[Obs, float]) uses prob =
  let w: Simplex[2] = sample dirichlet([1.0, 1.0]) as "w"
  let mu: Ordered[2] = sample ordered_normal(0.0, 10.0, 2) as "mu"
  let s: float in (0.0, inf) = sample half_normal(2.0) as "s"
  for n in y.axis(Obs)
    let z: int in 0..2 = sample enumerated categorical(w) as "z[\(n)]"
    observe normal(mu[z], s) == y[n] as "y[\(n)]"
```

```
model level(y: List[float]) uses prob =
  let q: float in (0.0, inf) = sample half_normal(0.5) as "q"
  var x: float = sample normal(0.0, 10.0) as "x[0]"
  for t in 0..y.len()
    if t > 0
      x = sample normal(x, q) as "x[\(t)]"
    observe normal(x, 1.0) == y[t] as "y[\(t)]"
    checkpoint
```

Won: every effect, address, support and the enumeration choice are visible; easiest to audit and
review; `checkpoint` makes SMC resampling points explicit. Lost: string addresses are typo-prone;
verbose; welded data.

### P3 Graph: static analyzability and speed

Axis: the compiler sees the whole model as a graph. Order-free equations as in BUGS and Stan, typed
`param` and `data` declarations, no control flow over random choices. Every model is static, so every
model gets a density, NUTS, automatic marginalization and vectorization. Cost: no open-universe models,
no dynamic structure; T5 needs a different form (a `scan`).

```
model coin
  data flips: List[bool]
  param p: float in [0, 1]
  p ~ beta(1, 1)
  flips ~ bernoulli(p)
```

```
model stop
  axis Obs
  data v: [Obs] km/h
  data d: [Obs] m
  param a: m
  param b: m*h/km
  param s: m > 0
  a ~ normal(0 m, 10 m)
  b ~ normal(0 m*h/km, 1 m*h/km)
  s ~ half_normal(5 m)
  d ~ student_t(4, a + b * v, s)
```

```
model schools
  axis School
  data y, sigma: [School] float
  param mu: float
  param tau: float > 0
  param eta: [School] float
  mu ~ normal(0, 5)
  tau ~ half_cauchy(5)
  eta ~ normal(0, 1)
  y ~ normal(mu + tau * eta, sigma)

let post = fit(schools, data(y:, sigma:), nuts(chains: 4, seed: 7))
```

```
model mix
  axis Obs
  data y: [Obs] float
  param w: simplex[2]
  param mu: ordered[2]
  param s: float > 0
  latent z: [Obs] 0..2
  z ~ categorical(w)
  y ~ normal(mu[z], s)
```

```
model level
  axis Time
  data y: [Time] float
  param q: float > 0
  param x: [Time] float
  x ~ random_walk(normal(0, 10), q)
  y ~ normal(x, 1)
```

Won: declarative and order-free; `param` versus `data` is explicit; whole-model vectorization
(`y ~ normal(mu + tau * eta, sigma)` over the axis) is the shortest correct plate. Lost: data and
parameters are fixed in the text (prior predictive needs another model); no general programs; T5
only works offline, not as a filter.

### P4 Loom: inference power

Axis: what you can do with a model. Models are generative functions (Gen) with typed addresses;
traces are first-class values; inference programs are Yan code composed from kernels; proposals are
models too. Cost: the inference layer is a second language users must learn; models are short but
programs that use them are long.

```
gen coin(n: int) -> float =
  p ~ beta(1, 1)
  for i in 0..n
    flips[i] ~ bernoulli(p)
  p

let post = importance(coin(4), obs(flips: data), draws: 2000, seed: 7)
```

```
gen stop(v: Array[Obs, km/h]) =
  a ~ normal(0 m, 10 m)
  b ~ normal(0 m*h/km, 1 m*h/km)
  s ~ half_normal(5 m)
  for i in v.axis(Obs)
    d[i] ~ student_t(4, a + b * v[i], s)

let post = nuts(stop(v), obs(d:), chains: 4, seed: 7)
```

```
gen schools(sigma: Array[School, float]) =
  mu ~ normal(0, 5)
  tau ~ half_cauchy(5)
  for s in School
    eta[s] ~ normal(0, 1)
    y[s] ~ normal(mu + tau * eta[s], sigma[s])

let post = nuts(schools(sigma), obs(y:), chains: 4, seed: 7)
```

```
gen mix(n: int) =
  w ~ dirichlet([1, 1])
  mu ~ ordered_normal(0, 10, 2)
  s ~ half_normal(2)
  for i in 0..n
    z[i] ~ categorical(w)
    y[i] ~ normal(mu[z[i]], s)

let k = gibbs(enumerate(on: .z), hmc(on: .w, .mu, .s, steps: 10))
let post = mcmc(mix(y.len()), obs(y:), k, iters: 2000, seed: 7)
```

```
gen level(steps: int) =
  q ~ half_normal(0.5)
  x[0] ~ normal(0, 10)
  y[0] ~ normal(x[0], 1)
  for t in 1..steps
    x[t] ~ normal(x[t-1], q)
    y[t] ~ normal(x[t], 1)

gen propose_x(tr: Trace[level], t: int) =
  x[t] ~ normal(tr.x[t-1] + 0.5 * (tr.y[t] - tr.x[t-1]), 0.5)

for b in smc(level, obs(y:), proposal: propose_x, particles: 2000, rejuvenate: mh(on: .q), seed: 7)
  io.print("\(b.x.last().mean())")
```

Won: conditioning at the call site with typed addresses (`obs(y:)` is checked against the trace
type); kernel algebra (`gibbs`, `enumerate`, `hmc(on: ...)`, `mh(on: ...)`); custom proposals checked
against the model's trace type (trace types make that sound). Lost: two languages to learn.

### P5 Lean: agent token cost and tooling

Axis: fewest tokens for an agent, LL(1) parse, one spelling, grammar small enough to export as a
GBNF/llguidance grammar for constrained decoding (R567). Every line starts with a keyword or a name;
plates are an axis suffix on the name; distributions are positional. Cost: terse to the point of
being cryptic for humans (the `@` plate and the single-letter defaults).

```
model coin(n: int) -> float
  p ~ beta 1 1
  flips@n ~ bernoulli p
  p

post = coin(4) given flips: data by is draws: 2000 seed: 7
```

```
model stop(v: Array[Obs, km/h])
  a ~ normal 0m 10m
  b ~ normal 0m*h/km 1m*h/km
  s ~ half_normal 5m
  d@Obs ~ student_t 4 (a + b * v) s
```

```
model schools(sigma: Array[School, float])
  mu ~ normal 0 5
  tau ~ half_cauchy 5
  eta@School ~ normal 0 1
  y@School ~ normal (mu + tau * eta) sigma

post = schools(sigma) given y: y by nuts chains: 4 seed: 7
```

```
model mix(n: int)
  w ~ dirichlet [1 1]
  mu ~ ordered_normal 0 10 2
  s ~ half_normal 2
  z@n ~ categorical w
  y@n ~ normal mu[z] s
```

```
model level(steps: int)
  q ~ half_normal 0.5
  x@steps ~ walk (normal 0 10) q
  y@steps ~ normal x 1

each b in level(y.len()) given y: y by smc particles: 2000 seed: 7
  print b.x.last.mean
```

Won: `given ... by ...` as one call-site phrase reads well and tokenizes small; axis-suffixed plates
are the shortest correct vectorization. Lost: juxtaposition calls diverge from every other Yan
surface (N3 and N8 friction), and humans misread `mu[z] s`.

### Scorecard

| Question | P1 Tilde | P2 Ledger | P3 Graph | P4 Loom | P5 Lean | Winner |
| --- | --- | --- | --- | --- | --- | --- |
| Where observation is decided | binding | in model | `data` decl | call site, typed | call site | P4 |
| Addresses | name | string | name | typed name | name | P4 |
| Plates | `for i` suffix | loop | whole-axis | loop | axis suffix | P3 (whole-axis) and P1 (index) |
| Supports | inferred | written | written | inferred | inferred | inferred, printable as P2 |
| Discrete latents | implicit | `enumerated` | `latent` | kernel | implicit | automatic, P3 |
| Dynamic structure | yes | yes | no | yes | partial | P4 |
| Inference spelling | `\|>` | `with prob =` | `fit(...)` | call | `given ... by` | P5 phrase, as a call |
| Tokens on T3 | 93 | 141 | 91 | 87 | 59 | P5 |

## 6. The mix: Yan Probabilistic as one generative story

### 6.1 The idea in one paragraph

A model is the generative story and nothing else. Every random quantity is a `~` line with a name;
the name is its address. A model never says what is observed: the caller says so with `given`, which
is checked against the model's trace type, so a misspelled name is a compile error. Plates are Yan's
index notation over labelled axes. Supports are types, inferred from the distribution. The compiler
gives every model three readings: a generative function under the `prob` capability (for simulation,
importance sampling, SMC and MH), a pure log-density over a derived parameter record (so `grad`
applies and NUTS, VI and Pathfinder work), and a trace type (so conditioning, proposals and posteriors
are typed). Inference algorithms are handlers or density consumers, chosen at the call site. Same
seed, same draws, everywhere.

### 6.2 Forms

**MOCK grammar** (extends plain Yan statements):

```
model    = ["pub"] "model" NAME [tparams] "(" [params] ")" ["->" type] "=" NEWLINE
           INDENT {mstmt} [expr NEWLINE] DEDENT
mstmt    = site "~" expr NEWLINE                         -- a random choice
         | "factor" expr NEWLINE                         -- add a log weight
         | yan-stmt                                      -- let, var, for, if, match
site     = NAME {"[" index {"," index} "]"}
index    = NAME [":" type] | expr                        -- binder (plate) or value (address)
given    = expr "given" "(" field {"," field} ")"         -- condition a model by site name
```

Rules:

1. **`~` has one meaning.** `x ~ d` declares a random choice named `x` with distribution `d`. It never
   observes and never rebinds; a second `x ~` in the same scope is an error (the Yan Meta
   `ymt-trace-rebind` rule, kept).
2. **Observation is `given`.** `m given (y: ys)` conditions model value `m` on site `y`. Field names
   are checked against the trace type of `m`; types and supports are checked too (`given (tau: -1.0)`
   fails at compile time when the value is a literal, at the boundary otherwise). Conditioning a
   latent at a fixed value (`given (tau: 2.0)`) is allowed and is how do-style clamping and
   sensitivity analysis are spelled.
3. **Plates are index notation.** `eta[s: School] ~ normal(0.0, 1.0)` binds `s` over the axis
   `School` and declares conditionally independent draws; an index that a right-side use already
   ranges (`y[s] ~ normal(theta[s], sigma[s])`) needs no annotation (RFC 0029 O6's rule: an index
   bound on the left is free on the right, and its range is the intersection of the uses). The site
   `eta` then has type `Array[School, float]`. A site indexed by a value inside a loop
   (`x[t] ~ ...` in `for t in ...`) is a dynamic address.
4. **Supports are types.** The site's type is the distribution's support: `tau ~ half_cauchy(5.0)`
   gives `tau: float in (0.0, inf)`, `w ~ dirichlet(...)` gives `Simplex[K]`. The density reading
   applies the matching bijection (log, stick-breaking, ordered) automatically. `yan view` prints the
   inferred support, so P2's explicitness is one command away.
5. **Units flow through.** Distributions are generic over `Quantity` (spec 11): `normal(0.0 m,
   10.0 m)` has support `m`; adding a slope in `m*h/km` to a speed in `km/h` checks, and a prior with
   the wrong unit is a type error. No production PPL checks units; this is free in Yan.
6. **Models are pure except `prob`.** A model may use no capability besides `prob` (and `sched` for
   `par` inside it). The `model` keyword is the N5 mark for `prob`, so it is not repeated in the
   signature. Purity is what makes the density reading legal for `grad` and makes replay exact.
7. **`factor e`** adds `e` to the log weight: censoring, soft constraints, Stan's `target +=`.
8. **Static or dynamic.** A model is static when its set of sites depends only on its arguments (no
   `~` under a branch or loop bound that depends on a random value): a decidable, conservative check
   in the style of N2. A static model has a record trace type and a density; a dynamic one has a
   choice-map trace type and no density. `nuts` on a dynamic model is a compile error that names the
   first offending branch.
9. **Discrete sites in static models** with finite support are marginalized automatically in the
   density reading (variable elimination over the plate structure) and drawn back from their
   conditionals in the posterior, so T4 needs no hand `log_sum_exp`.
10. **Address-keyed randomness.** The stream for a site is the `rand` stream keyed by seed, task path
    and site address (an extension of 7.7's task-path rule). Prior and posterior runs share draws at
    unchanged addresses (3.5); chains are tasks, so `chains: 4` is reproducible.
11. **The model is a value.** Calling a model returns `Model[T, R]` (`T` the trace type, `R` the
    return type) without running it. Methods: `.given(...)` (the keyword form's lowering),
    `.simulate(seed:)`, `.logp(trace)`, `.predict(post)`. `infer(m, alg)` stays a call, as
    `docs/surfaces.md` already says.
12. **Posteriors are labelled arrays.** `Posterior[T]` has one field per site with extra leading axes
    `Chain` and `Draw`, so `post.theta.mean(over: Draw)` and `post.tau.quantile(0.9)` are ordinary
    spec 11 array operations, and `post.table()` is a `Table` (Arrow) for export to ArviZ.
13. **Diagnostics fail loudly.** `infer` returns `Posterior[T] fail FitErr`; divergences over a
    threshold, R-hat over 1.01 or ESS under a floor are failures, not warnings, unless the call opts
    out (`nuts(check: lenient)`). Opinionated, and the point.

### 6.3 Lowering (sketch)

- `x ~ d` lowers to `let x = prob.sample(Addr("x"), d)`; `x[i] ~ d` inside a loop lowers to
  `prob.sample(Addr("x", i), d)`; a plate `eta[s: School] ~ d` lowers to one vectorized call
  `prob.sample_plate(Addr("eta"), School, d)`.
- `m given (y: v)` lowers to `condition(m, Obs.y(v))`, where `Obs` is a record type generated from the
  trace type (N4: a fixed naming rule, no new hashed names beyond it).
- The handler `condition` answers `sample` at an observed address with the given value and adds
  `d.logp(v)` to the weight; elsewhere it forwards. All `prob` operations are tail-resumptive, which
  is all handlers need here.
- The density reading is a stage-0 transform of a static model into a pure function
  `logp(theta: Params, obs: Obs) -> float` over unconstrained `theta`, with Jacobian terms from rule 4.
  `grad` (11.5) applies because the function has no capability.
- SMC uses a generator lowering: the model becomes a generator that yields at each observed site with
  its incremental weight; particles are generator values. Resampling needs copying a generator's
  state (open question 8.3).

### 6.4 The five tasks in the mix

T1:

```
model coin(n: int) -> float =
  p ~ beta(1.0, 1.0)
  flips[i: 0..n] ~ bernoulli(p)
  p

main() uses io =
  let post = infer(coin(4) given (flips: [True, True, False, True]), importance(draws: 2000, seed: 7))?
  io.print("mean \(post.p.mean())")
```

T2:

```
model stop(v: Array[Obs, km/h]) =
  a ~ normal(0.0 m, 10.0 m)
  b ~ normal(0.0 m*h/km, 1.0 m*h/km)
  s ~ half_normal(5.0 m)
  d[i] ~ student_t(4.0, a + b * v[i], s)

let post = infer(stop(v) given (d: dist), nuts(seed: 7))?
let prior = stop(v).simulate(seed: 7)          -- prior predictive from the same text
```

T3:

```
model schools(sigma: Array[School, float]) =
  mu ~ normal(0.0, 5.0)
  tau ~ half_cauchy(5.0)
  eta[s: School] ~ normal(0.0, 1.0)
  y[s] ~ normal(mu + tau * eta[s], sigma[s])

let post = infer(schools(sigma) given (y: scores), nuts(chains: 4, seed: 7))?
io.print("tau 90%: \(post.tau.quantile(0.9))")
```

T4:

```
model mix(n: int) =
  w ~ dirichlet([1.0, 1.0])
  mu ~ ordered_normal(0.0, 10.0, 2)
  s ~ half_normal(2.0)
  z[i: 0..n] ~ categorical(w)                  -- finite support: marginalized for nuts
  y[i] ~ normal(mu[z[i]], s)

let post = infer(mix(y.len()) given (y: y), nuts(seed: 7))?
let labels = post.z.mode(over: Draw)
```

T5:

```
model level(steps: int) -> float =
  q ~ half_normal(0.5)
  for t in 0..steps
    x[t] ~ normal(if t == 0 then 0.0 else x[t - 1], if t == 0 then 10.0 else q)
    y[t] ~ normal(x[t], 1.0)
  x[steps - 1]

for belief in filter(level(ys.len()) given (y: ys), smc(particles: 2000, seed: 7))
  io.print("level \(belief.ret.mean())")
```

T5 is dynamic (sites indexed by a loop), so `nuts` would be refused and `smc` is the natural choice;
`filter` is a generator of per-step posteriors (spec 10.5), so the filter is online with no new
control feature. (How a later line reads a site, `x[t - 1]` here, is open question 8.5.)

### 6.5 What came from where

| Decision | From | Why it won |
| --- | --- | --- |
| `~` as the only random-choice spelling | P1 | the shortest form every statistician reads |
| Observation at the call site, typed | P4 (and Turing's `\|`) | one model for every workflow question; typos are compile errors |
| `given` as a word | P5 | reads as English, no operator overloading of `\|` |
| Index notation plates over axes | P1 and P3, via RFC 0029 O6 | vectorization and conditional independence in one form, no dim bookkeeping |
| Supports as inferred types, printable | P1, with P2's view | short to write, explicit to read |
| Automatic discrete marginalization | P3 | the most requested missing feature in Stan |
| Static/dynamic split by a decidable check | P3 and P4 | static models get NUTS; dynamic models still exist |
| Kernels and proposals | P4, deferred | power for experts, not in the first slice |
| Diagnostics as failures | P2 | safety culture, matches Yan's `fail` |
| Units in priors | new | free from spec 11 |

## 7. Frameworks worth keeping

1. **One story, three readings.** Generative function, pure density, trace type, all derived from one
   text. The surface stays small because the compiler, not the user, writes the other two.
2. **Conditioning is a typed call-site operation.** The model is a simulator; data is applied, not
   declared. This alone fixes prior predictive checks, SBC, sensitivity analysis and typo bugs.
3. **Plates are axes.** Conditional independence, vectorization and subsampling all come from the
   index binder, and labelled axes make shape bugs type errors.
4. **Addresses are names, randomness is keyed by address.** Reproducibility, cheap MH, and common
   random numbers for comparing models, from the same rule Yan already uses for tasks.
5. **Workflow is in the test system.** `test "recovers tau" = sbc(schools(sigma), nuts(), sims: 200)`
   and prior predictive checks as `ex` examples: deterministic, cacheable, gated in CI.
6. **Vanguard: model calls as distributions.** With the `model` capability, `text ~ llm(prompt)` could
   be a site whose distribution is a language model, and `given` plus `factor` steer it by SMC
   (Loula et al. 2025). Yan's record and replay make such runs reproducible. A later RFC, not the
   first slice.

## 8. Open questions

1. **Yan Meta M4 alignment.** M4 observes by binding with a `~t: Trace` parameter. The mix argues for
   the capability plus `given`. Proposal for the Yan Probabilistic RFC: Yan Meta drops observe-by-binding
   and uses the same `given`, keeping `~t: Trace` only as the explicit form of the capability.
2. **Static check precision.** How conservative may the static predicate be before users fight it?
   Stan's experience suggests most real models are static; the check should accept `if` on data.
3. **Copying generators for SMC.** Resampling duplicates particles. Is generator state a value that can
   be cloned (it is a state machine after lowering), and what does that do to `@linear` captures?
4. **Improper priors and `factor`.** Baudart et al. show these break the generative reading. Forbid
   improper priors, or allow them only in static models?
5. **Reading a site.** In T5, `x[t - 1]` reads an earlier site. Options: a site is also a local of
   its array type (one namespace, so a local `x` and a site `x` cannot coexist), or sites are read
   only through their indexed form.
6. **Variational guides.** Guide types (Wang et al. 2021) fit as a second model whose trace type must
   cover the first; spelling `guide for schools` is untried here.
7. **Default inference.** Should `infer(m)` with no algorithm pick NUTS for static models and SMC for
   dynamic ones? Convenient, but it hides a decision the diagnostics then have to explain.
8. **Formula sugar.** A `regress y ~ 1 + x + (1 | g)` form for brms users: inside this surface, or a
   std helper that builds a model value? It would be the most used form by count.
9. **Engine pressure.** Automatic marginalization and the density transform are stage-0 work over
   checked Yan Core. If they outgrow lowering, R568 allows an engine; this study assumes they do not.

## 9. Sources

- Stan Reference Manual and release notes: CmdStan 2.36 (https://blog.mc-stan.org/2024/12/10/release-of-cmdstan-2-36/),
  CmdStan 2.37 (https://blog.mc-stan.org/2025/09/02/release-of-cmdstan-2-37/).
- PyMC documentation (dims and coords); ArviZ InferenceData.
- Bingham et al., "Pyro: Deep Universal Probabilistic Programming", JMLR 2019.
- Turing.jl and DynamicPPL, the probability interface and `condition` (https://turinglang.github.io/DynamicPPL.jl/stable/tutorials/prob-interface/).
- Cusumano-Towner et al., "Gen: a general-purpose probabilistic programming system with programmable
  inference", PLDI 2019.
- Becker et al., "Probabilistic Programming with Vectorized Programmable Inference" (GenJAX), POPL 2026
  (https://popl26.sigplan.org/details/POPL-2026-popl-research-papers/89/Probabilistic-Programming-with-Vectorized-Programmable-Inference).
- Lew et al., "Trace Types and Denotational Semantics for Sound Programmable Inference in Probabilistic
  Languages", POPL 2020 (https://popl20.sigplan.org/details/POPL-2020-Research-Papers/34/Trace-Types-and-Denotational-Semantics-for-Sound-Programmable-Inference-in-Probabilis).
- Wang, Hoffmann, Reps, "Sound Probabilistic Inference via Guide Types", PLDI 2021.
- Staton, "Commutative semantics for probabilistic programming", ESOP 2017; Heunen et al., "A convenient
  category for higher-order probability theory", LICS 2017.
- Obermeyer et al., "Tensor Variable Elimination for Plated Factor Graphs", ICML 2019.
- Murray et al., "Delayed Sampling and Automatic Rao-Blackwellization of Probabilistic Programs",
  AISTATS 2018.
- Baudart et al., "Compiling Stan to Generative Probabilistic Languages and Extension to Deep
  Probabilistic Programming", PLDI 2021.
- Gelman et al., "Bayesian Workflow", 2020; Talts et al., "Validating Bayesian Inference Algorithms with
  Simulation-Based Calibration", 2018; Modrák et al., "Simulation-Based Calibration Checking for
  Bayesian Computation", Bayesian Analysis 2023.
- Zhang et al., "Pathfinder: Parallel quasi-Newton variational inference", JMLR 2022; Hoffman and
  Gelman, "The No-U-Turn Sampler", JMLR 2014; Kucukelbir et al., "Automatic Differentiation Variational
  Inference", JMLR 2017.
- Claessen and Pałka, "Splittable pseudorandom number generators using cryptographic hashing",
  Haskell 2013.
- Blackwell et al., "Usability of Probabilistic Programming Languages", PPIG 2019
  (https://ppig.org/papers/2019-ppig-30th-blackwell/).
- Lew et al., "Sequential Monte Carlo Steering of Large Language Models using Probabilistic Programs",
  2023; Loula et al., "Syntactic and Semantic Control of Large Language Models via Sequential Monte
  Carlo", ICLR 2025 (https://arxiv.org/abs/2504.13139).
