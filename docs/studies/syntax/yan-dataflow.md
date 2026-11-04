# Yan Dataflow 言川 (`.ydf`): a syntax study

Status: brainstorm study, 2026-11-04. Not a spec change. Nothing here is normative; every code block
is a mock. No row numbers or diagnostic codes are allocated. Names are frozen and unchanged.

**One line.** Keep the Lustre core (equations, one clock per task, state records and step functions,
as accepted for `.ydf`), but make every memory born initialised, make rates a checked part of the
signature, put modes in the same indented shape as plain Yan's clause bodies, and give up the `->`
"followed by" operator, which collides with two existing meanings of `->` in Yan.

## 1. Methodology and process

**Question.** What should the syntax and the surface-level semantics of Yan Dataflow be, given that
it must lower to state records and step functions on the embedded profile and stay inside the one
core?

**Steps, in order.**

1. Read the current state narrowly: `docs/surfaces.md` section 13 (the MOCK grammar), RFC 0002 D9
   (the accepted example, lowering, limits and first slice), spec 16.7 (R536 admission rules N1 to N9,
   R544 itself), spec 17 (R320 profiles, R321 stack bound, R323 `srp` and `executor` scheduling) and
   spec 15's keyword list, to see which words and operators plain Yan already owns.
2. Survey the niche: synchronous languages (Lustre, Esterel, Signal, Lucid Synchrone, Scade 6 and the
   new textual Swan in Scade One, Heptagon, Vélus, Zélus), multi-rate and logical-time models
   (Giotto, Prelude, Lingua Franca), DSP (Faust), runtime monitors (Copilot), verification (Kind 2),
   FRP history (Elm), the PLC mainstream (IEC 61131-3 Structured Text) and recent work on LLMs writing
   control code (LLM4PLC, Agents4PLC). Ten web searches checked that the newer items exist and are
   current; older items are cited from memory and marked so.
3. Fix five tasks that cover the niche, so every proposal is compared on the same code:
   - **T1** PID controller with anti-windup (state, initialisation, derivatives).
   - **T2** debounce and rising edge (boolean streams, counters, the first tick).
   - **T3** garage door controller (modes, weak and strong transitions, a dwell timer).
   - **T4** flight control at 1 kHz with navigation at 50 Hz (multi-rate, crossing rates).
   - **T5** an 8-channel biquad filter bank at 48 kHz (delay lines, arrays of instances).
4. Write five proposals, each pushed to the end of one axis: brevity (A), familiarity for Lustre and
   Scade engineers (B), agent token cost and imperative familiarity (C), safety and explicitness (D),
   expressive power (E). Each proposal was written to win its axis, not to be balanced.
5. Experiment: (a) measure each proposal's five tasks with a token proxy (section 5.1); (b) check the
   current MOCK against plain Yan's grammar for ambiguities (section 5.2); (c) try to derive the
   diagnostics each proposal needs, to see which designs remove whole error classes (section 5.3).
6. Mix: one decision per design question, picking the proposal whose answer survives the others'
   criticism (section 6), then rewrite all five tasks in the mix.

**Confidence marks.** High: well established, several sources or direct reading. Medium: one source
or strong practitioner consensus. Low: my inference.

## 2. Findings

### 2.1 The market

| Tool | Who uses it | What it gets right | What hurts |
| --- | --- | --- | --- |
| Scade (Ansys), Scade 6, Scade One with Swan | avionics, rail, nuclear | qualified code generator (DO-178C, EN 50128), dataflow plus hierarchical state machines, static memory and time | cost, closed tooling, graphical models diff poorly; Scade One adds a textual language (Swan), a sign that text matters again |
| Simulink and Stateflow (MathWorks) | automotive, aerospace, robotics; the volume leader | block diagrams that control engineers read, simulation with plant models, autocode | not a language: hard to review, diff and merge; semantics of mixed solvers and sample times are subtle |
| Lustre V6, Heptagon, LustreC, Vélus | research, some industry | small, equational, analysable; Vélus is a verified compiler in Coq | clocks are hard to learn; no modules or rich types in classic Lustre |
| Esterel (now in Scade lineage) | hardware, protocol control | imperative synchronous control: `await`, `abort`, `every`, preemption | causality analysis is subtle; less used for numeric laws |
| Faust | audio DSP | five composition operators make DSP graphs tiny; one-sample delay is one character | block-diagram algebra is hard to read for non-specialists; names vanish |
| IEC 61131-3 ST, FBD, IEC 61499 | PLCs, factories; the largest population of control programmers | imperative ST with function-block state is what most control code looks like | no determinism story beyond the scan cycle, weak types, vendor lock-in |
| Copilot (NASA, Haskell EDSL) | runtime monitors | constant-memory stream programs, C99 output, used on flight software | Haskell embedding; an EDSL, not a surface |
| Lingua Franca | distributed and real-time systems | reactors with logical time, deterministic across threads and machines, timers and deadlines | heavier model than a control law needs |
| Zélus | research | synchronous discrete code and ODEs in one language, so the plant model sits beside the controller | research tool |
| Rust RTIC, Embassy | embedded Rust | Stack Resource Policy and async executors, which Yan already has as `srp` and `executor` | no dataflow; state is hand-written |

