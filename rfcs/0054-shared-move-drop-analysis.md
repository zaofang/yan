# 0054: One dataflow move and drop analysis

- Status: draft
- Research: the code survey in this RFC (main 85ca567, 2026-11-09); laws and plan in the study
  "Yan development laws and phased plans" (https://claude.ai/artifact/RaRsJ6bHuLRe1MQyNet4qq), item 7 of the
  short-term plan; RFC 0028 for the `yan-opt` drop contract
- Rows: filled in when accepted

## Summary

Whether a value has moved, is still live, or must be dropped is decided three times, by three
different mechanisms, and the bugs of #690 items 1, 2 and 4, the Option/else leaks and RFC 0028 D1 were
each fixed in one of them. This RFC proposes one analysis, a small crate over a control-flow graph with
three facts (maybe-moved, live, maybe-owned), consumed by the checker, the elaborator and drop
elaboration. It removes `next_use` and the name-keyed linearity tracking. It is design only: no code
changes with this RFC.

What exists today:

- `yan-elab` (`lower.rs`): `next_use`, `reachable`, `redefined`, `used_after` and `names_used` decide the
  last use of a name from source spans plus a stack of `Branch { group, arm, ends }` records. There is no
  graph, so a loop is handled by a separate `loop_outer` set, and `?`, `fail` and `match` fall-through are
  covered only through `ends`. Callers: E413 in `store` and in argument passing, and `Move` versus `Copy`
  in `consume`.
- `yan-check` (`lib.rs`): `moved: HashMap<Name, Span>` cloned and merged at branches, `taken` (fields moved
  out of a record, keyed by name), and `check_consumed` for `@linear` (E412), keyed by name and by a
  per-name list of uses (`How`). Shadowing and re-binding are handled by hand at scope exit.
- `yan-opt` (`drops.rs`): a third walk over checked Yan Core with its own `Live` map, scope stack and drop
  flags. It shares only `yan_check::own` (copy types and consumption helpers).
- `yan-mir` has no ownership analysis; it trusts the checked text.

Two of the three are keyed by name and by source position. A shadowed name, a name re-bound in a loop,
and a value moved in one `match` arm and used in another all depend on a bespoke patch in each place.

## Decisions

### D1: shape of the analysis

Options: (a) keep three walks and add a shared test corpus; (b) one crate, `yan-flow`, with a generic
control-flow graph and the three facts, each client lowering its own IR to that graph; (c) one analysis
on Yan Core only, with the checker and elaborator running it on the elaborated text.

(b) is proposed. The graph has basic blocks of opaque statements, edges for branches, loop back edges,
`break` and `continue`, `return`, `fail` and `?` exits, and `match` arms. A statement reports its reads,
its moves, its (re)definitions and its field takes by binding identity, not by name. The facts are
solved by the standard worklist to a fixed point. Clients provide only the lowering to the graph and the
meaning of a read, a move and a definition in their IR. (c) cannot serve E413, which must be decided
while lowering, because the move or copy chosen changes the text. (a) leaves the three mechanisms free
to diverge, which is the cause of this RFC.

Recommendation: (b).

- Decision: as recommended; Antonio, 2026-11-09
- Row: none (implementation design)

### D2: binding identity

Options: (a) names, as today; (b) a binding id assigned when the lowering to the graph sees a definition,
so a shadowing `let` or a loop re-bind is a new id and a name is only a lookup into the current scope.

Recommendation: (b). It removes the by-hand shadowing and scope-exit cases in the checker and the
`redefined` function in the elaborator. Spans stay in the diagnostics only.

- Decision: as recommended; Antonio, 2026-11-09
- Row: none (implementation design)

### D3: the three facts

1. Maybe-moved (forward, may): a read of a binding in this set is a use after move. Replaces `moved`.
2. Live (backward, may): a binding is live after a point if some path reaches a read before a
   redefinition. A last-use move is a read at which the binding is not live. Replaces `next_use` and
   `used_after`; the witness read for the "used again" label is the first read found on the search that
   proves liveness.
3. Maybe-owned (forward, may, with a must companion): where a drop is needed, and where a drop flag is
   needed because the path is only maybe-owned and the type has a custom `Drop` or is `@linear`.
   Replaces `taken`, `check_consumed` and the `Live` map in `drops.rs`. A field taken out of a record is
   a partial move: the record is owned-with-holes, tracked per field path.

Recommendation: all three, in the order moved, live, owned, one client at a time.

- Decision: as recommended; Antonio, 2026-11-09
- Row: (R number once accepted)

### D4: loops change one observable result

Today a name bound outside a loop and read inside it is treated as used again by `loop_outer`, and a name
read only before a back edge is missed by `next_use`. The graph treats the back edge as an edge, so both
cases are decided by the same rule: a read in a loop body is followed by every read reachable around the
loop. Programs the old rule accepted by accident can then gain an E413 (copy needed) and programs it
rejected by over-approximation can lose one.

Options: (a) accept the change, and fix any conformance program that changes; (b) keep `loop_outer` as an
extra rule on top of the graph.

Recommendation: (a). (b) is a second representation of the same fact (CLAUDE.md practice 12).
Conformance cases that depend on the old result are for the lead to change; the migration PR lists them
and does not edit them.

- Decision: as recommended; Antonio, 2026-11-09
- Row: R956

### D5: order of migration and what is deleted

Stages, each its own PR and each claiming "working" first:

1. `yan-flow` crate: graph, solver, unit tests; no client.
2. Elaborator: build the graph in a new module of `yan-elab` (not in `lower.rs`, DL13), answer
   `used_after` and `next_use` from it; delete `next_use`, `reachable`, `redefined`, `names_used`,
   `Branch`, `use_paths`, `defs`, `uses` and the `loop_outer` special case.
3. Checker: moved and linearity on the graph; delete `moved`, `taken`, `check_consumed`'s name keys.
4. `yan-opt` drops: the owned fact replaces the `Live` map; drop flags come from the maybe-owned versus
   must-owned difference.
5. Conformance cases for the Option/else leaks and the loop cases, proposed on a `conform/*` PR (DL4).

Recommendation: this order; the elaborator first because `next_use` is the one most often patched and
has no graph at all, the checker second because it owns the diagnostics, `yan-opt` last because it is
the consumer that must not regress performance.

- Decision: as recommended; Antonio, 2026-11-09
- Row: none (implementation design)

## Spec changes

Section 4.2 (E413: last use is decided on the control-flow graph, loops included), section 5.3
(drop elaboration reads the shared owned fact), section 5.4 and 10.6 (`@linear` consumption is the
maybe-owned fact at every exit), and the appendix rows for each accepted decision. No new diagnostic
code is proposed.
