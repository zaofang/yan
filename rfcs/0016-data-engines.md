# 0016: Data engines

- Status: accepted (the lead chose "Accept all four" on 2026-11-01)
- Research: [Deep frontier survey: speed and capability](https://claude.ai/artifact/U5Rg6qH5x2KMHX5dAfkh1H), picks 2, 8, 9 and 16
- Rows: R625 to R628

## Summary

Four additions for programs that process data at scale: views that update incrementally, batched
lookups that hide memory latency, fusion across kernels the compiler cannot read, and parallel scans.

## Decisions

### D1: incremental views

`@yong/frame` adds `q.incremental()`, a `View[R]` updated by `apply(delta)` in time proportional to
the change, compiled to a DBSP circuit over Z-sets (Budiu et al., VLDB 2023). Float sums in a view
are exact, so deletion is exact, and the result MUST equal a rerun of the plan with exact sums. The
engine is the `yong-dbsp-engine` crate (R576 layout).

- Decision: accepted, 2026-11-01
- Row: R625

### D2: batch lookups

`get_many` and `contains_many` on dicts and sets, and `index_many` on `Keyed`, return results in
input order and probe in interleaved groups with prefetching (AMAC, VLDB 2015). The table hash join
uses the same kernel.

- Decision: accepted, 2026-11-01
- Row: R626

### D3: access patterns for fusion

`@access(...)` on `extern`, `@kernel` and engine items declares the region each operand is read or
written per output tile. The optimizer MAY fuse across the call without seeing the body (Fern,
PLDI 2025). A body that contradicts its claim is E274; an extern claim is trusted and audited.

- Decision: accepted, 2026-11-01
- Row: R627

### D4: parallel scans

`par_scan(xs, m)` gives inclusive prefix folds in the R382 order. Integer affine recurrences in `for`
loops MAY run as scans when the solver rules out overflow; float recurrences only through the
explicit call (Blelloch 1990; ScanWeaver, 2026).

- Decision: accepted, 2026-11-01
- Row: R628

## Spec changes

11.9 (D1), 5.6 and 17 (D2), 10.6, 11.2 and 25 (D3), 9.3 (D4).