### 2.2 What works and what does not

1. **The synchronous hypothesis is the product.** Determinism, static memory and bounded time per tick
   are why Scade wins certification and why Lustre compiles to a step function. Yan already promises
   determinism and capability safety, so the fit is exact. Confidence: high (Benveniste et al. 2003;
   Halbwachs et al. 1991, from memory).
2. **Users need modes inside dataflow.** Pure equations make mode logic painful; Scade 6 added
   hierarchical automata as a conservative extension, and Stateflow ships beside Simulink for the same
   reason. Confidence: high (Colaço, Pagano, Pouzet 2005, from memory; Vélus later verified state
   machines too).
3. **Uninitialised `pre` is an error class people hit.** Lustre needs a separate initialisation
   analysis (Colaço and Pouzet 2004, from memory) because `pre x` is undefined on the first tick. Scade
   6's `last` with a declared initial value removes most of it. Confidence: medium.
4. **General boolean clocks are hard to learn; periodic rates cover most code.** Most industrial
   control is one rate or harmonic multi-rate. Prelude and Giotto made multi-periodic systems
   deterministic with periodic clocks and logical execution time (values published at period end).
   Confidence: medium (Colaço and Pouzet 2003 clocks paper; Giotto, Prelude from memory).
5. **Graphical-only models lose to text in review, diff and agent work.** Scade One's Swan and every
   Simulink diff tool point the same way. For a project built with agents this is decisive.
   Confidence: medium.
