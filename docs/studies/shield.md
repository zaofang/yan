# Shielded sections for Yan cancel scopes

Study and proposal, 2026-11-04. Docs only: no spec rows, no conformance edits, no diagnostic codes.
Items are labelled D1, D2 and so on; none carries a row number. Everything here is non-normative until
an RFC and accepted rows say otherwise.

Brief: item F5 of the language forums study.
Should Yan have a `shield` construct, a section that outer cancellation cannot interrupt, as in
`asyncio.shield` or trio's `CancelScope(shield=True)`? Cover prior art, the interaction with
deadlines and `Timeout` (9.4), capability-safety, a recommendation and open decisions.

## 1. Answer

Yes, in one bounded form: `with shield(d):`. Inside it, cancellation and deadlines from enclosing
scopes are held back, and the block runs under its own fresh deadline `now + d` instead. The duration
is mandatory, so a shield is a cancel scope that replaces the inherited deadline rather than
tightening it. When the block ends, any cancellation that arrived meanwhile is delivered at the next
cancellation point, as before. The shield grants no authority, needs no new capability, and cannot
outlive the drain and cancel grace that already bound shutdown.

Unbounded shields (trio, Kotlin `NonCancellable`) and task-orphaning shields (`asyncio.shield`) are
both rejected.

## 2. The gap today

Three existing rules combine into a hole:

1. Cancellation is delivered at every blocking, fallible capability operation (9.4). In a cancelled
   task, each further `send`, `write` or `recv` fails `Cancelled` at once.
2. A `drop` body does not block and is not a cancellation point (5.4). So cleanup that must talk
   to the outside cannot live in `drop`.
3. A function under an expired deadline cannot open a longer one: nested scopes keep the earliest
   instant (Yan Core `deadline at { ... }`).

So after a cancel, a program cannot send a TLS `close_notify`, a database `ROLLBACK`, a final journal
record, a "leaving" message to a peer, or flush a buffered writer. It can only drop the handle and let
the other side time out. Std already masks one case internally: an in-flight I/O operation that is
cancelled waits, masked, for its completion (9.7, borrowed buffers). User protocols and libraries
have no equivalent.

Today's workaround is to catch `Cancelled` and do the cleanup in another task that was not cancelled,
which breaks structured concurrency and is wrong under drain, where every task in the scope is
cancelled together.

## 3. Prior art

| System | Construct | Bounded? | What it teaches |
| --- | --- | --- | --- |
| trio | `CancelScope(shield=True)` | No | Outer cancels wait until the scope exits; pending cancel is delivered at the next checkpoint. Docs tell users to combine it with `move_on_after` so cleanup cannot hang. |
| AnyIO | `CancelScope(shield=True)`, `with move_on_after(d, shield=True)` | Optional | The combined form is the idiom; AnyIO added the `shield=` argument to timeout scopes for exactly this. |
| asyncio | `asyncio.shield(aw)` | No | Protects the inner task, but the awaiting caller is still cancelled and the inner task runs on detached. An orphan; conflicts with structured concurrency. Python 3.11 `TaskGroup` and `timeout` do not compose with it cleanly. |
| Kotlin | `withContext(NonCancellable)` | No | The documented cleanup idiom in `finally`. Widely misused outside `finally`, where it silently makes work uncancellable. |
| Go | `context.WithoutCancel(ctx)` (1.21) | No, but usually wrapped | Typical use is `WithTimeout(WithoutCancel(ctx), d)`: drop the parent's cancel and deadline, then impose a fresh one. That pair is the bounded shield. |
| Haskell | `mask`, `uninterruptibleMask`, `bracket` | No | `bracket` masks async exceptions in acquire and release; interruptible operations inside `mask` can still be interrupted. `uninterruptibleMask` is discouraged because it can hang `killThread`. |
| Rust | none; async drop debate | n/a | Dropping a future cancels it at any `.await`; cleanup that must await has no home. Years of async-drop design work exist for this gap. |
| Swift | none; cooperative `Task.isCancelled` | n/a | A task may ignore cancellation, so shielding is implicit and unbounded. |
| Erlang/OTP | supervisor shutdown timeout, `trap_exit` | Yes | A child gets a fixed shutdown budget to clean up, then `brutal_kill`. Bounded cleanup as a supervision parameter. |
| C# | `CancellationToken.None` for cleanup | No | Pass a fresh token to cleanup calls; same shape as Go. |

The pattern across the successful designs: drop the outer cancel, impose a fresh bound, deliver the
held cancel afterwards. Every unbounded form has a documented hang or misuse story.

## 4. Proposed semantics

Yan Agent spelling; plain Yan follows its own block form.

```
def close(conn: Conn, grace: Duration) -> fail IoError | Timeout uses net:
    with shield(grace):
        conn.send_close_notify()?
        conn.flush()?
```

- Inside `with shield(d):`, the effective deadline is `time.monotonic() + d` taken at entry,
  regardless of enclosing deadlines. Nested `deadline` and `timeout` scopes inside the shield keep the
  earliest instant as usual, so they can only tighten it.
- Cancellation of an enclosing scope (a sibling failure in `par`, drain's cancel, a caller's expired
  deadline) is recorded but not delivered inside the block.
