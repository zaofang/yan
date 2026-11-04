# Saving and restoring a seeded generator

Study and proposal, 2026-11-04. Docs only: no spec rows, no conformance edits, no diagnostic codes.
Items are labelled D1, D2 and so on; none carries a row number. Everything here is non-normative until
an RFC and accepted rows say otherwise.

Brief: item F12 of the [language forums study](https://claude.ai/artifact/E8xVSw3fhTwuxFu38DqrtT),
from the TC39 [SeededPRNG](https://github.com/tc39/proposal-seeded-random) proposal (Stage 2), which
fixes ChaCha12 behind `Random.Seeded` so a seed means the same stream everywhere and adds `getState` and
`setState` to save and resume it. The forums study rated F12 half covered: Yan already fixes the
streams of `seeded(seed, algorithm=ChaCha8())` and how they split (R683 to R685), but has no way to
snapshot a stream mid-run. It recommended `state()` and `from_state(s)` on the seeded handler, values
only, no new capability. This study keeps that shape and settles what a state is, where the operation
lives, and how it meets splitting, replay and `@durable`.

## 1. Answer

Yes, as a position in a frozen stream, not as opaque bytes:

    let s = rand.state()?               # RandState(algorithm=ChaCha8(), seed=42, position=1017)
    fs.write("ckpt/rand.wire", wire.encode(s))
    ...
    with rand = seeded(s.seed, s.algorithm, at=s.position) {
        resume_simulation(...)
    }

- `RandState` is a plain public record `{ algorithm: Algorithm, seed: uint64, position: uint64 }`,
  where `position` counts `next_u64` draws. It derives `Wire`, `Eq` and `Show`. Every value is valid.
- `state() -> Option[RandState]` is a new operation on the `Rand` trait with a default body returning
  `None`. The `seeded` handler returns `Some`; `os()` and handlers that do not override it return
  `None`.
- Restoring is a constructor argument: `seeded(seed, algorithm=ChaCha8(), at=0)`. There is no
  `from_state` method and no `set_state` on a live handler; a resumed run binds a new handler with
  `with`, as every other handler change does.
- No new capability, no new syntax, no diagnostic. Reading the state uses `rand`; building a seeded
  handler needs nothing, as today.

## 2. The gap today

17.3 defines the two algorithms, the derived operations and the split by branch index, and freezes
every stream with the module (R683 to R685). A program can reproduce a run from its seed, but only by
replaying every draw from the start. Grep finds no state, position or seek operation in sections 7, 11
or 17. A long simulation, a Monte Carlo job on the `job` platform that is preempted, or a procedural
world saved to disk has two choices today: count draws itself and burn through them again on resume,
or write its own generator outside `std.rand` and lose the R683 guarantees.

The forums study also named `@durable` as a fit. On reading 9.6.1 it is not: R416 journals every
capability result, so a resumed durable run gets its earlier draws back from the journal and continues
on the live handler from its end (R453). Durable functions need nothing from this proposal. The real
users are programs that checkpoint their own state outside a journal.

## 3. Prior art

- **TC39 SeededPRNG** (Stage 2): `getState()` returns a `Uint8Array`, `setState()` and a
  `Random.Seeded.fromState()` constructor take one back. The bytes are the full ChaCha12 state, so an
  arbitrary array can be an unreachable state; the proposal validates length only.
- **Go `math/rand/v2`**: `ChaCha8` and `PCG` implement `MarshalBinary` and `UnmarshalBinary`. The
  ChaCha8 encoding is seed plus position in a tagged format, close to what this study proposes.
- **Rust `rand_chacha`**: `get_seed`, `get_word_pos` and `set_word_pos` (a 68-bit word index) and
  `get_stream`/`set_stream`, plus serde behind a feature. Yan's ChaCha8 stream is defined as
  `rand_chacha` 0.9's (R683), so the seek is already implemented underneath.
- **NumPy**: `BitGenerator.state` is a dict (`{"bit_generator": "PCG64DXSM", "state": {...}}`) that can
  be read and assigned; `advance(delta)` jumps a PCG stream in O(log delta), and `jumped()` returns a
  far-away copy.
- **Python `random`**: `getstate()` and `setstate()` over a versioned tuple of 625 Mersenne Twister
  words. Opaque in practice, and tied to one algorithm.
- **C++ `<random>`**: every engine streams its state as text with `<<` and `>>`, and `discard(n)`
  skips ahead.
- **JAX**: there is no hidden state at all; a `PRNGKey` is a value, and `split`/`fold_in` make new
  keys. Saving the key is saving the generator.

The pattern that ages best is Go's and JAX's: the state is a small value that names a point in a
defined stream. Full-register snapshots (Python, TC39) are larger, can encode states no seed reaches,
and change when the algorithm's internals do.

## 4. Design

### 4.1 State as a position

R683 builds both algorithms from one `uint64` seed through SplitMix64, and R684 defines every derived
operation through `next_u64` only, with no buffered bits. So after `n` draws a seeded handler is fully
described by `(algorithm, seed, n)`. Storing that triple instead of the ChaCha8 key and buffer or the
PCG registers gives a state that is 17 bytes on the wire, readable in a log, comparable with `==`, and
always valid. Because streams are frozen with the module (20.5), a state saved by one toolchain means
the same stream on every later one; no version field is needed beyond the `Wire` schema hash.

`position` is a `uint64` count of `next_u64` draws, not ChaCha words or blocks, so it means the same
thing for both algorithms. 2^64 draws is centuries at a draw per nanosecond; a handler that reaches it
panics with `overflow`.

### 4.2 Seeking

`seeded(seed, algorithm, at=n)` starts the stream at draw `n`. For ChaCha8 this sets the block counter
to `n / 8` and skips `n mod 8` draws, O(1). For Pcg64Dxsm it is the standard LCG jump-ahead
(Brown 1994, NumPy `advance`), about 128 multiply-adds. `at` also covers skip-ahead, so no separate
`advance` or `discard` operation is proposed.

### 4.3 Where `state()` lives

A capability cannot be stored or returned (E306), and code that `uses rand` is typed against the `Rand`
trait, not the handler bound by the caller. A method on the concrete `Seeded` type would therefore be
unreachable from the code that draws. Putting `state()` on the trait, with a default `None`, lets any
function that `uses rand` ask, keeps user-written handlers (fakes, recorders) compiling unchanged, and
leaves `os()` honest: an OS-seeded generator has no reproducible position, so it says `None`.

The handler of `rand` is resolved at compile time (E307), so in a monomorphized build the `None` arm
for `seeded` and the `Some` arm for `os` fold away.

### 4.4 Splitting

Inside a `par` branch, `state()` returns that branch's stream: `seed` is the derived branch seed of
R685, and `position` counts the branch's own draws. Restoring it outside the `par` gives the same
stream, and nested `par` blocks under the restored handler split the same way as before, because R685
derives branch seeds from the seed alone.

Reading R685 for this exposed a wording gap. 17.3 says branch `k` is seeded with
`splitmix64(parent_seed ^ splitmix64(k + 1))`, which as written gives branch 0 of two consecutive
`par` blocks in the same task the same stream. 7.7 (SQ18) says instead that the nonce is a stable hash
of the task path, and the R681 to R695 registry entry says "split by task path". If the task path
includes which `par` the branch belongs to, the split rule reads something besides the seed, and
`RandState` must carry it too (a per-task split counter, 8 more bytes). D6 asks the lead to settle the
rule before the field list is fixed.

### 4.5 Record, replay and durable

`state()` is an ordinary `rand` operation. Record handlers log it, replay handlers feed it back, and
`@durable` journals it, with no special case. Its result is a function of the draws before it, so it
adds no nondeterminism and law 7 is untouched.

### 4.6 What stays out

- `crypto_rand` gets no `state()`. Its outputs must not be reproducible, and a readable position would
  invite using it as a seeded generator.
- The `sched` capability's `seeded(seed)` handler and `std.sim` hosts are not covered. A simulation
  checkpoint needs task trees, virtual time and host state as well, which is a separate study.
- No `set_state` on a live handler. Rebinding with `with` is the one way a handler changes.

## 5. Cost

- Std: one record type, one trait operation with a default, one `at` parameter, a seek for each
  algorithm (ChaCha8 already has it in `rand_chacha`; PCG jump-ahead is about twenty lines).
- Spec: rows for `RandState`, `state()`, `at=` and the seek definitions in 17.3, the `Rand` operation
  list in the 7.1 table, and whatever D6 decides for 7.7 and R685. No Core change, no new syntax, no
  diagnostic code.
- Tests: conformance cases that `state()` after `n` draws equals `position=n`, and that
  `seeded(s, at=n)` continues the stream bit for bit, for both algorithms and inside a `par` branch.
  Those are lead-owned and go in a separate PR.

## 6. Open decisions

- **D1. Adopt state and restore.** `state()` plus `at=`. Recommended: yes.
- **D2. State as a position record.** `RandState { algorithm, seed, position }`, public fields,
  `Wire`, `Eq`, `Show`, every value valid. Recommended: yes. Alternative: opaque bytes as in TC39,
  which would allow algorithms not built from a `uint64` seed later.
- **D3. Trait operation with default `None`.** `state() -> Option[RandState]` on `Rand`.
  Recommended: yes. Alternative: a method on the concrete seeded handler only, which code under
  `uses rand` cannot reach.
- **D4. Restore by constructor.** `seeded(seed, algorithm=ChaCha8(), at=0)`, no `from_state` and no
  `set_state`. Recommended: yes. Alternative: also add `seeded_from(state)` as sugar.
- **D5. Position unit.** Count of `next_u64` draws, `uint64`, panic with `overflow` at the end.
  Recommended: yes.
- **D6. Split rule wording.** Decide whether a branch seed depends on the seed and branch index only
  (17.3 as written; consecutive `par` blocks then share branch streams) or on the task path (7.7). If
  the latter, `RandState` gains a split counter. Recommended: task path with a per-task split counter,
  so consecutive `par` blocks draw independent streams, fixed in the same rows PR.
- **D7. No separate skip-ahead.** `at=` covers `advance` and `discard`. Recommended: yes.
- **D8. No `crypto_rand` state.** Recommended: yes.
- **D9. Replay and durable.** `state()` is recorded and journaled like any `rand` operation.
  Recommended: yes.
- **D10. Scheduler and sim checkpoints.** Out of scope here. Recommended: record as a possible later
  study, not a 1.0 item.
- **D11. Milestone.** The change is additive and blocks nothing. Recommended: after M1, in the next
  std minor once the rows land.

## 7. Not proposed

- A `jumped()` or `fork()` that returns an independent stream. R685's split already gives independent
  streams per branch.
- A manifest or audit fact for seeded generators. Nothing about authority changes.
- Algorithm-specific state accessors (ChaCha key, PCG increment). The position record hides them on
  purpose.

## 8. Next step

If D1 to D11 are accepted, an RFC from `rfcs/0000-template.md` with the `RandState` type, the `state()`
signature, the `at=` seek for both algorithms and the D6 split rule, then rows in the same PR as the
7.1, 7.7 and 17.3 edits.