6. **General FRP failed as an app architecture, not as control.** Elm removed signals in 2016 because
   they were the wrong default for UIs. Dataflow should stay a niche surface for control and DSP, not
   grow into a general reactive framework. Confidence: high (Elm's "A Farewell to FRP").
7. **DSP wants zero-initialised delay lines and one-character delays.** Faust's `x'` and `x@n` are
   loved by its users; its wiring algebra (sequential, parallel, split, merge, recursion) is what makes
   it hard for others. Confidence: medium.
8. **LLMs write control code better with a checker in the loop.** LLM4PLC and Agents4PLC both get
   their gains from compile, simulate and model-check feedback, not from the raw model. A surface with
   executable trace examples and per-tick contracts gives Yan's agent loop that feedback for free.
   Confidence: medium (both arXiv, 2024).
9. **Verified compilation is reachable for this subset.** Vélus verifies Lustre with modular reset
   and state machines down to assembly. Yan's formal core track could target exactly this subset.
   Confidence: medium.
10. **Monitors are a second use.** Copilot shows the same stream language doubles as a runtime-monitor
    language with constant memory. A `.ydf` node with `post` lines is already a monitor. Confidence:
    medium.

### 2.3 What the current MOCK gets wrong (from reading, section 5.2)

- `->` means "followed by" in `0.0 -> e`, the return type in `node f(x) -> T`, and a match arm in plain
  Yan. Three meanings in one surface, two of them on one line in `node pid(...) -> f32 =` followed by
  `let i = 0.0 -> ...`. Confidence: high.
- RFC 0002 D9 writes the period twice: `const dt: f32 = 0.001` and `every 1ms`. They can drift, and in
  real code they do. The period should be readable inside the node. Confidence: high.
- `last(out)` in the MOCK debounce uses `out`, which plain Yan reserves for the result in `post`. That
  is a good choice and should be made a rule. Confidence: high.
- The MOCK's `automaton` / `state` / `out` keywords add three words for one idea; plain Yan's clause
  bodies already give an indented, arm-per-case shape. Confidence: low (taste).

## 3. Five tasks, five proposals

Common to all: files are plain Yan plus nodes; any pure plain Yan function applies pointwise to
streams; nodes are `uses none`; the lowering is a state record and a step function.

### Proposal A: Wire (brevity first)

Faust's delay notation, implicit zero initial values, `where` clauses, tacit lanes, and a transition
table for modes. Rates written `@ 1ms`.

**T1**

```
pid(err, g: Gains) = g.kp*err + g.ki*i + g.kd*(err - err')/dt
  where i = clamp(i' + err*dt, -g.windup, g.windup)
```

**T2**

```
debounce(raw, n) = if c == n then raw else out'
  where c = if raw == raw' then min(c' + 1, n) else 0

rising(x) = x and not x'
```

**T3**

```
door(btn, top, closed, obst) @ 10ms = states
  Idle:    stop, rising(debounce(btn, 5)) => Opening
  Opening: up,   top => Open
  Open:    stop, 5s => Closing
  Closing: down, obst =>! Opening, closed => Idle
```

**T4**

```
fcs(imu, gps, tgt) @ 1ms = control(tgt, pose~, att)
  where att  = attitude(imu)
        pose = nav(att~, gps) @ 20ms
```

**T5**

```
biquad(x, c) = y where y = c.b0*x + c.b1*x' + c.b2*x@2 - c.a1*y' - c.a2*y@2

mixer(cs, g) @ 48kHz = par(k, 8, biquad(_, cs[k])) :> *(g)
```

- `x'` is the previous tick, `x@n` n ticks back, both zero (the type's default) before the first tick.
- `=>!` is a strong transition (checked before the mode body runs); `=>` is weak. A bare duration as a
  guard means "time in this mode".
- `x~` reads the latest value of a stream at another rate.
- `par(k, n, f)` makes n lanes; the merge operator sums lanes; `*(g)` is a section.

Wins: shortest by far (section 5.1). Costs: implicit zero init hides the bug class finding 3 names;
`'` reads as "next" to anyone from TLA+ or Verilog; `~` means recursion in Faust; the lane algebra is
write-only for most readers; nothing here looks like plain Yan.

### Proposal B: Ledger (Lustre and Scade, faithfully)

The proposal that a Scade engineer reads on day one: named outputs, declared locals, unordered
equations, `->` and `fby`, boolean clocks with `when` and `current`, `automaton` with `unless`,
`until`, `restart` and `resume`.

**T1**

```
node pid(err: f32, g: Gains) -> (u: f32)
  var i: f32
  var d: f32
  i = 0.0 -> clamp(last i + err * dt, -g.windup, g.windup)
  d = 0.0 -> (err - last err) / dt
  u = g.kp * err + g.ki * i + g.kd * d
```

**T2**

```
node debounce(raw: bool, n: u8) -> (ok: bool)
  var count: u8
  count = 0 -> if raw == last raw then min(last count + 1, n) else 0
  ok = false -> if count == n then raw else last ok

node rising(x: bool) -> (e: bool)
  e = false -> x and not last x
```

**T3**

```
node door(btn: bool, top: bool, closed: bool, obstacle: bool) -> (m: Motor) every 10ms
  var t: u16
  automaton
    state Idle
      m = Motor.stop
      until rising(debounce(btn, 5)) restart Opening
    state Opening
      m = Motor.up
      until top restart Open
    state Open
      t = 0 -> last t + 1
      m = Motor.stop
      until t == 500 restart Closing
    state Closing
      m = Motor.down
      unless obstacle restart Opening
      until closed restart Idle
```

**T4**

```
node fcs(imu: Imu, gps: Gps, tgt: Pose) -> (u: Motors) every 1ms
  var slow: bool
  var att: Angles
  var pose: Pose
  slow = every_n(20)
  att = attitude(imu)
  pose = current(nav(att when slow, gps when slow))
  u = control(tgt, pose, att)
```

**T5**

```
node biquad(x: f32, c: Coef) -> (y: f32)
  y = c.b0 * x + c.b1 * (0.0 fby x) + c.b2 * (0.0 fby 0.0 fby x)
      - c.a1 * (0.0 fby y) - c.a2 * (0.0 fby 0.0 fby y)

node mixer(xs: [f32; 8], cs: [Coef; 8], g: f32) -> (y: f32) every 48kHz
  y = g * fold(+, 0.0, map(biquad, xs, cs))
```

Wins: maximum transfer from Lustre, Scade and Kind 2; equations are order-free and every output is
named, which helps reviews and model checkers. Costs: `var` changes meaning against plain Yan (a
mutable binding there); `->` collides (section 2.3); `current` has its own initialisation hole; the
`dt`/`every` duplication stays; the most text of the equational proposals.

### Proposal C: Ticks (agent cost and the imperative majority)

Write the step function almost directly. A function that declares `state` is a node; statements run
in order each tick; `state` cells persist. One new keyword plus one attribute, which matches the
embedded profile's rule that profiles add attributes, not keywords (R320).

**T1**

```
@node(every = 1ms)
pid(err: f32, g: Gains) -> f32 =
  state i = 0.0
  state prev = err
  i = clamp(i + err * dt, -g.windup, g.windup)
  let d = (err - prev) / dt
  prev = err
  g.kp * err + g.ki * i + g.kd * d
```

**T2**

```
debounce(raw: bool, n: u8) -> bool =
  state count: u8 = 0
  state prev = raw
  state held = false
  count = if raw == prev then min(count + 1, n) else 0
  prev = raw
  if count == n
    held = raw
  held

rising(x: bool) -> bool =
  state prev = x
  let e = x and not prev
  prev = x
  e
```

**T3**

```
type Door = Idle | Opening | Open | Closing

@node(every = 10ms)
door(btn: bool, top: bool, closed: bool, obstacle: bool) -> Motor =
  state mode = Door.Idle
  state t: u16 = 0
  let press = rising(debounce(btn, 5))
  let next = match mode
    Idle if press -> Opening
    Opening if top -> Open
    Open if t >= 500 -> Closing
    Closing if obstacle -> Opening
    Closing if closed -> Idle
    m -> m
  t = if next == mode then t + 1 else 0
  mode = next
  match mode
    Opening -> Motor.up
    Closing -> Motor.down
    _ -> Motor.stop
```

**T4**

```
@node(every = 1ms)
fcs(imu: Imu, gps: Gps, tgt: Pose) -> Motors =
  state pose = Pose.zero
  let att = attitude(imu)
  every 20 ticks
    pose = nav(att, gps)
  control(tgt, pose, att)
```

**T5**

```
biquad(x: f32, c: Coef) -> f32 =
  state x1 = 0.0
  state x2 = 0.0
  state y1 = 0.0
  state y2 = 0.0
  let y = c.b0 * x + c.b1 * x1 + c.b2 * x2 - c.a1 * y1 - c.a2 * y2
  x2 = x1
  x1 = x
  y2 = y1
  y1 = y
  y

@node(every = 48kHz)
mixer(xs: [f32; 8], cs: [Coef; 8], g: f32) -> f32 =
  var acc = 0.0
  for k in 0..8
    acc += biquad(xs[k], cs[k])
  g * acc
```

Wins: the most training data looks like this (Structured Text function blocks, Arduino, C static
locals); the grammar is plain Yan plus one statement; the lowering is nearly the source. Costs:
order-dependent state updates bring back the shift-register bug (swap `x2 = x1` and `x1 = x` and the
filter is wrong, silently); modes are hand-rolled boilerplate; `every 20 ticks` runs the slow work
inside the fast task, so the 1 ms budget must hold the 20 ms work (a worst-case spike); nothing is
order-free, so a model checker sees a program, not equations.

### Proposal D: Proof (safety and explicitness first)

Everything a certification reviewer asks for is in the text: units on types, the rate in the
signature, every memory with its initial value, assume and guarantee per tick, cost budgets, trace
examples, and explicit conversions between rates.

**T1**

```
node pid(err: f32[rad], g: Gains) -> u: f32[N*m]
  rate 1kHz
  assume g.windup >= 0.0
  guarantee abs(u) <= g.limit
  cost cycles <= 400, stack <= 96
  memory i: f32[rad*s] = 0.0
  i = clamp(last i + err * period, -g.windup, g.windup)
  let d: f32[rad/s] = (err - (last err else err)) / period
  u = clamp(g.kp * err + g.ki * i + g.kd * d, -g.limit, g.limit)
```

**T2**

```
node debounce(raw: bool, n: u8) -> ok: bool
  assume n >= 1
  guarantee ok != (last ok else false) implies stable(raw, n)
  memory count: u8 = 0
  memory ok: bool = false
  count = if raw == (last raw else raw) then min(last count + 1, n) else 0
  ok = if count == n then raw else last ok

node rising(x: bool) -> e: bool
  ex [F, T, T, F, T] => [F, T, F, F, T]
  e = x and not (last x else x)
```

**T3**

```
node door(btn: bool, top: bool, closed: bool, obstacle: bool) -> m: Motor
  rate 100Hz
  guarantee obstacle implies m != Motor.down
  modes
    mode Idle
      m = Motor.stop
      until rising(debounce(btn, 5)) restart Opening
    mode Opening
      m = Motor.up
      until top restart Open
    mode Open
      m = Motor.stop
      until in_mode >= 5s restart Closing
    mode Closing
      m = Motor.down
      unless obstacle restart Opening
      until closed restart Idle
```

**T4**

```
pub task fcs(imu: Imu, gps: Gps, tgt: Pose) -> u: Motors
  rate 1kHz
  let att = attitude(imu)
  let pose = nav(decimate(att, 20), decimate(gps, 20)) at 50Hz
  u = control(tgt, hold(pose, init: Pose.zero), att)
```

**T5**

```
node biquad(x: f32, c: Coef) -> y: f32
  assume stable(c)
  y = c.b0 * x + c.b1 * delay(x, 1, init: 0.0) + c.b2 * delay(x, 2, init: 0.0)
      - c.a1 * delay(y, 1, init: 0.0) - c.a2 * delay(y, 2, init: 0.0)

pub task mixer(xs: [f32; 8], cs: [Coef; 8], g: f32) -> y: f32
  rate 48kHz
  cost cycles <= 2000
  y = g * sum(map(biquad, xs, cs))
```

Wins: the guarantee in T3 is exactly the property a door certifier wants, and Kind 2 style
k-induction can discharge it; units catch the `err * dt` dimension bug; no cross-rate read is
implicit. Costs: four header lines before the first equation in T1; `memory` and `last ... else`
both handle initialisation (two ways); `cost` needs a WCET analysis Yan does not have yet.

### Proposal E: Timeline (expressive power first)

Three ideas from outside Lustre: Esterel's sequential control (`await`, `sustain ... until`,
preemption that restarts a block), Zélus's continuous time (`der`, so a plant model sits beside the
controller and the solver is chosen by the task's period), and Lingua Franca's logical time (timers,
results published one period later).