- The block's own expiry is `fail Timeout`, as for any deadline scope. A function that opens a shield
  has `Timeout` in its fail type, mirroring the existing rule for `deadline` and `timeout`.
- On exit, a recorded cancel is delivered at the next cancellation point after the block; the block's
  value or failure is returned normally first. The shield exit is not itself a cancellation point, so a
  value computed in the shield is never lost.
- Child tasks, `par` branches, actor `call`s and `Remote` calls made inside the shield inherit the
  shield's deadline, not the outer one. A `Remote` call sends the shield's remaining budget.
- Drain is not cancellation: intake operations inside a shield still fail `Draining`.
- Under drain, the shield's instant is clamped to the end of the enclosing scope's cancel grace, so a
  shield fails `Timeout` before the process-level abort instead of being killed by it.
- Determinism: cancellation is still delivered only at capability operations, and under virtual time
  the shield's expiry is a deterministic instant. Nothing new reaches the replay tier.

Yan Core: a sibling of `deadline at { ... }`, `shield at { ... }`, whose rule differs in one clause:
the body's instant is `at`, not `min(at, inherited)`, and inherited cancellation is held. `with
shield(d)` lowers to `shield time.monotonic() + d { ... }`.

## 5. Capability-safety

A shield touches no resource. Every operation inside it still needs its capability in `uses`, so it
grants no authority. What it changes is liveness: a caller that opened `with timeout(t)` may now wait
up to `t + d`, where `d` is the longest shield on the path.

Options for making that visible:

- **No capability (recommended).** The cost is bounded by `d`, and the process bound (drain grace plus
  cancel grace) holds absolutely. `yan audit` lists every shield with its budget and call path, as it
  already lists other liveness-relevant items.
- **Marker capability** `uses shield`, like `nondet`: visible in signatures and deniable in a ceiling.
  Honest but viral: every caller of a library `close` would need it, which pushes users towards
  skipping graceful close, the outcome the construct exists to prevent.
- **Budget in the signature**, such as `shields 2 s`: precise, but a new kind of effect annotation for a
  small gain.

The marker form can be added later if audits show abuse; removing it later would be harder.

Two rules keep the construct from becoming a general "ignore cancellation" switch:

- The duration is mandatory. There is no `with shield():`.
- A lint flags a shield whose body has no blocking capability operation (it shields nothing), and one
  whose duration is not below the default cancel grace (it would be clamped under drain anyway).

## 6. Decisions

| Item | Question | Recommendation |
| --- | --- | --- |
| D1 | Add a shield construct at all? | Yes, bounded, for cancel-safe cleanup. |
| D2 | Bounded or optional bound? | Mandatory duration; no unbounded form. |
| D3 | Clamp under drain? | Yes: the instant is clamped to the end of the cancel grace, so a shield times out before the abort. |
| D4 | Capability? | None; `yan audit` lists shields. Marker capability held in reserve. |
| D5 | When is a held cancel delivered? | At the next cancellation point after the block; the block's own result returns first. |
| D6 | Yan Core form? | New statement `shield at { ... }`, sibling of `deadline at { ... }`. Alternative: a `shield` flag on `deadline`. |
| D7 | Fail type? | Always includes `Timeout`, as for `deadline` and `timeout` scopes. |
| D8 | Drain inside a shield? | `Draining` still applies; only cancellation is held. |
| D9 | Surface spelling? | `with shield(d):` in Yan Agent and the matching `with` block in plain Yan; no new keyword. |
| D10 | Lints? | Two warnings: shield with no blocking operation; duration at or above the default cancel grace. |
| D11 | Should actor handlers shield by default? | No change: the caller's deadline is already never delivered into a running handler (9.6), so handlers need no shield for the caller's sake. |

## 7. Not proposed

- `asyncio.shield`-style detached inner tasks: they outlive their scope.
- Async drop: a `drop` that blocks would make every scope exit a cancellation point and every drop
  order observable. A shielded `close` called explicitly, with `drop` as the non-blocking fallback, gives
  the same result.
- A `finally`-style `on cancel:` block: expressible as handling `Cancelled` and then entering a shield;
  a second construct is not needed.

## 8. Next step

If D1 to D11 are accepted, an RFC after M1 with the rows for 9.4, 3.5 (Yan Core), 14.1 and 15 (surface)
and two warning codes, plus conformance cases proposed in a separate PR: held cancel delivered after
the block, clamp under drain, nested timeout tightening inside a shield, `Remote` budget inside a shield.

## 9. Decisions (Antonio, cards, 2026-11-04)

D1, D2, D3, D5, D6, D8, D9, D10 and D11 accepted as recommended. Two answers differ from the
recommendation:

- D4: shield needs a marker capability, `uses shield`, visible in signatures and deniable in a ceiling.
  `yan audit` still lists shields. The cost noted in section 5 stands: library `close` functions that
  shield make every caller declare it, so the RFC must say how a caller forwards or confines it.
- D7: a shield's expiry is a separate error type (called `ShieldTimeout` in the card), not `Timeout`.
  A function that opens a shield carries that type in its fail type. Whether it is a subtype of
  `Timeout` is left to the RFC.
