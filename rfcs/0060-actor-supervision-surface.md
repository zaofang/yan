# 0060: Actor supervision: the surface and what stops an actor

- Status: draft
- Research: none; follows RFC 0056 (accepted) slice 3 and 9.6. Gap issue #835.
- Rows: filled in when accepted

## Summary

RFC 0056 D3 fixed what a restart restarts from (`ChildSpec`, R958) and D2 the root supervisor (R957),
but 9.6 and 17.3 leave four things open that slice 3 needs to run: how a program writes a supervisor
spec, how it starts a `ChildSpec`, what stops an actor (and so triggers a restart), and where an
escalation goes when no supervisor is above. The interpreter implements the recommended option of each
(slice 3); native follows in slice 4.

## Decisions

### D1: writing a supervisor spec

`with supervisor(spec):` takes a `std.actor.Supervisor`. Three constructors name the strategies the
way 9.6 spells them: `actor.one_for_one`, `actor.one_for_all` and `actor.rest_for_one`, each
`(intensity: int = 1, period: Duration = 5 * s, shutdown: Duration = 5 * s) -> Supervisor`, so 9.6's
defaults are the parameter defaults.

Options: (a) the three constructors; (b) one `Supervisor(strategy=OneForAll(), ...)` record with a
variant type for the strategy; (c) keyword arguments on `supervisor(...)` itself. (a) reads like 9.6
and needs no variant spelling; (b) is longer at every use; (c) makes `with supervisor(...)` a second
call syntax.

- Decision: (a), inconsequential, closest to 9.6
- Row: none until accepted

### D2: starting a restartable child

`actor.start(ChildSpec(start, (args...), restart=permanent), capacity=1024) -> Actor[S]` where `start`
is a top-level function returning `S` with no fail type, `args` a tuple evaluated once, and `restart`
one of `permanent` (default), `transient`, `temporary`. Each start and restart calls `start(*args)`.
`actor.spawn(state)` stays the temporary form (R958).

Options: (a) a separate `actor.start`; (b) `actor.spawn` accepting a `ChildSpec` as its state. (b)
makes a `ChildSpec` value ambiguous with a state of that type.

- Decision: (a), inconsequential
- Row: none until accepted

### D3: what stops an actor

9.4 and 8 leave no in-process crash: a panic aborts the process (R5). The only failure a handler has
is its fail type.

Options:

- (a) A handler failure stops its actor. The caller still gets the failure; queued messages are
  dropped; the supervisor restarts the actor by its restart type and strategy. A failed handler may
  have left the state half updated, so the actor restarts from `start` instead of keeping it. This
  is the reading of RFC 0056's slice 3 case ("restart after a handler failure") and of R408, whose
  `Stopped{reason}` reasons include the handler's failure.
- (b) A handler failure is only a reply; the actor keeps its state. Then nothing in-process stops an
  actor, `transient` and the restart intensity never apply, and a half-updated state survives.

`transient` and `permanent` differ only once a normal stop exists (no surface for one yet); every
failure is abnormal.

- Decision: (a) recommended; question card on the slice 3 thread
- Row: none until accepted

### D4: escalation

9.6: exceeding the intensity stops the supervisor and escalates. A `with supervisor` scope is code,
not a restartable child, so no supervisor above can restart it, and the root has no parent.

Options: (a) stop the supervisor's children in reverse start order and panic with kind `supervisor`,
since a process is the next isolation boundary (8); (b) stop the children and continue, with calls
failing `Stopped`; (c) cancel the scope's body (9.4). (b) hides a crash loop; (c) needs a supervisor
to own a task, which slice 3 does not have.

- Decision: (a), inconsequential, fail loud
- Row: none until accepted

### D5: shutdown

A scope's children stop in reverse start order when the scope ends, however it ends (return, break,
failure). The per-child shutdown timeout bounds a stop that has to wait for a running handler; in the
interpreter a stop never waits, so it is accepted and not reached.

- Decision: as above, inconsequential
- Row: none

## Spec changes

- 17.3 `std.actor`: `Supervisor`, `one_for_one`, `one_for_all`, `rest_for_one`, `start` (D1, D2).
- 9.6: a handler failure stops its actor (D3); escalation from a scope or the root panics (D4).