**T1**

```
node pid(err: f32, g: Gains) -> f32 =
  der i = err from 0.0 within -g.windup..g.windup
  g.kp * err + g.ki * i + g.kd * d/dt err
```

**T2**

```
node debounce(raw: bool, n: u8) -> bool from false =
  loop
    await raw != out
    watch raw == out
      await n ticks
      out = raw

node rising(x: bool) -> bool = x and not last x
```

**T3**

```
pub node door(btn: bool, top: bool, closed: bool, obstacle: bool) -> Motor every 10ms =
  loop
    sustain Motor.stop until rising(debounce(btn, 5))
    attempt
      sustain Motor.up until top
      sustain Motor.stop for 5s
      sustain Motor.down until closed
    retry when obstacle
```

**T4**

```
reactor fcs(imu: Imu, gps: Gps, tgt: Pose) -> Motors every 1ms =
  timer slow every 20ms
  let att = attitude(imu)
  on slow
    pose = nav(att, gps) after 20ms
  control(tgt, pose, att)
```

**T5**

```
node biquad(x: f32, c: Coef) -> f32 from 0.0 =
  c.b0 * x + c.b1 * x@1 + c.b2 * x@2 - c.a1 * out@1 - c.a2 * out@2

pub node mixer(xs: [f32; 8], cs: [Coef; 8], g: f32) -> f32 every 48kHz =
  g * sum(for (x, c) in zip(xs, cs): biquad(x, c))

-- a plant model for tests, simulated in the same file
node motor_plant(v: f32, k: f32, tau: f32) -> f32 =
  der w = (v * k - w) / tau from 0.0
  w
```

