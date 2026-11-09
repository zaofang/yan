# 0059: Actor overflow policies, send-only handles and send failures

- Status: accepted
- Research: none; this RFC settles how R814, R813 and R435 are spelled, without new behavior. Gap issue #835 (slice 2b items 1 to 3 and 5).
- Rows: R974 to R977

## Summary

RFC 0056 slice 2b (#882) gave handler calls their `ActorError` fail type on the interpreter. Three
parts of 9.6 still cannot be implemented as written:

- `Actor[S, P]` names its overflow policy `block`, `drop_oldest`, `drop_new` or `fail`, and `fail` is
  a keyword, so `Actor[S, fail]` does not parse. R435 also spells the policy as a keyword argument,
  `actor.spawn(e, mailbox=..., overflow=...)`, which conflicts with R956 (`capacity`, `P` from the
  expected type).
- `actor.send_only(h)` returns a handle whose type 9.6 does not name.
- A failed send "returns its payload", but the failure value has no type.

A fourth item follows from slice 2b: the local `Stopped` has no reason, while `RemoteError` has
`Stopped{reason}` (9.6.1).

## Decisions

### D1: the policy spelling

Options: (a) std types `Block`, `DropOldest`, `DropNew` and `Reject`; (b) the keyword argument
`spawn(e, overflow="drop_oldest")` with `P` out of the handle type; (c) lowercase type names, with
`fail` renamed `reject`.

(a) keeps `P` in the handle type, as R814 requires, so a send's fail type can follow it. The names are
ordinary types and parse in every surface. `Actor[S]` is still `Actor[S, Block]`. `P` comes from the
expected type, as R956 says (`a: Actor[Log, DropOldest] = actor.spawn(Log())`). Any `P` other than
`Block` needs `uses nondet` at the spawn (E709). R435's `mailbox=` and `overflow=` are replaced by
R956's form.

- Decision: (a), the lead, 2026-11-09 (card on the slice 2b thread)
- Row: R974

### D2: the send-only handle type

Options: (a) `SendOnly[S, P]`, with `Remote[SendOnly[S, P]]` for a remote one; (b) `Mailbox[S, P]`;
(c) `Actor[S, P]` with a hidden flag that rejects calls.

(a) names what the handle permits. It is `Send` and shares like `Actor[S, P]` (`nondet`, R442), and a
call through it is E201 with the fix `actor.send(h)`. (b) suggests the actor's own queue, which the
holder does not own. (c) hides authority in a value the type does not show, against least authority.

- Decision: (a), the lead, 2026-11-09 (card on the slice 2b thread)
- Row: R975

### D3: the failure of a send

Options: (a) one generic record `Undelivered[A, R]`, where `A` is the tuple of the handler's arguments
after `self` and `R` is the reason; (b) a sum `Stopped(args) | Full(args) | Timeout(args) |
Cancelled(args)` per handler; (c) failures without the payload.

(a) gives every send one shape: `fail Undelivered[A, R]` with fields `reason: R` and `args: A`. `R` is
the reason union that `P` allows: `Stopped | Timeout | Cancelled` for `Block`, `Stopped | Full` for
`Reject`, and `Stopped` for `DropOldest` and `DropNew`. A remote handle adds the transport part of
`RemoteError` to `R`. The caller retries with `actor.send(h).f(*e.args)`. `Full` is a new fieldless
std class beside `Stopped`. (b) makes a new sum per handler and per policy. (c) breaks R813's retry
promise.

- Decision: (a), the lead, 2026-11-09 (card on the slice 2b thread)
- Row: R976

### D4: why an actor stopped

`Stopped` gets one field, `reason: StopReason`, where `StopReason = Normal() | Shutdown() |
Crashed(error: str) | NodeDown()`, where `Crashed` carries the failure's shown text so it is `Wire`, and `RemoteError`'s `Stopped{reason}` is this same type (one fact, one
representation). Supervision (RFC 0056 slice 3) fills it. Until then a local call never fails
`Stopped`.

- Decision: inconsequential, most complete
- Row: R977

## Spec changes

- 9.6: the bounded-mailbox and one-way send bullets (D1, D2, D3).
- 14 (R435): the spawn sentence (D1).
- 17.3: `std.actor` lists the policy types, `SendOnly`, `Undelivered`, `Full`, `StopReason` (D1 to D4).
- 9.6.1: `RemoteError` names `StopReason` (D4).
