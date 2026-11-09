# 0056: Actor runtime: spawn, handler calls, mailboxes, supervision, native and Remote on `std.sim`

- Status: accepted
- Research: none; the model is accepted spec (9.6, 9.6.1, R264, R390 to R399, R400 to R419, R435, R442, R813, R814). This note fixes how it runs and closes the gaps the spec leaves open. Gap issue #835.
- Rows: R956 to R960

## Summary

Actors exist only as types today: `Actor[S]`, `Remote[Actor[S]]`, `std.actor.share` (stubbed in
`yan-interp/src/sync.rs`) and the E720 check in `yan-elab/src/lower/call_expr.rs`. There is no spawn,
no handler call (R435), no mailbox, no supervisor and no `cluster` handler, so no actor program runs on
either backend (#835).

This RFC gives actors one runtime model: an actor is a task of the existing cooperative scheduler of
`par concurrent` (RFC 0043), owned by a supervisor scope, with a bounded mailbox; a handler call enqueues
a message and parks the caller on a reply slot. The interpreter gets it first, native ports it the way
RFC 0043 ported `par concurrent`, and `Remote` runs over a `sim` handler of `cluster` in one process.
It also fills four gaps in 9.6: the spawn signature, who owns an actor when no supervisor is bound, what
a restart restarts from, and which task runs first under `sequential`.

## Decisions

### D1: the spawn surface

`std.actor.spawn(state: S, capacity: int = 1024) -> Actor[S, P]` moves `state` into a new actor and
returns its handle. `P` is inferred from the expected type and defaults to `block`; any other `P` needs
`uses nondet` (R814, already E709). `capacity >= 1` is a `pre` contract of `spawn` (section 8).

Handler calls stay as 17.2 resolves them: `h.f(args)` on `Actor[S, P]` finds `def f(var self: S, ...)`
in the module that declares `S` (R435) and is the `call` of 17.3's table; there is no separate `call`
function to spell. `actor.send(h).f(args)` and `actor.send_only(h)` are as R813. The call's fail type is
the handler's plus `ActorError` (`Stopped | Timeout | Cancelled | Deadlock`).

Options: (a) value state, as above; (b) a start function `spawn(f, args)` only, as child specs do.
(a) is the common case and reads like the spec's examples; D3 gives restartable children the start
function form, so both exist with distinct jobs.

- Decision: (a), inconsequential, most complete reading of 17.3
- Row: R956

### D2: who owns an actor when no supervisor is bound

9.6 says actors are children of the scope that binds their supervisor (`with supervisor(spec):`), but
not what happens when `spawn` runs outside every such scope.

Options:

- (a) A root supervisor bound by std before `main`, like the other default handlers (7.4):
  `one_for_one`, children `temporary`, intensity 1 per 5 s. Actors spawned outside any `with
  supervisor` are its children and are stopped, in reverse spawn order, when `main` returns. Test
  functions get their own root, so actors never outlive a test.
- (b) `spawn` outside every `with supervisor` is a compile error (a new E code), so ownership is always
  written.
- (c) The nearest enclosing `par` or task scope owns the actor.

(a) keeps small programs small and matches how std binds `sched`, `alloc` and `time`; the root is
structured (it ends with `main`), so nothing leaks. (b) is stricter but makes every script open a
supervisor block. (c) mixes two scope kinds and makes an actor's lifetime depend on unrelated `par`
blocks.

- Decision: (a), the lead, 2026-11-09 (card on this thread)
- Row: R957

### D3: what a restart restarts from

A supervisor restarts a crashed child, but `spawn(state)` consumed its initial state, so there is
nothing to restart from.

Options:

- (a) Two forms. `spawn(state)` makes a `temporary` child: never restarted, the state is moved once.
  A restartable child is a `ChildSpec(start, args, restart=permanent)` where `start` is a top-level
  function returning `S` and `args` its arguments; each (re)start calls `start(args)` under the
  supervisor's capabilities (R397). This is R411's child spec, so the local and the remote form are one
  type, and off-node starts add only `@remote` and `Wire` (E721).
- (b) `spawn(state)` keeps a copy of the initial state for restarts, requiring `S: Clone`.
- (c) Only `ChildSpec`; no value spawn.

(a) restarts from a known start without copying hidden state and reuses the R411 shape. (b) restarts
from a stale snapshot and silently adds a `Clone` bound. (c) drops D1's common case.

- Decision: (a), the lead, 2026-11-09 (card on this thread)
- Row: R958

### D4: one scheduler, one task identity

Actors are tasks of the scheduler that runs `par concurrent` branches (interpreter `run_tasks`,
yan-rt `sched.rs`), not a second scheduler (CLAUDE.md practice 12). A task's identity is its task path
(R398): a `par concurrent` branch is `parent / branch index`; an actor is `supervisor scope / spawn
index`; a handler run is the actor's task. Under `sequential`, the runnable task with the lowest path in
lexicographic order runs next, which extends 9.1's lowest-index rule to actors; under `seeded(seed)`
the choice among runnable tasks is the seeded generator's. This is what makes R264 hold: the same seed
gives the same interleaving.

A task parks on a channel end (today), a reply slot or a mailbox with no space (new). Progress (the
`progress` counter) also counts a message admitted, dequeued or replied, so the retry rule of R390
carries over unchanged. Global quiescence (R396) and virtual-time advance read the same park set.

- Decision: one scheduler, inconsequential, most complete
- Row: R959

### D5: mailboxes, calls and replies

Each actor has one mailbox: a ring of `capacity` messages, each a handler index, its moved arguments
(R264) and, for a `call`, a reply slot. Order is admission order, which implies the spec's per-sender
FIFO (9.6) and is deterministic under D4. `call`:

1. Under the call deadline (enclosing scope or 5 s), admit the message per `P` (block parks on space).
2. Park the caller on the reply slot.
3. The actor task dequeues; a message whose deadline passed is dropped and its caller fails `Timeout`
   (R397); otherwise the handler runs to completion (non-reentrant) and fills the slot with its
   `Result`.

`send` stops after step 1 and has no slot. A crash drops the queue and fails every waiting caller
`Stopped{reason}`; a restart starts empty (R813). `@reentrant` handlers (E715 already checks them) may
start while another handler of the same actor is parked; the actor task then holds one frame stack per
running handler. The first slice rejects `@reentrant` at run time with an "unsupported" diagnostic,
never a silent fallback (practice 13), and a later slice implements it.

- Decision: as above, inconsequential
- Row: none; 9.6 already says this

### D6: deadlock detection

Under `sequential`, `seeded` and `explore` the wait-for graph is the park set: a task parked on a reply
slot waits for the slot's actor, a task parked on a channel end waits for the tasks holding the other
end. A cycle fails `Deadlock` in the youngest caller (highest task path among the cycle's callers),
matching 9.6. Under `pool` no graph is kept (R349); global quiescence still panics (R396).

- Decision: as above, inconsequential
- Row: none

### D7: the interpreter

The VM already parks a task from nested plain calls by unwinding its frames into `Saved` and replaying
them (`vm.rs`, `unwinding`, `chain`). An actor task reuses that: its body is a loop that dequeues a
message and calls the handler's code as an ordinary call, so a handler that calls another actor parks
through any depth of plain calls. A wait under a std (Rust) frame, such as a closure passed to `map`,
cannot be saved; it stays an "unsupported" diagnostic naming the std function, as today for channels.
`std.actor.share` becomes a plain handle copy plus the `Remote` wrapper; the `nondet` check is static.

- Decision: as above, inconsequential
- Row: none

### D8: native

Native follows RFC 0043: a handler that may block gets its `$co` coroutine variant (`yan-mir/src/co.rs`),
the actor loop is a yan-rt task that dequeues and resumes the handler's frame, and mailboxes and reply
slots live in a new `yan-rt/src/actor.rs` beside the channel runtime, driven by the scheduler of
`sched.rs` with D4's order. The handler table per state type `S` is generated in a new MIR module
(not `yan-mir/src/lower.rs`, practice 5). Native prints what the interpreter prints, by construction; each
native slice attaches both outputs.

- Decision: as above, inconsequential
- Row: none

### D9: `Remote` over `std.sim`

R419 puts the `sim` handler in M1. `cluster = sim(nodes: int, seed: int)` runs every node in this
process as a `std.sim` host: each node has its own root supervisor (D2), its own node id (R403, derived
from the seed, so runs repeat) and its own task-path prefix. A call through `Remote[Actor[S]]` always
encodes and decodes its arguments, result and failure with `Wire` (R402), carries the remaining deadline
(R408) and is delivered at most once, FIFO per ordered actor pair. Faults are off by default; `sim`
takes optional `drop`, `delay` and `partition` schedules, all drawn from the seed in virtual time, so
`explore` (R417) can cover message orders. `monitor` and `Down{reason}` (R411) are delivered by the sim
membership view.

- Decision: as above, inconsequential
- Row: R960

## Slice plan

Each slice is one PR: interpreter output, native output from slice 4 on, one proposed `conform/*` case,
and its gap items numbered in #835.

1. **Local spawn and handler calls, interpreter.** D1, D2, D4, D7: `spawn`, `h.f(args)` with `block`,
   root supervisor, `sequential` order, global quiescence. Case: a counter actor called from `main` and
   from two `par concurrent` branches.
2. **Mailboxes and send, interpreter.** D5 in full: capacity, the four policies, `send`, `send_only`,
   call deadlines under virtual time, `Timeout` drops, D6 deadlock. Cases: `drop_oldest` under
   `nondet`, a two-actor call cycle failing `Deadlock`.
3. **Supervision, interpreter.** D3: `with supervisor(spec)`, `ChildSpec`, three strategies, restart
   types, intensity and escalation, shutdown order and timeout, `Stopped` to waiting callers. Case:
   `one_for_one` restart after a handler failure.
4. **Native.** D8 for slices 1 to 3, both outputs per program. `@reentrant` lands here on both backends.
5. **Remote on `sim`.** D9: `share` to `Remote`, the always-encode path, `monitor`, seeded faults,
   `RemoteError`. Case: a two-node `sim` call with a dropped message failing `Unreachable`.

QUIC, TCP, views, entities, groups and migration stay M4 (R419) and are not in this plan.

## Spec changes

- 17.3 `std.actor`: the `spawn` signature (D1) and `ChildSpec` (D3).
- 9.6: the root supervisor (D2), `spawn` children are `temporary` and restarts call a start function (D3).
- 9.1: the `sequential` ordering sentence covers actor tasks by task path (D4).
- 9.6.1: the `sim` constructor and its fault parameters (D9).
