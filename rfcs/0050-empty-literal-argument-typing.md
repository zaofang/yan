# 0050: Empty literals in arguments of an optional parameter

- Status: accepted
- Research: [R567 failure-mix study (T18)](https://claude.ai/artifact/1tUqnYo9Mds7wDV2JBruKs), item D1
- Rows: R933

## Summary

An empty `[]` or `{}` passed as an argument takes its type from the called parameter (14.1). The elaborator did so
only when the parameter was a `list` or a `dict`; under `T | None` it fell through to E241. The same literal in an
`ex` line failed the same way, because `ex` lines are checked as calls. Models write `ex f([])` for an optional
collection parameter and spend a repair round on the annotation request. The literal now takes its type from the
parameter through `| None`, and the argument is wrapped in `Some`. Bindings (`xs = []`) are unchanged.

## Decisions

### D1: R933, empty literal arguments typed through `| None`

An empty `[]` or `{}` in an argument, in a body or in an `ex` line, takes the type `U` of a parameter declared
`U | None`, and is passed as `Some` of it. Other empty literals keep their rules. Recommended: accept.

- Decision: accepted; Antonio, standing approval of 2026-11-07
- Row: R933

## Spec changes

14.1 (functions).
