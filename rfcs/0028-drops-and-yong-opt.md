# 0028: Ownership facts, drop elaboration and the `yong-opt` contract

- Status: accepted
- Research: IR levels and assessment (project file `ir-study/ir-levels-and-assessment.md`, read
  against main 815d36d, 2026-11-03)
- Rows: R769 to R773

## Summary

Native code leaks every `Str` and `List` block because nothing says where a value dies, and
monomorphization lives inside `yong-mir` lowering (`crates/yong-mir/src/lower.rs`, `struct Mono`)
although section 22 assigns it to `yong-opt`. This RFC fixes both by making `yong-opt` a pass from
checked Yong to optimized Yong. Ownership facts stay in the Yong text: checked Yong already writes
every consumption (`sink`, `move`, `ret`, constructor fields, `sink` captures), so where a value
dies is a function of the text and its types. Drop elaboration is a `yong-opt` pass that writes
`drop x` into Yong, as section 3 already allows. Monomorphization becomes the first `yong-opt`
pass and emits ordinary instance items, so the interpreter can run optimized code and every
conformance program becomes a differential test of the optimizer.

What exists today, for reference:

- Yong already has `drop Name`, `reuse Name as Ctor`, `move Place` and `copy Place`
  (`yong-core` `StmtKind::Drop`, `Rhs::Move`, `Rhs::Copy`), printed, parsed and hashed.
- `yong-check` (M0) tracks moved names per path (`moved`, joined at branches) and reports use
  after move; it exports nothing.
- `yong-elab` decides last use lexically (`used_after` over spans), not over the control-flow
  graph.
- The interpreter runs generic Yong directly; `yong-mir` keys instances on `(definition, shapes)`.

## Decisions

### D1: Ownership facts are the Yong text, computed by one shared analysis

Options weighed:

1. `yong-elab` exports its last-use and move facts as a side table. Rejected: Yong also comes from
   hand-written `.yong`, `raise`, patches and the stage-0 cache, none of which has elab's tables,
   and the facts would be surface-pipeline state, not language.
2. `yong-check` exports a side table keyed by statement. Rejected: Yong statements have no stable
   ids (hashing uses positional locals, R519), so the table breaks on any edit and cannot be
   cached by item hash.
3. Recommended: no exported facts. Checked Yong MUST make every consumption of a non-copy value
   visible in the text: it leaves its local only by `move`, a `sink` argument, `ret`, a
   constructor field, a `sink` capture or `drop`. `yong-check` owns one analysis over that text,
   per function and per place (fields tracked separately, 4.2 partial moves): definitely
   initialized, maybe initialized, moved. It is a library API of `yong-check` that `yong-check`
   uses for its use-after-move and exclusivity errors and that `yong-opt` calls for drop
   elaboration. It is recomputed on demand, never stored or hashed.

Elab keeps inserting moves (4.2 rule 1). Its lexical decision must agree with the analysis;
a disagreement is an internal error (I-code), which turns elab's span heuristic into a checked
claim.

This is not true of today's output, which is the first piece of work. On main 788505f,
`ys = xs` then `return ys` with `xs: list[int]` elaborates to a bare bind:

    fn f(let xs: List[i64], let c: bool) -> List[i64] {
        let ys: List[i64] = xs
        if c {
            ret ys
        }
        ...

Under D1 a bare bind of a non-copy name is a validator error. Elab must write `move xs` when it
owns `xs`, and here, where `xs` is a `let` parameter it cannot move, `copy xs` or E413 (4.2
rule 3). Returning a borrowed list as owned without either is the same gap, one level up.

- Decision: accepted as recommended (Antonio, card, 2026-11-03)
- Row: R769

### D2: Drops are explicit in optimized Yong, never in checked Yong

Section 3 already says `drop` and `reuse` appear only after the ownership passes. This item makes
the boundary enforceable and says how conditional moves are written.

1. Checked Yong (what elab produces, what is hashed, what `raise` prints) contains no `drop` and
   no `reuse`. A `drop` or `reuse` in checked Yong is a validator error.
2. After drop elaboration, every non-copy local is consumed or dropped exactly once on every
   path. Drops sit where section 5.4 puts them: scope exit in reverse declaration order,
   temporaries at end of statement, fields in declaration order.
3. A conditional move needs no new syntax. Recommended: the pass pushes the drop into the branch
   or join-point edge where the value is still owned, when that does not move a drop of a type
   with custom `Drop` (R343) or a `@linear` type earlier than 5.4 places it. Otherwise it
   writes a drop flag as an ordinary `var _dN: bool`, cleared at the move, and `if _dN { drop x }`
   at scope exit. The post-optimization checker accepts exactly this pattern.
4. A type with no custom `Drop` anywhere inside it frees only memory, which R385 makes
   unobservable, so the optimizer MAY drop it at its last use instead of scope exit. This is what
   reuse analysis (5.3 step 4) needs, and it removes most drop flags.
5. `drop x` is one primitive: run the drop glue of `x`'s type. Drop glue (element loops, field
   order, variant switch) is generated per layout in `yong-mir`, not written as Yong items. Drop
   specialization (5.3 step 3) is a Yong rewrite: `drop x` of a statically known variant becomes
   drops of its fields.

