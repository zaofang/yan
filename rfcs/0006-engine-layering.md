# 0006: Where niche surface engines sit

- Status: accepted (the lead approved writing the amended architecture into the spec on 2026-11-01:
  "accept the grand strategy into rfc/spec/row/wherever appropriate")
- Research: [Yong architecture plan: one core, many surfaces](https://claude.ai/artifact/NpTvDsHWcBnPtCmWvLtqFC),
  section "Amendments after the Xin and Wen assessment"
- Rows: R576

## Summary

R558 (section 1) says every surface, niche surfaces included, lowers to the HIR, and R560 (22.2)
says no surface crate depends on anything below it. R568 then let a niche surface run some items on
its own engine, "in `yong-<name>` or a std package". Read together, a surface crate would hold runtime
code below the HIR. This RFC places the engine.

## Decisions

### D1: Engine items and engine crates

An item that runs on a niche surface engine enters the HIR as its boundary signature only, as an
extern item. The engine is a separate crate, `yong-<name>-engine`, or a std package, at the runtime
layer beside `yong-rt`. The surface crate `yong-<name>` keeps only lexer, parser, AST and lowering,
so N6 and the 22.2 layering hold unchanged for every surface.

Options weighed: let the surface crate hold the engine (one crate per surface, but it breaks the
one-way layering and makes every tool that loads a surface link its runtime); separate engine crate
(one more crate per engine surface). Recommended and accepted: the separate crate.

- Decision: accepted, 2026-11-01
- Row: R576

## Spec changes

- 1: engine items enter the HIR as extern signatures.
- 16.7: N6 and the "Two routes" rule name `yong-<name>-engine`.
- 22.2: the layering paragraph places engine crates beside `yong-rt`.
