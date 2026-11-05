# 0039: Drops on the failing edge of `?`

- Status: accepted
- Research: none; the leak counter on `conformance/yag/run/concurrency/lowest_index_failure.yag` (one native block live), 2026-11-05
- Rows: R871

## Summary

In optimized Yan Core, `let x = f(a)?` returns early on failure, and the early return leaves every owned
local undropped. Drop elaboration covers `ret`, `fail`, `break` and `continue` but the `?` form has the
exit inside it. This RFC decides how that edge carries its drops.

## Decisions

### D1: how the `?` failing edge expresses its drops

Options: (a) a drop list on the `?` form, `f(x)? drop a, b`, valid only in optimized Core; (b) `yan-opt`
rewrites `?` into a `match` with drops in the failing arm, which loses the `?` shape after optimization;
(c) `yan-mir` frees live locals in the failing branch, which splits drop logic across two crates and bypasses
the post-optimization check. Recommend (a): drop placement stays in `yan-opt`, the checker verifies the
edge, and checked Core is unchanged.

- Decision: (a), Antonio, 2026-11-05
- Row: R871

## Spec changes

3.5 (the drop list beside `drop`), 5.3 (drop elaboration covers the failing edge).
