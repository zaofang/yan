# 0017: std algorithms

- Status: accepted (the lead chose "Accept all four" on 2026-11-01)
- Research: Deep frontier survey: speed and capability, picks 10, 12, 13 and 14
- Rows: R629 to R632

## Summary

Proven algorithms that fill specific std gaps: boolean regular expressions in linear time, exact sums
where only the fixed tree was available, cache-friendly static search, and compact set structures.

## Decisions

### D1: boolean regex patterns

`compile(pattern, boolean=True)` adds `&`, `~` and lookarounds without nesting, matched
leftmost-longest in linear time (RE#, POPL 2025). Without the flag nothing changes.

- Decision: accepted, 2026-11-01
- Row: R629

### D2: exact sums everywhere

`math.fsum` names `sum_exact` for Python readers, and `sum(exact = true)` reaches `ColExpr` and query
aggregates. The kernel is Neal's superaccumulator (2015). The survey first missed that `sum_exact`
already existed; this item is what remained.

- Decision: accepted, 2026-11-01
- Row: R630

### D3: static sorted collections

`frozen_sorted_set` and `frozen_sorted_dict`, built once, in an Eytzinger or S-tree layout
(Khuong and Morin, 2017), with `rank`.

- Decision: accepted, 2026-11-01
- Row: R631

### D4: filters and compressed sets

`fuse_filter` (binary fuse filters, Graf and Lemire, 2022), seed-independent, and `roaring_set` for
`u32` and `u64`.

- Decision: accepted, 2026-11-01
- Row: R632

## Spec changes

17 (D1, D3, D4); 11.1 and 17 (D2).
