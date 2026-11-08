# 0049: Positional arguments in the lenient reader

- Status: accepted
- Research: [R567 failure-mix study (T18)](https://claude.ai/artifact/1tUqnYo9Mds7wDV2JBruKs), item D2
- Rows: R932

## Summary

Yan Agent requires keyword arguments for parameters that share a type in a callee with three or more parameters
(E210). Models trained on Python pass them by position, and a repair round for a fault that never changes meaning
costs a generation. Haiku mbpp 402 stayed unrepaired and mbpp 706 never turned green in the spec phase on this rule
alone. The lenient reader accepts such a call, reports a note, and reads it as the keyword form. `--strict` is
unchanged.

## Decisions

### D1: N49, positional arguments read as keywords

When E210 is the only fault of a call, the lenient reader accepts it with note N49 and elaborates the arguments as
keywords. The first argument that needed a keyword and every later one are named, because a positional argument cannot
follow a keyword one; arguments before it, and the exempt literals and equal-name arguments before it, stay
positional. Record constructors follow the same rule. `yan check --strict`, and Yan, keep E210. The printer emits the
keywords, so the canonical text needs no extra round. Meaning is unchanged: the arguments are matched to parameters by
the position they were written at. Recommended: accept.

- Decision: accepted; Antonio, standing approval of 2026-11-07
- Row: R932

## Spec changes

14.1 (calls), 14.4 (N49), 25 (E210, N range).
