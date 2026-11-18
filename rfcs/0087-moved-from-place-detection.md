# 0087: Detecting a use of a moved-from place under `--ub`

- Status: draft
- Research: code reading for #1663 (no study; the facts are cited below)
- Rows: filled in when accepted

## Summary

Spec 6.7 item 8 is "a double drop or use of a moved-from place". The checked machine
(`yan-machine`) catches only the double drop, and only at the allocator (`Memory::free` returns
`false`, `rt.rs free_block`). It cannot see a move: Yan Core spells one (`Rhs::Move`, printed
`move p.f`, decided by `yan-elab/src/lower/consume.rs` from last-use facts), but `yan-mir` erases it
(`lower/values.rs` reads a `Move` place exactly as it reads a `Proj`), and `Operand` is
Local/Int/F64/Bool/Unit/Str/Func. This RFC makes the move visible to the machine with a MIR
instruction that has no code, so a read, a borrow, a second move or a drop of a place whose value
left it is `ub(8)` at the access, naming the place and the move site. Native code and the
interpreter are unchanged. Stage claimed by the later slices: working, then right.

## Decisions

### D1: lowering emits a codeless `Inst::Move`, the machine keeps the moved state

`yan-mir` gets `Inst::Move { places: Vec<Loc> }`, emitted right after the read that consumes the
value, with `Loc = Local(Local) | Word { ptr: Operand, word: u32 }` (a flattened scalar slot, or a
word of a block, which is how a field moved out of a heap record is addressed after lowering).
Like `Inst::Arm` it generates no code in any backend; `yan-cg-cranelift`, `yan-opt` and the register
VM skip it.

The machine keeps two kinds of state. For frame locals, a per-frame bit beside each local. For
memory, a third byte state, `Byte::Moved`, beside `Uninit` and `Init(u8)` (`mem.rs`); a word
store clears it (re-initialisation), a word load of it is `ub(8)`. A `Copy` or `Scalar` that reads a
moved local is `ub(8)`; an assignment to a moved local clears it. A drop, which is MIR-generated
glue over the same loads, therefore needs no special case: dropping a record with a moved word
reads it. The existing allocator report stays for a block freed twice and keeps its message.

Conditional moves need nothing static: the marker executes on the path taken, and drop flags
(`yan_check::drop_flags`, `Fn::flags`) already guard the drops that must not run, so a drop that
runs after a move is exactly the bug being detected. The remainder drop after a partial move
(`Rhs::Rest`) reads only the unmoved words.

Alternatives weighed:

- *Status quo (allocator only).* Catches a double free of one block and nothing else: a scalar field
  or a moved `Copy`-looking slot read after the move is silent, and the report names the allocator,
  not the move. Rejected: item 8 stays half implemented.
- *Overwrite the source with `Uninit` in the machine.* Needs the same marker, and reports item 6
  or reads as defined zero. Rejected: wrong item, no move site.
- *An `Operand::Move(Local)` variant.* Exact at each use but touches every `match` on `Operand` in
  all backends and the optimiser, and invites a backend to give it meaning. Rejected: a new
  representation in every consumer for a checker-only fact.
- *Run the Core interpreter under a move-tracking mode.* The interpreter is the oracle (DL5); making
  it the checker mixes the two roles, and `--ub` is the machine's job (6.9). Rejected.
- *Dynamic marker in MIR (this proposal).* One instruction, zero cost outside the machine, exact on
  every path. Recommended: it is the only option that is complete and keeps the backends equal.

