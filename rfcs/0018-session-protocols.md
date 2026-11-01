# 0018: Session protocols

- Status: accepted (the lead chose "Accept as normative" on 2026-11-01)
- Research: [Deep frontier survey: speed and capability](https://claude.ai/artifact/U5Rg6qH5x2KMHX5dAfkh1H), pick 18
- Rows: R627, R628

## Summary

Yong finds deadlocks in actors and channels at test time (9.6). Multiparty session types rule out
deadlocks and unexpected messages within a session before the program runs, `Remote` included.

## Decisions

### D1: global protocols

`session.protocol(spec)` is a stage-0 type function over a Scribble-style string: messages, sequence,
a choice by one role, named recursion and `end`. The checker projects it onto each role (Honda,
Yoshida and Carbone, POPL 2008). A protocol that cannot be projected is E728. A stage-0 string, as
`math.ein` uses, keeps the three surfaces' grammars unchanged; Chang may add sugar later.

- Decision: accepted as normative, 2026-11-01
- Row: R627

### D2: linear endpoints

`session.connect[P]()` returns one `@linear` endpoint per role. `send` and `recv` consume the endpoint
and return the next state's; a choice is received as a sum. An unallowed send is E727; dropping an
endpoint early is E412. A failing branch cancels the session with `SessionClosed`. Session channels
are exempt from E713 and deterministic. Asynchronous subtyping (Rumpsteak, PPoPP 2022) MAY reorder
sends.

- Decision: accepted as normative, 2026-11-01
- Row: R628

## Spec changes

9.5 (D1, D2), 9.6 (D2), 25 (E727, E728).