The interpreter executes `drop x` (today it clears the register), and in `yong test --ub` it
reports a double drop or a use after drop.

- Decision: accepted as recommended (Antonio, card, 2026-11-03)
- Row: R770

### D3: `yong-opt` is Yong to Yong

1. Every `yong-opt` pass reads valid Yong and writes valid Yong: the `yong-core` validator
   accepts it and `yong-check` accepts it in a post-optimization mode (D2 rules added, everything
   else unchanged). A pass whose output fails is an internal error naming the pass.
2. Optimized Yong is printable (`yong view --as yong --opt`) and runnable by the interpreter
   (`yong run --opt`).
3. Item identity stays on checked Yong. Optimized items are cached keyed by the checked item's
   hash, the instance key (D4) and the `yong-opt` version, never hashed as identities.
4. `yong-mir` reads only optimized Yong. It keeps what depends on layout: scalar slots, drop glue,
   match decision trees, borrow lowering (D5) and registry calls.
5. Differential law: for every conformance program, the interpreter on checked Yong, the
   interpreter on optimized Yong, and native code agree on output, failure and panic kind. The
   law harness (R564) runs the middle leg in CI.

- Decision: accepted as recommended (Antonio, card, 2026-11-03)
- Row: R771

### D4: Monomorphization and comptime specialization move into `yong-opt`

1. The first `yong-opt` pass replaces each reachable generic instance with an instance item and
   rewrites calls to name it. `yong-mir`'s `Mono` keeps only the worklist that lowers instances
   in order; it rejects a type parameter with an internal error.
2. Instance item syntax, legal only in optimized Yong (like `drop`):
   `inst fn id[i64](let x: i64) -> i64 { ... }`. The bracket holds concrete type, `Nat` and
   comptime arguments, never parameters. A call `id[i64](x)` names the instance when one exists.
   Options weighed: a mangled name (`id__i64`, collides and reads badly) and an `@instance`
   attribute on a renamed fn (two names for one thing). `inst` reads like the call it serves.
3. The instance key is the generic item's hash plus the hashes of its arguments, so two modules
   that instantiate `std.list.len[i64]` share one instance.
4. Comptime parameters specialize the same way (`inst fn pow[3](...)`), which closes the
   "comptime type parameter" gap in `yong-mir`.
5. Instantiation depth is bounded at 64 nested instances; polymorphic recursion past the bound is
   E275, reported once per cycle.

- Decision: accepted as recommended (Antonio, card, 2026-11-03)
- Row: R772

### D5: Pass order of 5.3 and 22.3

Section 5.3 lists borrow lowering as the first ownership pass. Borrow lowering changes no Yong
text (it chooses pointer or value per 4.1's lowering column), so it is a `yong-mir` step. The
order becomes:

`yong-opt`: monomorphization and comptime specialization, static handler resolution, drop
elaboration, drop specialization, reuse analysis, then fusion and the rest.
`yong-mir`: borrow lowering, layouts, drop glue, decision trees.

`yong run` keeps interpreting checked Yong by default; `--opt` interprets optimized Yong.
Native builds always run the required passes (monomorphization, handler resolution, drop
elaboration) even at the lowest level.

- Decision: accepted as recommended (Antonio, card, 2026-11-03)
- Row: R773

## Spec changes

- 3 (Yong grammar and rules): `inst` item, legal only after optimization; `drop` and `reuse`
  invalid in checked Yong (D2.1, D4.2).
- 4.2: consumptions are explicit in checked Yong; elab's moves are re-derived and checked (D1).
- 5.3: pass list and owners (D2, D5); drop flags as `var` plus `if`; last-use drop for types
  without custom `Drop`.
- 5.4: the early-drop permission and its limit (D2.3, D2.4).
- 22.2: `yong-opt` row says Yong to Yong; `yong-mir` row drops monomorphization (D3, D4).
- 22.3: optimization order (D5) and the differential law (D3.5).
- 25: E275 for instantiation depth (D4.5).

## Not in this RFC

- Translation validation of individual passes (the Alive2-style check from the theory survey);
  D3 makes it possible later.
- RVSDG or e-graphs inside `yong-opt`; only if benchmarks ask.
- The neutral HIR rewrite and the `yong-syntax` decision, tracked separately.
