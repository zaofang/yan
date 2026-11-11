# 0066: Trace diff: `yan replay --diff`, and the trace container it reads

- Status: draft
- Research: [Yan tooling study: borrow and champion](https://claude.ai/artifact/SAaQdxrqitundhjM7DoBxz),
  champion C1; tracker #1004, under #449.
- Rows: filled in when accepted

## Summary

Section 7.6 and R305 specify recording, replay and the trace format, and `yan replay --against REV`
reports the first divergence of one trace on another build. Nothing compares two recorded runs. This
RFC adds `yan replay --diff a.trace b.trace`: it aligns two traces by task-tree path and op index and
reports the first divergence, classed as setup, schedule, request, reply or shape. The question it
answers is "what did my patch change at run time", in one line, with no build needed. rr and Pernosco
debug one run; no toolchain aligns two, because no other toolchain has a deterministic schedule and a
per-task capability log to align on.

The lead's build order (study D1, 2026-11-11) puts this first, on top of the unbuilt `yan run --record`
and `yan replay`. Those two commands implement R305 as written and need no decision here. At 4fbc37b
(2026-11-11) `crates/yan-cli/src/main.rs` has neither, and no crate reads or writes a trace.

Champions C4 (upgrade dossier), C6 (trace shrinking) and C8 (agent session journal) read R305 traces
too. D2 fixes the trace container they share, so the four RFCs agree on one representation (practice
12).

## Decisions

### D1: what the diff aligns on

R305 keys each stream by task-tree path and numbers entries by op index. R308 derives span ids from
the same task-tree path and a per-task counter. Both name the same identity.

Options: (a) align streams by task-tree path and entries by op index, with R308 spans shown as
context; (b) align by R308 span id; (c) a sequence alignment (Myers) over every entry, ignoring task
structure. (b) needs a tracing handler bound in both runs and gives the same answer as (a) when one
is. (c) finds a "closest" match that the determinism guarantee makes meaningless: once a task's
request differs, every later entry of that task is a consequence. Recommended (a).

- Decision:
- Row:

### D2: the trace container

R305 fixes the header fields and entry contents but not the file. C1, C4, C6 and C8 all need random
access to one task's stream, and the production ring buffer (7.6) writes a window, not a whole run.

Proposal: a trace file is a sequence of length-prefixed canonical `Wire` records: the header, then
entries in the order they were recorded, each tagged with its task-tree path and op index, then a
trailing index from task path to the offsets of its entries, then a fixed-size footer holding the
index offset. A writer that stops early (panic, kill) leaves no index; readers rebuild it by one scan.
Op indices are absolute, so a ring-buffer window aligns with a full trace of the same run. The header
gains a container version; R305's header fields are unchanged. The format stays unfrozen (20.2): a
trace replays, and diffs, on the minor that wrote it.

Options: (a) the above; (b) one file per task in a directory; (c) the whole trace as one `Wire`
value. (b) breaks single-file bundles (C6) and `tests/traces/` storage (R300). (c) cannot be written
incrementally or survive a crash. Recommended (a).

- Decision:
- Row:

### D3: divergence classes and which one is first

A divergence is one of:

- **setup**: header handler bindings or seeds differ. Reported before any stream, because every
  stream difference may follow from it.
- **schedule**: a nondeterministic-choice entry (`uses nondet`, `explore`, `pct`, cancellation, timer
  race) differs.
- **request**: the capability, operation or argument digest of an entry differs. The program asked
  for something different; this is the class a code change produces.
- **reply**: the request matches and the recorded result differs. The environment answered
  differently; this is the class a flaky dependency produces.
- **shape**: a task exists in one trace only, or one stream ends earlier.

Header toolchain minor, root item hashes and edition are printed as context, not as divergences;
differing root hashes are the expected case for "before and after my patch".

First is defined over the task tree, since wall time is unobservable (7.7): order entries by
(task-tree path, op index), with a child task's stream placed at its spawn entry in the parent. The
first divergence is the least one in that order. Divergences in a task's descendants after its first
divergence are counted as consequent and not listed unless `--all` is given.

Options: (a) these five classes and this order; (b) a single "differs" class; (c) order by recorded
position in the file. (b) loses the request/reply split, which is the line between "my code changed"
and "the world changed". (c) depends on recording order, which a different scheduler handler may
change without changing meaning. Recommended (a).

- Decision:
- Row:

### D4: redacted values

7.6 applies each capability's redaction hook before recording. Two redacted results compare equal as
recorded even when the real values differed.

Options: (a) compare what was recorded, and report the count of redacted entries that matched as "not
compared"; (b) record a keyed digest of the unredacted value beside the redaction, so redacted entries
compare; (c) refuse to diff traces with redactions. (b) leaks equality of secrets across runs to
anyone holding both traces, which C6 bundles would ship. (c) makes production traces useless.
Recommended (a).

- Decision:
- Row:

### D5: trace-only by default, source lines on request

R305 entries hold argument digests, not arguments, and the trace holds no source spans. A trace-only
diff can say "op 14 of task 0.2: `fs.read` argument digest differs" but not which line or which
value.

Options: (a) `--diff` works on the two files alone; `--explain` replays each trace on a build (the
current tree by default, `--rev-a` and `--rev-b` to name others), and since replay up to the
divergence reproduces the run exactly, prints the source line and both argument values at the first
divergence; (b) always require builds; (c) record full arguments under a flag. (b) loses the
no-build case, which is the one CI and C4 need. (c) adds a second argument representation and a
second redaction surface. Recommended (a).

- Decision:
- Row:

### D6: one engine, three front ends

R306 has one trace query engine for the DAP server, `yan debug` and the MCP `debug` tool, with a
`diverge(trace, rev)` query. `yan replay --against REV` is replay on another build followed by a
comparison.

Proposal: add `diff(trace_a, trace_b)` to the R306 query engine, returning the divergence record of
D3; `yan replay --diff` and the MCP `debug` tool call it; `--against REV` becomes "replay on REV,
recording, then `diff`", so there is one comparison, not two. `diverge(trace, rev)` is the same
composition.

Options: (a) the above; (b) a separate diff in the CLI. (b) is a second comparison beside
`--against` (practice 12). Recommended (a).

- Decision:
- Row:

### D7: output and exit status

Line form: one line per listed divergence, `task 0.2 op 14 request fs.read: digest 3fa1.. vs 9c07..`,
ending in `| repro: yan replay a.trace` when the class is request or reply. `--format json` emits one
record per divergence (class, task path, op index, both entries, consequent count, span when
`--explain` ran) and a summary record. Exit 0 when no divergence, 1 when any, 2 on a read error, as
`diff(1)` does, so scripts compose.

A divergence is not a diagnostic: nothing in the program is wrong. It gets no E or W code.

Options: (a) the above; (b) emit divergences as 18.2 diagnostic records with a new code. (b) puts a
non-error into the diagnostic stream that agents treat as repair work. Recommended (a).

- Decision:
- Row:

### D8: backend trace equality as a conformance law

Determinism (7.7) says the interpreter and native code give the same results for the same capability
replies. With D2 and the diff, that becomes checkable on capability traffic, not only on stdout: for
every conformance program, the interpreter trace and the native trace MUST diff clean.

Options: (a) add the law and run it in `tools.difftest` once native recording lands (M2a, R378);
(b) leave the differential test on stdout only. (a) catches a backend that issues a different request
and happens to print the same output. Recommended (a), with the native half deferred to M2a.

- Decision:
- Row:

## Spec changes

- 7.6: D2 container paragraph after R305; D4 sentence on redacted entries.
- 18.1: `yan replay --diff a b [--all] [--explain [--rev-a REV] [--rev-b REV]]` in the command table,
  M1 (interpreter traces).
- 18.4: D3 classes and order, D7 output and exit status, D6 `diff` query beside `diverge` in R306.
- 24 (conformance suite): D8, native half at M2a.

Implementation order: `yan run --record` and `yan replay` on the interpreter (R305 as written), then
the D2 container, then `diff` in the query engine, then the CLI. New code goes in a new module of
`yan-interp` and the CLI, not in the lowering files of practice 5.