Wins: T3 is the clearest of all five (a door is a sequence, so write a sequence); `der` lets tests
close the loop against a plant model without Simulink; logical time makes multi-rate deterministic by
construction. Costs: three semantic models in one surface (equations, sequential control,
continuous time); Esterel causality is harder than Lustre's; `der` needs a solver choice and error
bounds the profile cannot certify; `reactor` is a second kind of item.

## 4. Scores

1 is worst, 5 best. Token proxy from section 5.1.

| Axis | A Wire | B Ledger | C Ticks | D Proof | E Timeline |
| --- | --- | --- | --- | --- | --- |
| Brevity | 5 | 2 | 2 | 1 | 4 |
| Read on day one (control engineer) | 2 | 5 | 4 | 3 | 3 |
| Read on day one (plain Yan user) | 2 | 3 | 5 | 3 | 3 |
| Agent cost and regularity | 3 | 3 | 4 | 2 | 3 |
| Safety by construction | 1 | 3 | 2 | 5 | 3 |
| Power (modes, rates, DSP) | 3 | 3 | 2 | 3 | 5 |
| Fit with plain Yan's grammar | 2 | 2 | 5 | 3 | 3 |
| Lowering stays simple (law 1) | 4 | 5 | 5 | 4 | 2 |

No proposal wins more than two rows. The mix takes one decision at a time.

## 5. Experiments

### 5.1 Token proxy

Each proposal's five task blocks, comments removed, counted with a lexical tokenizer (identifiers,
numbers with unit suffixes, and each other non-space character). This is a proxy, not a model
tokenizer; it ranks texts of the same language family reliably but not across languages.

| Proposal | Lines | Bytes | Proxy tokens |
| --- | --- | --- | --- |
| A Wire | 15 | 701 | 277 |
| B Ledger | 42 | 1443 | 459 |
| C Ticks | 65 | 1579 | 496 |
| D Proof | 49 | 1779 | 575 |
| E Timeline | 28 | 1028 | 325 |
| Mix (section 6.2) | 36 | 1446 | 454 |
| Mix without its `pre`, `post`, `ex` lines and the extra `attitude` node | 31 | | 383 |

