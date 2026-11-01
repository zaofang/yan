# 0003: Yong gaps found by the surface study

- Status: accepted (the lead accepted every decision below on 2026-11-01: "Accept everything into rfcs")
- Research: [Surface shortlist, revision 2](https://claude.ai/artifact/3cSMv6uiyc9DgCXBDGsmhD), section
  "Ideas for Yong itself"
- Rows: R551 to R557

## Summary

Writing the niche surfaces of RFC 0002 against `spec/` found seven places where Yong itself falls
short. Three are small, well-understood library additions and become normative now: role-aware model
calls in `@yong/model`, durable timers in `@yong/durable`, and `head` and window operations in
`@yong/frame`. Four need design work and join the research track (26.6): a dry-run handler class,
streams over `Remote`, a ceiling-level `fs.only`, and semantic diff. A parser and printer derived from
one grammar, also raised by the study, is already research row R527 and gets no new row.

## Decisions

### D1: Role-aware model calls

`@yong/model` adds `generate_chat[T: Wire](messages: List[Message]) -> Untrusted[T] fail ModelError`,
with `type Message = { role: Role, content: Template }` and `Role` one of `system`, `user`,
`assistant`. Providers distinguish roles, and a single `Template` cannot carry them without an ad hoc
marker convention. `generate(p)` is `generate_chat([Message(user, p)])`. A handler whose provider has
no roles renders the messages in order under a fixed role header, documented per handler.
`fake_model`, `replay`, `model.only` and recording (R305) cover it as they cover `generate`.

Options weighed: a role-marker convention inside `Template` (no API change, but every handler parses
it); `generate_chat` (one new operation, typed). Recommended and accepted: `generate_chat`. Xun
(RFC 0002) lowers to it.

- Decision: accepted, 2026-11-01
- Row: R551

### D2: Durable timers

`@yong/durable` adds a journaling capability `timer` with `sleep(d: Duration)` and
`sleep_until(t: Instant)`. The first execution journals the absolute deadline. On resume, a deadline
already passed returns at once, and one still pending waits for the remainder. The durable store
persists pending wake-ups and resumes the run (`durable.resume`) when one falls due, so a process may
exit while a workflow sleeps for days. `timer` is the way an `@durable` function waits; E725 keeps
covering every non-journaling capability.

- Decision: accepted, 2026-11-01
- Row: R552

### D3: `head` and window operations in `@yong/frame`

`@yong/frame` adds `head(n)` on `LazyFrame[R]`, and window expressions
`expr.over(partition_by=[...], order_by=[...])` with `row_number`, `rank`, `dense_rank`, `lag(e, k)`,
`lead(e, k)`, and running `cum_sum`, `cum_count`, `cum_min` and `cum_max`. Order within a partition is
the stable sort of 11.9, and ties keep input order, so window results obey the determinism rule of
11.9. A "top N per group" is `filter(row_number().over(...) <= n)`. Plans using them compile at stage 0
like any constant-shape plan. Cha's `take` and `take N per K` (RFC 0002) lower to these.

- Decision: accepted, 2026-11-01
- Row: R553

### D4: Dry-run handler class (research)

A handler class whose operations perform no effect and return `Deferred[T]` placeholders for results
known only after a real run. A `Deferred` may flow into later operations' arguments; branching on one
is an error. Binding the dry-run handler of a capability previews any program that uses it as an
operation graph; binding the real handler then applies it, re-reading state and failing with `Drift`
where a read differs from the preview. Zhu (RFC 0002) needs it; every agent action could offer a
preview with it.

- Decision: accepted for research, 2026-11-01
- Row: R554 (amends 7.3 if adopted)

### D5: Streams over `Remote` (research)

`Remote` handles are call and reply only (9.6.1). A stream form, a call returning a typed receiver
with credit-based backpressure carried on the R404 transport, would let Qi services and remote actors
send sequences without a subscription pattern. Questions: failure and cancellation mid-stream, replay
of a partially consumed stream, and `Wire` evolution of element types.

- Decision: accepted for research, 2026-11-01
- Row: R555 (amends 9.6.1 if adopted)

### D6: Ceiling-level `fs.only` (research)

Rooted directory handles (17.3) limit paths inside a program, but package ceilings (19.2) cannot name
paths; only `net.only` exists (17.6). An `fs.only(read=[...], write=[...])` attenuator, usable in
ceilings, would let Ling scripts and agent tools carry their reachable paths in the manifest.
Questions: path canonicalization, symlinks and mount points, and how it composes with rooted handles.

- Decision: accepted for research, 2026-11-01
- Row: R556 (amends 17.6 and 19.2 if adopted)

### D7: Semantic diff (research)

`yong diff --semantic A B` reports, for the decidable fragment (`uses none`, bounded loops, no
floats), whether two item versions are equivalent, and a distinguishing input when they are not;
outside the fragment it falls back to `ex` and property tests. It would let an agent refactor carry a
proof that behaviour did not change.

- Decision: accepted for research, 2026-11-01
- Row: R557

### Not a new row: one grammar per surface

Deriving each surface's parser and printer from one grammar, so the round-trip laws hold by
construction, is R527 (26.6). RFC 0002's surfaces are expected to use it if it lands first.

## Spec changes

- 17.4: `generate_chat` (R551).
- 9.6.1: durable timers (R552).
- 11.9: `head` and windows in `@yong/frame` (R553).
- 26.6: research rows R554 to R557.
