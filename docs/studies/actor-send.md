# One-way `actor.send` for Yan

Study and proposal, 2026-11-04. Docs only: no spec rows, no conformance edits, no diagnostic codes.
Items are labelled D1, D2 and so on; none carries a row number. Everything here is non-normative until
an RFC and accepted rows say otherwise.

Brief: item F4 of the [language forums study](https://claude.ai/artifact/E8xVSw3fhTwuxFu38DqrtT).
Yan actors have only `call` (9.6): `h.f(args)` sends a message and suspends the caller until the
handler returns, under a deadline. There is no fire-and-forget form. Study a one-way send: mailbox
ordering, backpressure, failure, cancellation and its interaction with deadlines and `Timeout` (9.4,
R472) and the [shield study](shield.md), capability safety and determinism.

## 1. Answer

Add a std view, not syntax: `actor.send(h).f(args)`. `actor.send(h)` returns a `Send[S]` view of an
`Actor[S]` or `Remote[Actor[S]]` handle, and 17.2 resolves `f` on it exactly as it does on the handle
(R435). The call enqueues the message and returns once the message is admitted to the mailbox (or, for
a remote handle, handed to the transport); it never waits for the handler. Only handlers whose result
is unit and whose fail type is empty can be sent. Sends and calls from one holder to one actor share
one FIFO.

The send's own failure type is `Stopped | Full | Timeout | Cancelled`, plus the transport part of
`RemoteError` for a remote handle. It is a cancellation point only while it waits for mailbox space.
Once admitted, the message is detached from the sender: cancelling or timing out the sender does not
retract it, and the handler runs under the actor's own scope, as calls already do (R397).

One finding outside `send` itself: the lossy overflow policies (`drop_oldest`, `drop_new`, `fail`)
make results depend on timing under `pool`, so binding one should need `uses nondet` (D8).

## 2. The gap today

- A caller that wants no answer still suspends for the whole handler, holding its deadline budget and
  its place in any wait-for graph. Notifications, log or metric sinks, and fan-out to many actors pay a
  round trip each.
- The workaround is a `par concurrent` branch per notification that makes the `call` and ignores the
  result. That costs a task per message, keeps the sender's scope open until every handler has run,
  and puts the call's `Timeout` and `Stopped` on a branch where nothing handles them.
- A reply pattern (callee answers later through the caller's handle) already needs a one-way send to
  avoid a call cycle, which W808 flags and the deadlock detector would report.

## 3. Prior art

| System | One-way form | Ordering | Backpressure | Failure seen by sender |
| --- | --- | --- | --- | --- |
| Erlang/OTP | `Pid ! Msg`, `gen_server:cast` | FIFO per sender-receiver pair | none; unbounded mailbox | none; use `monitor` |
| Akka (classic and typed) | `tell` / `!` | FIFO per pair | unbounded default; bounded mailbox with push timeout, overflow to dead letters | none; dead letters log |
| Orleans | `[OneWay]` grain method | not guaranteed | none | none |
| Pony | every behaviour is async | causal: per pair FIFO plus causality across actors | runtime mutes senders to overloaded actors | none |
| Swift (pitch, `send` and `@oneway`) | `send a.m()` on non-throwing, unit methods | consecutive sends from one task to one actor in order | none | none |
| Ray | `a.m.remote()` returns a future that may be dropped | per caller, in submission order | bounded by object store | via the dropped future, ignored |
| Tokio `mpsc` / Go channels | `send().await` / `ch <- v` | FIFO | bounded: sender suspends; `try_send` fails `Full` | `Closed` |

Lessons:

1. Everyone restricts one-way to calls whose answer nobody needs: unit result and no error the caller
   could act on (Swift, Orleans). Yan should do the same and say so in the type.
2. Unbounded mailboxes (Erlang, Akka default) are the main cause of actor-system memory blowups; Pony
   needed runtime muting to recover. Yan's bounded mailboxes (9.6) must apply to sends too, and the
   suspending sender is the backpressure, as in Tokio and Go.
3. Pony's causal order is stronger than per-pair FIFO but needs runtime support that does not
   cross machines. R408 already settled on per-pair FIFO; keep it.
4. Mixing one-way and request-response on one queue keeps a cheap barrier: a `call` after a run of
   sends observes all of them (Erlang and Akka users rely on this).

## 4. Proposed semantics

Yan Agent spelling.

```
def record(log: Actor[Log], ev: Event) -> fail ActorError | Cancelled:
    actor.send(log).append(ev)?
```

**Shape.** `actor.send(h)` borrows the handle and returns `Send[S]` (or `Send[Remote[S]]`). It is not a
second sender: it borrows `h`, so R391 and E709 are untouched and no `nondet` is needed. Handler lookup
is the R435 rule with `Send[S]` added to the receiver types of 17.2. Sending a handler whose result is
not unit or whose fail type is not empty is a compile error naming the handler (D2).

**Admission and backpressure.** The payload is moved into the mailbox (R264). Then by policy:

- `block` (default): if the mailbox is full the sender suspends until there is space. This is a
  blocking, fallible capability operation, so it is a cancellation point and obeys the enclosing
  deadline, or the 5 s call default when there is none (9.6). Expiry is `fail Timeout`.
- `fail`: a full mailbox fails `Full` at once.
- `drop_new` / `drop_oldest`: the send succeeds and the policy drops a message; std counts drops in
  `metrics`.

A send that does not wait is not a cancellation point and never switches tasks, like `print` (9.4).
A send that fails leaves nothing in the mailbox; its moved payload is dropped by ordinary scope exit
(D5).

**Ordering.** One FIFO per ordered pair (holder, actor), shared by sends and calls. No order across
senders (unchanged). A message admitted before a `call` is handled before that call, so a call is a
barrier for earlier sends. The memory model already has the edge "actor send before its handler runs"
(section 6); send adds none.

**Detachment.** After admission the message belongs to the actor. Its handler runs with no caller
deadline and is not cancelled by the sender's scope. This adds no orphan work: the handler runs in the
supervisor's scope (R397), which already outlives callers, and stopping that scope stops the actor.
Contrast `call`, where a caller's expiry drops a not-yet-dequeued message and a remote caller's cancel
cancels the callee's handler (R408).

**Stopped actors.** A send to an actor that has stopped fails `Stopped`. Messages still queued when an
actor stops or crashes are dropped, as in Erlang; a restart starts with an empty mailbox. Callers
blocked in `call` get `Stopped`; senders are not told about messages dropped after admission. Code that
needs to know uses `monitor` (R411) or a `call` barrier.

**Remote.** `uses cluster` is unchanged (R400). Admission means the frame is accepted by the per-pair
QUIC stream (R404), whose flow-control credit is the backpressure, under the same deadline rule.
Delivery stays at most once (R408). The send can fail `Unreachable` or `Rejected` when the link already
knows; it cannot report `SchemaMismatch` or a remote `Stopped` that happens after the frame left. A
remote message carries no budget, since nobody waits.

**Deadlines, `Timeout` and R472.** A send's fail type always includes `Timeout` and `Cancelled`,
because the policy is chosen at spawn and is not in the handle's type (D6 weighs putting it there). No
change to R472: a function that opens a deadline still has `Timeout` in its fail type.

**Shield.** Inside `with shield(d):` (shield study, section 4) a send waits for admission under the
shield's deadline, not the outer one, and a held cancel does not interrupt it. This makes `send` the
natural cleanup primitive: a cancelled task can post a goodbye or audit event to a logger actor within
a bounded shield, without starting a task and without waiting for the handler.

**Drain.** Sends are not intake operations, so they do not fail `Draining`. During drain a supervisor
stops its children in shutdown order (R397); a sender racing that stop gets `Stopped`.

## 5. Capability safety

- The handle is the authority, as for `call`. `actor.send` needs no capability, consistent with R442
  (actors have none). The handler runs with the actor's capabilities, never the sender's, so a send
  cannot amplify authority.
- A `Send[S]` view cannot be stored past the borrow or shared, so it is not an attenuated capability
  that could leak. An owned send-only facet (a handle that can send but not call) is possible later
  (D9) but not needed now.
- Payloads follow the existing rules: `Send` locally, `Wire` for `Remote` (E720), checked when the
  handle is shared as `Remote`.
- The reply pattern (pass your own handle so the callee can send back) creates a second sender to the
  replier's own actor, so it goes through `share` and `uses nondet`, as today. Nothing new here.

## 6. Determinism

- Without `share`, each actor has one holder, so each mailbox has one producer and one consumer. With
  the `block` policy, the sequence each handler sees is fixed by the program, whatever the scheduler:
  a Kahn network, deterministic even under `pool`. Under `sequential` and `seeded` the interleaving is
  fixed as well (R264, R390).
- A non-waiting send is not a switch point under `sequential`, so adding a send does not reorder other
  tasks.
- The lossy policies break this. Whether `drop_new` drops, or `fail` returns `Full`, depends on how
  fast the consumer drains, which under `pool` is timing. This is the same reason `try_send` needs
  `uses nondet` (E709), and it already applies to `call` today. Recommendation: spawning an actor with
  `drop_oldest`, `drop_new` or `fail` requires `uses nondet` (D8). Under virtual time and `seeded` the
  outcome stays reproducible.
- Expiry while waiting for admission depends on real time, as every deadline does; under virtual time
  it is deterministic.

## 7. Decisions

| Item | Question | Recommendation |
| --- | --- | --- |
| D1 | Spelling? | `actor.send(h).f(args)`, a std view resolved by 17.2. Rejected: a `send` keyword (Swift), new syntax for a std feature; `actor.send(h, msg)` with message values, since Yan handlers are functions, not message types; `@oneway` on the handler, which hides the non-waiting at the call site. |
| D2 | Which handlers can be sent? | Unit result and empty fail type. A failing handler's error has nowhere to go; a panic still goes to the supervisor. |
| D3 | Shared FIFO with `call`? | Yes, one queue per (holder, actor), so a `call` is a barrier. |
| D4 | When does `send` return? | At admission: in the mailbox locally, accepted by the stream remotely. Not at delivery. |
| D5 | On failure, is the payload returned? | No: it is dropped. Returning it (Tokio's `SendError(T)`) adds a generic failure type for a rare retry case. |
| D6 | Overflow policy in the handle type? | No for now; the send's fail type carries `Full` and `Timeout` always. Typing the policy (`Actor[S, block]`) would let `drop_*` sends drop `Full` from the fail type, at the cost of a second type parameter everywhere. |
| D7 | Queued messages when an actor stops? | Dropped; restart starts empty. Senders learn through `monitor` or a `call` barrier. |
| D8 | Lossy policies need `uses nondet`? | Yes at spawn, for both `send` and `call`. This amends 9.6 and needs its own row. |
| D9 | Owned send-only handle? | Not now. Revisit if an untrusted-plugin case appears. |
| D10 | Detach after admission? | Yes: no caller deadline or cancel follows the message, matching R397. |
| D11 | Lints? | A warning for `send` to a `drop_*` actor inside a `shield`, where the drop defeats the point; and the existing W808 skips send edges, which cannot deadlock. |

## 8. Not proposed

- Unbounded mailboxes or an unbounded send. 9.5 already rules them out for channels.
- Causal ordering across actors (Pony). It does not survive R408's at-most-once transport.
- A future-returning send (Ray). That is `call` in a `par concurrent` branch.
- Per-message time-to-live. A message nobody waits for has no deadline to honour; if a use case
  appears, a handler can compare a timestamp in the payload.

## 9. Next step

If the lead accepts D1 to D11, an RFC covers the std surface (`Send[S]`, the 17.2 extension, the fail
types), the 9.6 amendments (shared FIFO, stopped-actor drop, D8 nondet on lossy policies) and the
diagnostic for D2. Conformance cases to propose separately: send then call barrier, full mailbox under
`block` with deadline expiry, cancel while blocked leaves nothing queued, send inside a shield, send to
a stopped actor, `drop_new` spawn without `uses nondet` rejected, Kahn determinism under `pool`.

## 10. Decisions (Antonio, cards, 2026-11-04)

D1, D2, D3, D4, D7, D8, D10 and D11 accepted as recommended. Three answers differ from the
recommendation:

- D5: on failure the payload is returned to the sender (Tokio-style `SendError(T)`), so a failed send
  can be retried with the same value. The fail type of `send` is generic in the payload type.
- D6: the overflow policy is part of the handle type (`Actor[S, block]`). Sends to `drop_*` actors then
  drop `Full` from their fail type, at the cost of a second type parameter on actor handles.
- D9: an owned send-only handle is added, for least authority (for example untrusted plugins).