C's count is inflated by hand-rolled modes and shift registers; D's by headers. A's lead comes almost
entirely from implicit initialisation and the lane algebra, the two features the findings argue
against. The mix costs about as much as B but carries a contract or example in four of five tasks,
which no other proposal except D does; without those lines it is 17 percent below B and 23 percent
below C. The biggest remaining cost is full parameter types in signatures, which plain Yan requires on
`pub` items and which a reviewer wants anyway.

### 5.2 Ambiguity check against plain Yan

Reading spec 15's keyword list and the MOCK grammar together:

- `->` is already the return arrow and the match-arm arrow. As "followed by" it is a third meaning,
  and `if a then b else c -> d` needs a precedence rule nobody will remember. Drop it.
- `var` is a mutable binding in plain Yan. B's `var` for stream locals changes its meaning. Drop it.
- `pre` is a contract word, as RFC 0002 D9 already notes; `last` is free and is also Scade 6's and
  Heptagon's word. Keep `last`.
- `from` and `every` are not reserved in plain Yan (`from` is soft); `every` appears in the MOCK and
  RFC. `until`, `unless`, `mode`, `latest`, `resume` are free. All can be soft words.
- `out` already means the result in `post`. Reusing it for "the result of this node" in `last out`
  is consistent.

### 5.3 Diagnostics each design needs

Trying to write the first error messages for each proposal showed one structural result: if every
memory is born with a value, the separate "`last` with no first-tick value" diagnostic disappears into
causality. The rule is: `last x` on the first tick is `x`'s declared `from` value; with no `from`, it is
`x`'s own first-tick value, which is legal exactly when that value does not depend on `last x`. That
is a causality question about tick zero, so one check covers both. For a counter
(`let c = last c + 1`) the checker reports the tick-zero cycle and offers the fix "add `from 0`". For
an edge detector (`x and not last x`) no `from` is needed and the first tick gives `false`, the
behaviour engineers want. Confidence: low until a checker exists, but the rule is short.

## 6. The mix

### 6.1 Decisions

| Question | Pick | From | Why |
| --- | --- | --- | --- |
| Equations or statements | equations, order-free, one binding per name | B | order-free code is what model checkers and reviewers want; C's shift-register bug is silent |
| Item keyword | `node` | B, RFC | statefulness deserves a visible word; plain Yan functions stay pointwise |
| Delay | `last x`, prefix, binds tighter than arithmetic; `last last x` for two | B, Scade 6 | one word, no parentheses, reads aloud |
| Initial values | `from v` on any binder (parameter, `let`, result); default is tick-zero self value (5.3) | D, mixed | every memory born initialised, without D's second mechanism |
| First-tick operator | none; `first` is a built-in `bool` stream for the rare case | new | frees `->` for its two plain Yan meanings |
| Period | `every 1ms` on the root node; `period` readable inside every node, typed as a duration | new | removes the `dt` duplication |
| Rate polymorphism | a node without `every` runs at its caller's rate | Lucid Synchrone | most nodes are rate-free; only tasks name rates |
| Crossing rates | `node(...) every 20ms` instantiates at a sub-rate; reading across rates needs `latest x`, with logical execution time | D, Giotto, E | deterministic, one explicit word, no boolean clocks in the first slice |
| Modes | `mode Name` blocks with `until c -> Next` (weak) and `unless c -> Next` (strong); `-> resume Next` keeps history | B, Lucid Synchrone | `->` as "goes to" matches plain Yan match arms |
| Time in a mode | `after 5s` as a guard, lowered to a tick counter | A, E | the common case gets one word |
| Contracts | `pre`, `post` per tick, as in plain Yan; temporal helpers (`stable`, `held`, `since`) in std | D, Kind 2 | no new contract words |
| Examples | `ex` lines over traces written as lists, one element per tick | D | executable tests the agent loop can run, finding 8 |
| Arrays of instances | `for` comprehensions over fixed-size arrays make one instance per element | A, C | no lane algebra; plain Yan syntax |
| Sequential control | deferred: `sustain` and `attempt ... retry` from E as a research slice that lowers to modes | E | the clearest door, but a second model |
| Continuous time | deferred: `der` as research, for test plant models only | E, Zélus | needs solver and error bounds |
| Units | rely on plain Yan's units; `period` has unit `s` | D | catches the dimension bug for free |

### 6.2 The five tasks in the mix

**T1**

