# Call-once function values for Yan

Study and proposal, 2026-11-04. Docs only: no spec rows, no conformance edits, no diagnostic codes.
Items are labelled D1, D2 and so on; none carries a row number. Everything here is non-normative until
an RFC and accepted rows say otherwise.

Brief: item F6 of the [language forums study](https://claude.ai/artifact/E8xVSw3fhTwuxFu38DqrtT).
Study function values that may be called at most once, as Rust's `FnOnce` and the Swift pitch
[annotate functions called at most once](https://forums.swift.org/t/pitch-introduce-a-way-to-annotate-functions-that-are-called-at-most-once/89088).
Cover prior art, capabilities, linear and affine use, determinism, lowering to Yan Core, and a
recommendation.

## 1. Answer

Add one marker on function types, `once`, beside the existing `par` prefix (4.6, R436): `once (A) -> R`
in Yan, `once Callable[[A], R]` in Yan Agent, `once Fn(let A) -> R` in Yan Core. Calling a `once` value
consumes it, as if the value were passed `sink`: after the call it is dead, and a second call is the
existing use-after-move error (E401). Nothing new is checked at run time.

A closure literal needs no annotation. Its type is `once` exactly when its body consumes one of its
own captures on some path. A plain or `par` function value is accepted wherever a `once` one is
expected; the reverse is an error. A closure that captures a `@linear` value is itself `@linear`, so
it must be called exactly once (E412), with failure paths released as R443 already says.

The capture rules of 4.6 do not change: an escaping `once` closure still cannot capture a capability
(E306), an `inout` parameter, a `var` or a view (E406). The marker adds no nondeterminism and no
capability. In std it widens the closure parameters that are called at most once (`unwrap_or_else`,
`or_insert_with`, `Once.get_or_init`, completion callbacks), which is source compatible for every
caller.

## 2. The gap today

Confirmed against the spec. A closure owns its captures (4.6; a heap-backed capture is moved at last
use or needs `copy`/`share`), and R769 lists a `sink` capture as a way for a value to leave its local.
But a function type has conventions only for its parameters (3, `FnType`), not for the function value
itself, and every call through a function value reads it. So a closure body cannot move an owned
capture out:

```
let rows: List[Row] = load()
let r: List[Row] = cache.or_insert_with(fn [sink rows] () -> List[Row] { ret rows })
```

Returning `rows` from the body would consume a capture of a value that may be called again. The only
legal body today is `ret copy rows`, an O(size) deep copy of a value that is then dropped with the
closure. The same holds for every "produce the value lazily" API (`Option.unwrap_or_else`,
`Result.or_else`, `Lazy`, `Once`), for builders that hand their accumulated state to a finishing
callback, and for completion callbacks that forward an owned buffer.

For `@linear` captures the gap is a hole, not a cost. A session endpoint (R634) moved into a callback
can be consumed by neither a read-only call nor a drop, so the callback is unusable, or, if the
checker lets the closure be dropped uncalled, the endpoint leaks past E412.

## 3. Prior art

| Language | Mechanism | Checked | Notes |
| --- | --- | --- | --- |
| Rust | `FnOnce` / `FnMut` / `Fn` traits, `Fn: FnMut: FnOnce` | statically, by ownership | Closure kind inferred from the body; `Box<dyn FnOnce>` callable since 1.35; the model Yan's ownership already matches |
| Swift | pitch for an at-most-once annotation (forum thread above); `consuming` parameters (SE-0377) and noncopyable types | proposed | The forum's long thread is mostly about definite initialization through closures and about escaping completion handlers |
| Kotlin | contracts, `callsInPlace(block, InvocationKind.EXACTLY_ONCE)` | trusted, not verified | Used so `val x` can be assigned inside `run { }`; the callee's promise is not checked |
| Linear Haskell | linear arrow `a %1 -> b` | statically | Linearity of the argument, not of the function value; one-shot functions follow from linear captures |
| OCaml 5 | effect continuations are one-shot | at run time | Second resume raises `Continuation_already_resumed` |
| Go | `sync.OnceFunc` (1.21) | at run time | Callable many times, body runs once; an idempotence wrapper, not a type |
| C++ | `std::move_only_function` (C++23) | statically, partly | Move-only but callable repeatedly; call-once remains a convention |

Two lessons. First, the only designs that grow capability without a runtime cost are the ones tied to
ownership (Rust, Linear Haskell); Yan has that ownership already. Second, Kotlin's exactly-once
contract exists for definite initialization of outer `val`s; Yan has no need for it, because a
non-escaping closure may already assign an outer `var` and the result can be returned instead (D11).

## 4. Semantics

### 4.1 Calls consume

A value `f: once (A) -> R` is called by consuming it. In the ownership analysis (4.2, R769) the call
`f(a)` is a use of `f` by `sink`. All existing rules then apply without change:

- a second call, a call in a loop body, or a call after `f` was moved is E401;
- calling `f` in two branches of one `par`, or in a `par for` body, is a `sink` conflict under the law
  of exclusivity (4.3), so no new rule is needed for concurrency (D6);
- a `once` value stored in a field is called by moving it out first, as with any non-copy field; the
  idiom for an optional callback is `var cb: Option[once () -> ()]` and `cb.take()`.

### 4.2 Which closures are `once`

A closure literal's type is `once` when its body, on some path, consumes a non-copy capture (moves it,
passes it `sink`, returns it, or stores it in a constructor). This is decided by the same per-function
ownership analysis that `yan-check` already runs, applied to the closure body with captures as owned
locals. A closure that never consumes a capture is a plain function value, so writers of today's code
see no change (D3).

### 4.3 Coercion, no subtyping

Yan has no subtyping, so the order `par` then plain then `once` is an implicit coercion at the points
where an expected type is known: arguments, binds with a declared type, returns and constructor
fields. A plain or `par` value given where `once` is expected is accepted unchanged. A `once` value
given where a plain function is expected is an error whose fix is either to mark the callee's parameter
`once` (offered when the callee's body calls it at most once on every path) or to `copy` the consumed
capture (D2, D10). `par once` is rejected: a value that can be called once cannot be called from
several branches (D6).

### 4.4 Affine and linear

`once` is affine: at most one call. Dropping an uncalled `once` value drops its captures, in the order
of R770. That is the right default for completion callbacks and lazy defaults, where not calling is
normal.

A closure that captures a `@linear` value cannot be dropped, because that would drop the capture. So
such a closure is itself `@linear`: its type is `once`, and E412 requires it to be consumed, which for
a function value means called or passed `sink` onward. On a path already propagating a failure,
including `Timeout` and `Cancelled` from a cancel scope (9.4, R472), the closure is released like any
`@linear` value (R443): the drop thunk runs the release intrinsics of its linear captures (D4). This
gives exactly-once callbacks for protocol endpoints (R634) without a separate `exactly_once` marker.

## 5. Capabilities

No change to the capability rules, by design (D7).

- An escaping `once` closure is still an escaping closure: it cannot capture a capability (E306) or an
  `inout` parameter, `var` or view (E406). Being called at most once does not make a captured
  capability safe to carry past its scope, because the call can happen after the scope ends.
- A non-escaping `once` argument may use the caller's capabilities and `var`s, and is fail-transparent,
  as every non-escaping closure argument is (4.6). This is where most of the value is:
  `opt.unwrap_or_else(fn [sink fallback] () -> T { ret fallback })` inside a function with `fs` in
  scope works without a copy and without new rules.
- `uses` on a `once` function type means what it means on any function type.
- Not proposed: a "single-use capability" built from a `once` closure. Attenuation is a handler's job
  (`net.only`, `proc.only`, R674), and a closure cannot carry a capability out of its scope anyway.

## 6. Determinism

`once` adds no source of nondeterminism. Whether a value is called is decided by the program's control
flow, as for any move. Three interactions are worth stating.

- **`Once` and `Lazy` (R393).** When several tasks race on one `Once`, the winner's initializer runs
  and the losers' are dropped uncalled. Which closure runs is already a scheduling outcome, which is why
  R393 requires such an initializer to use only `alloc` or else `uses nondet`. Widening the parameter to
  `once` changes nothing there. The losers' captures are dropped, and drop bodies use only `alloc`
  (R343), so the drop order is unobservable. A `@linear` closure cannot be passed to `get_or_init`,
  since a loser could not be dropped; that falls out of D4.
- **Cancellation.** A `once` callback held by a cancelled scope is dropped uncalled. That is a normal
  affine drop, ordered by R770, and is deterministic given the cancel point.
- **Replay.** Calls through `once` values are ordinary calls, so the replay log (R305) records nothing
  new.

## 7. Lowering to Yan Core

No runtime flag, no new Core statement.

1. **Type.** Core `FnType` gains the `once` marker, printed and hashed like the `par` prefix. Because
   it changes what callers may do, it is part of a function's signature hash.
2. **Calls.** A call through a `once` value lowers as a call that takes the value's environment by
   `sink`. In checked Core the callee's body sees the captures as owned locals and may move them
   (R769); the environment record has no custom `Drop`, so partial moves out of it are legal (4.2).
3. **Closed lambda sets (R622).** Unchanged. The value is a tagged record of its captures and the call
   is a direct call or a switch on the tag; the record is passed `sink`, which R384 lowers to a pointer
   to the caller's storage, so no copy is emitted.
4. **Open sets and `dyn`.** The value is the usual code pointer, environment pointer and drop thunk.
   A `once` call transfers the environment block to the callee, which frees it after moving out what it
   uses; an uncalled value runs its drop thunk. A plain function value coerced to `once` reuses its
   code with a wrapper that drops the environment after the call.
5. **Drop elaboration (R770).** A `once` value consumed on some paths and not others is dropped on the
   edge where it is still owned, as for any conditionally moved local; a `@linear` one follows the
   `@linear` exception of R770.
6. **Tail calls.** `become f(args)` through a `once`-typed parameter is legal under R260's rule for
   `Fn`-typed parameters; the parameter's conventions include the marker, so E271 compares it.

## 8. FFI and remote

- **C callbacks (12).** Registered callbacks already carry owned captures and a drop thunk. C cannot
  be trusted to call a callback once, so a raw `extern` callback type stays plain. std offers a one-shot
  wrapper whose trampoline holds one word of state and panics on a second call (D9). This is the only
  place a runtime check appears, and only at the system boundary.
- **Remote.** Closures never cross a machine (R407), so `once` has no remote form. A reply pattern
  across nodes uses a handle and a one-way send ([actor send study](actor-send.md)), not a closure.

## 9. Surfaces and agents

- **Yan:** `once (A) -> R`; with the `par` prefix grammar of 4.6 this is one more prefix word.
- **Yan Agent:** `once Callable[[A], R]`, mirroring `par Callable[[A], R]` (14, R436).
- **Agents rarely write it.** Closure literals are inferred (4.2), so the marker appears only in
  signatures of higher-order functions, which E240 already makes explicit. The common error, passing a
  consuming closure to a plain parameter, gets an autofix (D10) that either widens the callee or inserts
  `copy`, so the agent loop needs no model generation for it.
- **Lenient reader.** Python has no equivalent; nothing to map.

## 10. Decisions

| Item | Question | Recommendation |
| --- | --- | --- |
| D1 | Add a `once` marker on function types, whose call consumes the value? | Yes: `once (A) -> R`, `once Callable[[A], R]`, `once Fn(...)` in Core; checked only by ownership |
| D2 | How do plain, `par` and `once` relate? | Implicit coercion `par` to plain to `once` at expected-type positions; no subtyping; `once` to plain is an error |
| D3 | Is a closure literal's `once`-ness inferred? | Yes, `once` iff its body consumes a non-copy capture on some path; no annotation on literals |
| D4 | A closure capturing a `@linear` value? | It is `@linear` and `once`: called exactly once (E412), released per R443 on failure paths |
| D5 | Which std parameters widen to `once`? | The at-most-once ones: `Option`/`Result` `*_or_else` and `map_or_else`, map-entry `or_insert_with`, `Once.get_or_init`, `Lazy` constructor, completion callbacks; source compatible |
| D6 | Concurrency rule? | None new: two calls in a `par` are an exclusivity conflict; `par once` is rejected |
| D7 | Capability rules? | Unchanged: E306 and E406 apply to escaping `once` closures; no single-use capability via closures |
| D8 | Runtime representation? | No flag; call takes the environment by `sink`; R622 unchanged; open sets use code, env and drop thunk |
| D9 | FFI callbacks? | Raw `extern` callbacks stay plain; std one-shot wrapper panics on a second call |
| D10 | Fix for a `once` closure given to a plain parameter? | Autofix: widen the callee's parameter when it provably calls it at most once, else `copy` the capture |
| D11 | Kotlin-style definite initialization through exactly-once closures? | Non-goal |

## 11. Cost

Type checker: one bit on function types, one coercion, and closure-kind inference reusing the
existing ownership analysis. Core: the marker in `FnType`, printer and hash. Lowering: the `sink`
environment convention and the open-set wrapper. std: parameter widenings in D5 and the FFI wrapper in
D9. Conformance would need cases for E401 on a second call, inference, coercion in both directions, the
`@linear` closure under failure and cancellation, and `par once`; those go in a separate PR by the lead.

## 12. Decisions (Antonio, cards, 2026-11-04)

D1, D4, D5, D8 and D9 accepted as recommended. Six answers differ from the recommendation:

- D2: `par`, plain and `once` function types are related by real subtyping, not only implicit
  coercion at expected-type positions. The RFC must define variance for function types and generics.
- D3: a closure literal's `once`-ness is annotated explicitly, not inferred from its body.
- D6: `par once` is allowed: a `once` value may be handed to one of several parallel branches. The RFC
  must say who may call it and how two branches are kept from both calling it.
- D7: a `once` closure may carry a single-use capability. The RFC must define the capability rules.
- D10: a `once` closure given to a plain parameter is a diagnostic only, with no autofix.
- D11: Kotlin-style definite initialization through exactly-once closures is supported, which needs
  definite-call analysis beyond ownership.
