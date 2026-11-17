# One ownership analysis in `yan-check`

Design note, 2026-11-17. Docs only: no spec rows, no conformance edits, no diagnostic codes. Items are labelled D1 to D6; none carries a row number. Tracks #1583 scope item 6, #1573 item 2, practice 12. Main at 6d1d1cc.

## Problem

"What does this statement consume?" has three answers today.

1. `yan-check/src/lib.rs` (the pre-optimization checker, `Cx`): `use_name` calls `flow.moved` for `How::Store`, a `Jump` argument and an elaborator-inserted `Rhs::Move`; a `sink` argument or a pattern binder moved out of a scrutinee is not a move there. It has no view map, so it cannot see that a name stands for `h.a`.
2. `yan-check/src/own.rs` (`consumed`, `consumption`, `proj_view`, `pat_views`): used by drop elaboration and the ownership facts in `yan-opt`, by the post-optimization check in `post.rs`, and by `yan-elab/src/copy_out.rs`. It knows views and partial moves.
3. `yan-elab/src/lower/consume.rs` (`implicit_copy`, `consume`, `moved_into`): decides copy versus move and E413 from last use, by hand at each value-producing position.

#1500 finding 1 was the symptom: (1) and (2) disagreed on a store. #1605 and #1606 closed the E413 holes in (3) position by position. The structural problem remains, and it is why a partial move out of a custom `Drop` or `@linear` root is only found by drop elaboration, after `yan check` has accepted the program: `yan run` stops with a bare `not supported yet: a field moved out of a type with a custom Drop` (`yan-opt/src/drops.rs:640`), and `yan check` says nothing.

## Proposal

D1. `own::consumption` is the one definition of what a statement consumes. `Cx::stmt` calls it for every statement and replaces its hand-written `moved`/`release` calls. It keeps a `Views` map (`proj_view` on `let x = p`, `pat_views` on `match` arms), as `post.rs` already does. Recommended: yes, this is the practice 12 fix. Cost: a walk change in `Cx` and a re-run of the whole corpus to prove no new E401.

D2. A partial consumption `(root, path)` becomes a per-field event on the existing `yan_flow` `Field` holes (today used only for `@linear` fields, `flow.rs`). A later read of `root` as a whole while a hole is maybe-open is E401 ("`root` is partly moved"); a read of another field is fine; an overwrite of the field closes the hole. Recommended: reuse the holes, add no second structure.

D3. The set of types whose drop is observable (`Observable`: custom `Drop`, `@linear`, extern handles, and holders) is a pure function of the module. It moves from `yan-opt/src/drops.rs` to `yan-check/src/own.rs`, so `yan-check` and `yan-opt` read one computation. `yan-opt` keeps its `unsupported` guard as an internal-error backstop (it should be unreachable).

D4. Rejection. A partial consumption whose root type `has_drop` (the root's own `Drop`/`@linear`/extern handle, not a holder) is rejected in `Cx` before drop elaboration, so `yan check` and every backend agree. Spec 4.2 already says this (SQ13); the row exists in prose, not as a case. Diagnostic: E413 with the `.copy()` fix, as #1573 proposes, when the field type is copyable by `.copy()`; otherwise the same error without a fix. Recommended: reuse E413, not a new code, so no catalogue change. Open: see D5.

D5 (accepted by Antonio 2026-11-17, as recommended). `flow.rs` already lets a `@linear` record give up its linear fields one by one so each can be consumed ("a record whose linear fields are all holes is consumed with them", E412, 5.4). Spec 4.2 says partial moves out of a `@linear` type are not allowed. These contradict. Recommended: allow moving a linear field out of a `@linear` record (it is how a `@linear` value is consumed, 5.4) and reject only moves of non-linear fields out of a `@linear` or custom-`Drop` root. That needs one sentence in 4.2 and one row. The alternative, rejecting every partial move out of a `@linear` root, breaks the existing E412 hole logic and its cases.

D6. `copy_out` (`yan-elab/src/copy_out.rs`) keeps deciding where a borrowed parameter is copied, but reads `own::consumption` and the shared `Views`, not its own `borrowed` map of `Def::Param`/`Proj`/`Binder`. Recommended after D1 lands, as its own PR, because it changes where `copy` statements are inserted and so the elaborated IR.

## Slices

1. This note (docs only), review by Antonio on D4 and D5.
2. D1 and D3: `Cx` reads `own::consumption`; `Observable` moves. No behavior change intended; gate: `yan check` over the corpus has no new diagnostics and the Rust tests pass. Stage claimed: right.
3. D2 and D4: per-field holes, partly-moved E401, the Drop/`@linear` rejection, with its spec row and conform cases (a paired PR, lead merge). Includes the or-pattern and slice binder cases of #1573 item 1.
4. D6.

## Risks

- `Cx` runs before elaboration's copy insertion in some paths; the `Views` map must be computed on the same Yan Core the checker sees. Mitigation: slice 2 changes nothing observable and is proved on the corpus first.
- The per-field hole state doubles flow facts for records. Mitigation: only roots that are partly moved get fields.