```
node pid(err: f32, g: Gains) -> f32 =
  post abs(out) <= g.limit
  let i from 0.0 = clamp(last i + err * period, -g.windup, g.windup)
  let d = (err - last err) / period
  clamp(g.kp * err + g.ki * i + g.kd * d, -g.limit, g.limit)
```

`last err` needs no `from`: on the first tick it is `err` itself, so `d` starts at zero.

**T2**

```
node debounce(raw: bool, n: u8) -> bool from false =
  pre n >= 1
  let count from 0 = if raw == last raw then min(last count + 1, n) else 0
  if count == n then raw else last out

node rising(x: bool) -> bool =
  ex rising([false, true, true, false, true]) == [false, true, false, false, true]
  x and not last x
```

**T3**

```
pub node door(btn: bool, top: bool, closed: bool, obstacle: bool) -> Motor every 10ms =
  post obstacle implies out != Motor.down
  mode Idle
    Motor.stop
    until rising(debounce(btn, 5)) -> Opening
  mode Opening
    Motor.up
    until top -> Open
  mode Open
    Motor.stop
    until after 5s -> Closing
  mode Closing
    Motor.down
    unless obstacle -> Opening
    until closed -> Idle
```

A body made of `mode` blocks is an automaton; the first mode is initial; entering a mode resets its
state unless the arrow says `resume`.

**T4**

```
node attitude(imu: Imu) -> Angles = complementary(imu, alpha: 0.98)

pub node fcs(imu: Imu, gps: Gps, tgt: Pose) -> Motors every 1ms =
  let att = attitude(imu)
  let pose = nav(latest att, gps) every 20ms
  control(tgt, latest pose, att)
```

`nav(...) every 20ms` is an instance at a rate that divides the task's (checked). Reading `pose` at
1 ms or `att` at 20 ms without `latest` is a rate error with a fix that inserts it. Under logical
execution time `latest pose` is the value `nav` published at the end of its last period, so the
result does not depend on scheduling.

**T5**

```
node biquad(x: f32 from 0.0, c: Coef) -> f32 from 0.0 =
  c.b0 * x + c.b1 * last x + c.b2 * last last x - c.a1 * last out - c.a2 * last last out

pub node mixer(xs: [f32; 8], cs: [Coef; 8], g: f32) -> f32 every 48kHz =
  g * sum(for (x, c) in zip(xs, cs): biquad(x, c))
```

`from 0.0` on a parameter sets the delay line's initial value. The `for` makes eight `biquad`
instances, each with its own state.

### 6.3 Grammar sketch (MOCK)

```
item     = {attr} ["pub"] node | yan-item
node     = "node" NAME "(" [dparams] ")" "->" type ["from" expr] ["every" expr] "="
           (dexpr | NEWLINE INDENT {contract} (body | modes) DEDENT)
dparams  = dparam {"," dparam}
dparam   = NAME ":" type ["from" expr]
body     = {dbind} dexpr NEWLINE
dbind    = "let" NAME ["from" expr] [":" type] "=" dexpr NEWLINE
modes    = mode {mode}
mode     = "mode" TYPENAME NEWLINE INDENT body {trans} DEDENT
trans    = ("until" | "unless") guard "->" ["resume"] TYPENAME NEWLINE
guard    = expr | "after" expr
dexpr    = "last" dexpr                         -- previous tick, binds tighter than binary operators
         | "latest" dexpr                       -- value from another rate, logical execution time
         | NAME "(" [args] ")" "every" expr     -- sub-rate instance
         | yan-expr                             -- pure, no allocation; node calls make instances
contract = ("pre" | "post" | "ex") expr NEWLINE
```

New soft words: `node`, `last`, `latest`, `every`, `mode`, `until`, `unless`, `after`, `resume`
(`from` is already soft). Built-ins: `period`, `first`. Removed from the MOCK: `->` as an operator,
`automaton`, `state`, `out` as a mode keyword.

### 6.4 Laws of the surface

A framework to judge later additions against:

1. **Born initialised.** Every memory has a value on the first tick, given by `from` or by causality.
2. **One tick, no cycles.** Within a tick, dependencies are acyclic; only `last` breaks a cycle.
3. **Rates are types.** A node either names its rate or inherits its caller's; crossing a rate is
   one visible word.
4. **Pure inside, effects at the edge.** Nodes are `uses none`; only the root task's handler touches
   peripherals.
5. **The lowering is printable.** `yan view --as yn` shows the state record and step function, so
   every surface construct has a plain Yan reading (law 1 of the surface semantics study).

### 6.5 What it lowers to

All of it is lowering, per R544 and the surface semantics study's option A: a state record with one
field per `last` use, mode tag, `after` counter and sub-node instance; a step function
`step(var s, inputs) -> output` with `uses none`; `every` binds the root step to an `srp` or
`executor` task (R323); `latest` lowers to a double buffer swapped at the publishing instance's
period end; `ex` traces lower to tests that call the step function once per element. No core change.

