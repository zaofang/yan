# 0093: `Atomic.wait` on pool workers, native deadline scopes and `Type.Variant()` paths

- Status: accepted
- Research: code reading for #1703 (no study; the facts are cited below)
- Rows: R1109 (D3); D1 and D2 rows land with their slices

## Summary

Three gaps left by RFC 0089 slice 1 and PR #1730 (the OS futex in `yan-rt`) are one design question
each, and none is a bookkeeping row. First, no pool worker can reach `wait`, because RFC 0086 D5
keeps `Inst::Atomic` and the futex calls out of pool arms (`yan-mir/src/pool.rs`), so the OS path is
unit-tested but unreachable from a program. Second, `wait` fails `Timeout` only in the interpreter,
because MIR and native code have no deadline scopes (`K::Deadline` is unsupported in
`yan-mir/src/lower/stmt.rs`; #835 item 17). Third, conformance cases and the `.yn` lowering spell a
variant through its type, `LoadOrdering.Acquire()`, which spec 15 does not define (it defines `B()`
and a module-qualified `schema.Grocery()`, R652). Stage claimed: right.

## Decisions

### D1: whether atomics and `wait` may run on pool workers

RFC 0086 D5 sends an arm with an atomic to the inline path so the pool never trades determinism for
speed, and D6 compares byte for byte across thread counts. A `wait` in one arm released by a `notify`
in another cannot be inline: the interpreter runs arms in source order, so the first arm deadlocks
(R396) and the pool would not. Options: (a) keep atomics inline, so `wait` outside `par concurrent`
is a deadlock on every backend and the OS futex in #1730 serves only FFI-free future threads;
(b) admit `Inst::Atomic`, `Fence`, `FutexWait` and `FutexNotify` to `pool_safe`, and define a plain
`par` whose arms use `wait` as concurrent for that block (an arm that waits makes the block
`par concurrent`, with an E-code on `par` that mixes it with `sink` exits); (c) admit only the
counter-style operations (`fetch_*`, `load`, `store`, `compare_exchange`) and keep `wait` and
`notify` inline. Recommended: (c) first, then (b) once INNOV-2 (#1583) says `--ub` and native-live
sweeps are clean, because D6's matrix can already check (c) (a result that is the same under every
interleaving) and (b) needs a deterministic definition of which arm a `wait` may block on. (a) is the
deferral option and is cheap to revive.

- Decision: accepted as recommended, (c) first then (b) behind INNOV-2 (lead card, 2026-11-19)
- Row: with S3

### D2: native deadline scopes, and what `wait` returns under one

`with deadline` and `with timeout` (9.4, 9.6) exist only in the interpreter. `wait` cannot fail
`Timeout` natively until MIR carries a scope. Options: (a) a scope stack in `yan-rt` (a thread-local
absolute monotonic reading, pushed and popped by two `RtFn`s that `Term::Unwind` pops), with every
blocking operation (`recv`, `send`, actor calls, `wait`, `sleep`) reading the innermost reading and
returning `TIMEOUT` (already status 6 in `actor.rs`); (b) lower the scope as a closure argument to
each blocking call; (c) leave deadlines interpreter-only and document native as refusing a program
with `with deadline` loudly (practice 13). Recommended: (a), because it is one representation for
every blocking operation (practice 12), `wait_os` already takes the deadline, and the machine needs
the same reading to agree. Virtual time under a `clock` handler (9.6) must drive the same reading in
the machine and interpreter; native uses the monotonic clock. This is the larger slice and gets its
own issue; (c) is the interim and is what native does today.

- Decision: accepted as recommended, (a) a scope stack in `yan-rt` (lead card, 2026-11-19)
- Row: with S2

### D3: `Type.Variant()` as a spelling

A sum variant is written `B()` where it is in scope, and `mod.B()` for another module's. A case and
the `.yn` lowering use `LoadOrdering.Acquire()`, which resolves because the checker treats a type
name as a path root, but no row says so. Options: (a) define `T.V(args)` for a sum `T` as the
qualified spelling of `V`, always legal, required when two sums in scope share a variant name (E-code
on the ambiguous bare form), and printed by the formatter only where it disambiguates; (b) forbid it
and make std export the variants bare (`Acquire()`), which breaks the spelling the std atomic table
(17.3) and RFC 0089 use; (c) accept it for std ordering types only. Recommended: (a), because a
sum's variants are namespaced by it in every sum-typed language that Yan's users come from, the
checker already accepts it, and (c) is a special case with no principled edge. The row lands with a
case that spells it in each position (expression, pattern, argument).

- Decision: accepted as recommended, (a) (lead card, 2026-11-19). The formatter rule (print `T.V` only where it disambiguates) is not in S1: the formatter keeps the written spelling
- Row: R1109

### D4: slices

S1 D3 (small, no backend work, unblocks nothing but the row). S2 D2 scope stack and `Timeout` for
`wait`, `recv`, `send` and actor calls, with interpreter, machine and native outputs attached.
S3 D1 (c), then (b) behind INNOV-2. #1703 closes with S2 and S3; the deferral options keep a
revival path.

- Decision: accepted as recommended (lead card, 2026-11-19). S1 landed with D3: Yan parses `T.V` patterns and lowers `T.V` expressions
- Row: none

## Spec changes

15 (D3: qualified variant spelling), 9.4 and 9.6 (D2: native scopes), 6.2 and 6.5 (D1: which atomic
operations a pool arm may use, and when a plain `par` is concurrent), 17.3 (std atomic table).