- Decision: (lead's answer, date)
- Row: (R number once accepted)

### D2: one source of moves, the ownership facts already computed

Practice 12 (one fact, one representation) applies. Which places a statement consumes is
`yan_check::own::consumption` / `consumed` / `part_move` (`yan-check/src/own.rs`), and which locals
are conditionally owned is `drop_flags`, read once into `Fn::flags`. Lowering reads those and emits
`Inst::Move`; it does not recompute last-use. Call arguments passed to `sink` parameters are
consumed by `consumed`, so the caller marks them at the call. New lowering goes in a new module
`yan-mir/src/lower/moves.rs` (practice 5). If `consumption` does not give the field path in the
form lowering needs, the fix is to extend that function, not to add a parallel walk in `yan-mir`.
Before coding, grep `yan-opt/src/drops.rs` (it emits `Rhs::Move`, line 773) and `yan-elab` copy-out
for passes that create or remove a move after elaboration, so the marker is emitted from the final
Core, not an earlier form.

- Decision: (lead's answer, date)

### D3: what counts as moved-from, and when it ends

A place is moved-from from the move until a store or assignment to that same place. Rules for the
spec text:

1. A whole local or a field path moved out is moved-from; its siblings are not (a field of a local stays readable after another field was moved).
2. A read, a `view` or `MutView` borrow, a second move, a `Copy`, or a drop of a moved-from place
   is item 8, reported at the access with the move site.
3. A moved-from place may be assigned; that ends the state. A loop that moves a place and
   re-initialises it before the back edge is defined.
4. Moving out of a `Copy` place is not a move and is never marked.
5. In safe code the static check (E401 family, 4.2) rejects every program the machine would flag, so
   item 8 is reachable only through unsafe code, foreign code or a compiler bug. The machine is the
   backstop for the last, which is the point of #1242.

- Decision: (lead's answer, date)

### D4: how a conformance case reaches the state (open; lead's choice)

Safe code cannot reach it (D3.5), so a `conformance/ub/` case needs an unsafe move-out. Today
`Ptr.read` copies the pointee (assumption A11); a double drop through it is already `ub(8)`
(`conformance/ub/double_drop.yn`).

- *(a) Add `Ptr.take`: read, and leave the pointee moved-from.* `read` stays a copy, so a peek at a
  non-`Copy` value that is then forgotten stays defined. Gives a direct case, and a `take` followed
  by a read of the source is the minimal item 8 program. Costs one std function and its rows
  (6.6, 17). Recommended: precise, no false positive, smallest change to meaning.
- *(b) Make `read` of a non-`Copy` type a move.* No new API, but `read` twice without a drop, defined
  today and in Rust, becomes UB. Rejected.
- *(c) No unsafe case; test only with hand-built MIR in `yan-machine` unit tests.* Zero spec
  surface, but item 8 then has no conformance case and the spec row has none (CASE WITH ROW).
  Acceptable only if (a) is declined; the rows then wait.

- Decision: (lead's answer, date)

## Spec changes

Rows to add on acceptance, each with its case in the same PR (numbers come from
`tools/rows.py next N` at that time; none appear here):

1. 6.7: the moved-from state, its start, its end (D3 rules 1, 3, 4) and the closed list item 8
   restated as read, borrow, move or drop of a moved-from place.
2. 6.9: the UB checker reports item 8 at the first access after a move and prints the move site in
   the `yan replay` trace.
3. 6.6, if D4(a): `Ptr.take`, its safety contract (the pointee must be live and initialised; it is
   moved-from after) and that `write` re-initialises.
4. 6.7: a partial move leaves sibling fields defined and the remainder drop reads only them (ties
   to the existing rows on `rest` and field moves, no new behaviour).

No new E or W code: item 8 is a UB report, not a diagnostic. Cases: `conformance/ub/use_after_take.yn`
(read after `take`), `ub/drop_after_take.yn` (scope-end drop of a taken owner, `-- ub: 8`),
`ub/take_then_write_is_defined.yn` (re-initialisation runs clean), and the existing `double_drop.yn`
must keep reporting item 8.

## Slice plan

One issue per slice, each under #1663 (and #1242), each a PR with its own stage claim.

1. **S1, MIR marker (working).** `Inst::Move`, `Loc`, `lower/moves.rs` reading the D2 facts; all
   backends and passes accept and ignore it; MIR printer and `yan-mir` unit tests; interpreter and
   native outputs of the conformance corpus unchanged (attach both). No machine change yet, so no
   behaviour change.
2. **S2, machine state (working, then right).** Frame bits, `Byte::Moved`, ub(8) at read and drop
   with the move site; unit tests over hand-built MIR (D4(c) coverage, always kept); `double_drop`
   still item 8.
3. **S3, `Ptr.take` with rows and cases (right).** Only if D4(a). Std function, lowering in
   `lower/ptr.rs`, rows 1 to 4 and the three cases, manifest regenerated, difftest clean.
4. **S4, audit and cost (fast).** Confirm no pass in `yan-opt` drops or reorders a marker across the
   access it guards; callgrind instruction counts of native output before and after S1 identical,
   recorded against the baseline (#350).

S1 and S2 may be developed in either order only if S2 builds its tests on hand-made MIR; the
recommended order is S1, S2, S3, S4. Rows are numbered in S3 (or in S2 under D4(c)) as the final
commit after merging main.