## 7. Open questions

1. **Root inputs.** How do peripherals feed a root node: a plain Yan `main` that reads the HAL and
   calls the step, or `in` and `out` declarations bound to HAL handles in the node header? The mix
   assumes the first (law 4); the second is shorter for small firmware.
2. **Event clocks.** Sporadic inputs (a CAN frame arrives) need a boolean clock (`on c`) or an
   interrupt task. The first slice has only periodic rates; which comes next?
3. **`from` on parameters.** It is concise for DSP but surprising elsewhere. Alternative: std
   `delay(x, n, from: v)` only.
4. **Units of `period`.** If plain Yan's units are not ready for the embedded profile, `period` is an
   `f32` of seconds, and the dimension check waits.
5. **Default transition semantics.** Reset on entry (Scade's `restart`) or history (`resume`)? The mix
   picks reset as the default, as Scade and Lucid Synchrone do.
6. **Latency of `latest`.** Logical execution time adds one slow period of latency. Fine for most
   control; a phase-aligned read (Prelude's operators) may be needed for tight loops.
7. **Sequential control.** E's `sustain` and `attempt ... retry` read best for sequences like the
   door. Admit as a research slice lowered to modes, or keep modes only?
8. **Verification target.** Should the formal core track aim at a Vélus-like verified compile of
   exactly this subset (nodes, `last`, modes with reset)?
9. **WCET.** D's `cost` lines need a cycle bound. Out of scope until the embedded profile has one.

## 8. Sources

Checked on the web for this study:

- Scade One and Swan, Ansys: https://www.ansys.com/en-gb/products/embedded-software/ansys-scade-one
- Swan code generation discussion, Ansys forum: https://innovationspace.ansys.com/knowledge/forums/topic/generating-efficient-code-from-a-swan-model/
- Bourke et al., "A Formally Verified Compiler for Lustre", PLDI 2017: https://www.di.ens.fr/~pouzet/bib/pldi2017.pdf
- Pesin, Vélus at SYNCHRON 2025: https://synchron-2025.imag.fr/slides/basile_pesin_velus-smtcoq_2025.pdf
- Colaço and Pouzet, "Clocks as First Class Abstract Types", EMSOFT 2003: https://www.di.ens.fr/~pouzet/bib/emsoft03.pdf
- Bourke and Pouzet, "Zélus: A Synchronous Language with ODEs", HSCC 2013: https://www.di.ens.fr/~pouzet/bib/hscc13.pdf
- Lohstroh et al., "Reactors: A Deterministic Model of Concurrent Computation for Reactive Systems": https://www2.eecs.berkeley.edu/Pubs/TechRpts/2020/EECS-2020-235.html
- Orlarey, Fober, Letz, "Syntactical and semantical aspects of Faust", Soft Computing 2004: https://link.springer.com/doi/10.1007/s00500-004-0388-1
- Champion et al., "The Kind 2 Model Checker", CAV 2016: https://www.di.ens.fr/~pouzet/cours/comasic/cours4/CAV2016-Kind2.pdf
- Kind 2 Lustre input and contracts: https://kind.cs.uiowa.edu/kind2_user_doc/2_input/1_lustre.html
- Copilot (NASA): https://github.com/Copilot-Language/copilot
- Czaplicki, "A Farewell to FRP": https://elm-lang.org/news/farewell-to-frp
- Fakih et al., "LLM4PLC", 2024: https://arxiv.org/abs/2401.05443
- Liu et al., "Agents4PLC", 2024: https://arxiv.org/abs/2410.14209

Cited from memory, not re-fetched:

- Halbwachs, Caspi, Raymond, Pilaud, "The synchronous data flow programming language LUSTRE", Proc. IEEE 1991.
- Benveniste et al., "The synchronous languages 12 years later", Proc. IEEE 2003.
- Berry and Gonthier, "The Esterel synchronous programming language", Sci. Comput. Program. 1992.
- Colaço, Pagano, Pouzet, "A conservative extension of synchronous data-flow with state machines", EMSOFT 2005.
- Colaço and Pouzet, "Type-based initialization analysis of a synchronous dataflow language", STTT 2004.
- Hamon and Pouzet, "Modular resetting of synchronous data-flow programs", PPDP 2000.
- Henzinger, Horowitz, Kirsch, "Giotto: a time-triggered language for embedded programming", Proc. IEEE 2003.
- Forget, Boniol, Lesens, Pagetti, "A multi-periodic synchronous data-flow language" (Prelude), HASE 2008.
