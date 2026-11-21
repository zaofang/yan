# 0094: Channel constructor spelling and the near-miss diagnostic

- Status: accepted
- Research: R567 hard rerun, #1727 item 2 (`bench/r567/results/hard-2026-11-18b/`, `hard-sonnet-s3/hard_channel_capacity.*`)
- Rows: pending

## Summary

The constructor already exists: `Channel[T](capacity)` returns `(Sender[T], Receiver[T])` (spec 9.5, 17; `std.chan`; the R567 yn primer documents it). It is a call on an indexed type name, which elaboration handles by name in `yan-elab/src/lower/call_expr.rs`. The R567 failures are near misses of the name, not a missing feature. In the hard rerun the yn arm wrote `Chan[int](cap)` and got `E017 ... calling a value: not supported by this toolchain yet` followed by a bare `E200 cannot find Chan`; the repair round then wrote `nondet.channel(cap)` (E017 again) and abandoned channels. The yag arm tried `chan.`, `channel.` and `sync` (E200 each) and abandoned channels. Python's habit is one object (`queue.Queue(maxsize=n)`), Go's is `chan`. The Python-fidelity policy (R807) forbids a bare E200 here: the name must resolve, or the error must name the real constructor. This RFC picks which.

## Decisions

### D1: how a near-miss name is answered

Options:

- A. Accept `Chan[T](cap)` (and `chan`, `channel`) as spellings of `Channel[T](cap)`. Cheapest for models, but it adds a second spelling of a frozen name, and every such alias adds a spelling the formatter, reflection and the name tables must round-trip. One fact, one representation (practice 12) argues against it.
- B. Keep one spelling and make every near miss a typed diagnostic that names the real constructor: `cannot find `Chan`: channels are built with `Channel[T](capacity)`, which returns `(Sender[T], Receiver[T])`; no capability is needed for a plain send and receive` (E200, with the hint in the message as R1063 does for `type` and `open`). The repair round then lands on the right name in one step.
- C. Lenient-reader N-code that rewrites the habit to `Channel[T](cap)`. Rejected on shape alone: Python's `Queue` is one object with `put` and `get`, Yan's constructor returns two handles, and a rewrite would need dataflow to split the uses.

Recommended: B. It keeps one name and meets R807 (a real function exists; the error names it). Habit names in the table: `Chan`, `chan`, `channel`, `make_chan`, `Queue`, `queue.Queue`, `asyncio.Queue`, `sync.Channel`, `nondet.channel`.

- Decision: accepted as recommended: B, typed E200 naming `Channel[T](capacity)`, one spelling (lead, 2026-11-19)
- Row:

### D2: the E017 before the E200

`Chan[int](cap)` ends in `E017 calling a value` because the callee `Index(Name, T)` is not a known constructor, and only afterwards in E200 for the unresolved name. The E017 is noise: the callee root did not resolve. Recommended: when the callee's root name is unresolved, report only the E200 with the D1 hint and no E017. Genuine calls of a non-function value (for example `x(1)` with `x: int`) keep their present diagnostic; whether that one becomes a typed code is a separate question and is not decided here.

- Decision: accepted as recommended: only the E200 is reported when the callee root is unresolved (lead, 2026-11-19)
- Row:

### D3: hint scope, fixed table or generic near-name search

Options: (a) a fixed table of habit names per feature, as `lenient_py.rs` has today; (b) a generic edit-distance search over names in scope for every E200.

Recommended: (a) now, deterministic and testable as spec rows; file (b) as a separate RFC because it changes every E200 message and the conformance catalogue at once. Deferred, not dropped: a gap issue records it.

- Decision: accepted as recommended: fixed habit-name table now; generic near-name search deferred to its own RFC (lead, 2026-11-19)
- Row:

### D4: replace the name-keyed special case with an ordinary generic call

`Channel[T](cap)` and `Atomic[T](v)` are lowered by matching the callee name in `call_expr.rs`; the target `std.chan.Channel` is already an ordinary std function taking a type argument. Long term, one resolution path (explicit type arguments on any std generic constructor) is more correct and would let the next constructor (for example a pool or a barrier) work without a new match arm. It changes no surface behavior. Recommended: not in this RFC's slice; file it as a numbered item in a gap issue (practice 9) and decide it with the next constructor that needs it.

- Decision: accepted as recommended: not in slice 1; filed as a numbered item in a gap issue (lead, 2026-11-19)
- Row:

## Spec changes

None if D1 is B and D3 is (a) beyond: 9.5 gains one sentence naming the constructor's near-miss hint, and 25 (catalogue) gains the E200 hint text for channel names in the same row set as R1063. Rows and the case `conformance/` for the diagnostic are added only after acceptance, in the same PR (CASE WITH ROW).

## Slice 1 (after acceptance)

Elaboration only: the hint table and the D2 suppression, in a new module beside `lenient_py.rs` (practice 5). Cases: a yn and a yag program using `Chan[int](cap)` expecting the single E200 with the hint, and the existing `Channel[int](cap)` programs unchanged. Both backends are untouched; the interpreter and native outputs of the unchanged programs are attached to the PR as the Working evidence.
