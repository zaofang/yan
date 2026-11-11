# 0073: Agent session journal: record, replay and diff `yan mcp` sessions

- Status: draft
- Research: [Yan tooling study: borrow and champion](https://claude.ai/artifact/SAaQdxrqitundhjM7DoBxz),
  champion C8; tracker #1004, under #449.
- Rows: filled in when accepted

## Summary

`yan mcp` (21.4) and `yan agent` (R616, R617) are how models touch Yan. Today a session leaves only the
R619 run record of `yan agent`, which holds token counts, round reports and tool-call summaries; a
session driven by an outside client through `yan mcp` leaves nothing. Neither can be re-executed. This
RFC adds a `--journal FILE` flag to both commands. The journal is an R305 trace in which the MCP server
is the program: requests are what the transport capability returned, replies are what the server sent.
`yan replay` then re-executes a session, and C1 (RFC 0066) diffs two sessions, so "where did model A and
model B first act differently on this task" is one command. Prior art (LangSmith-style logs) stores text
that cannot be replayed; here replay is exact because the server is a deterministic function of its
requests and the source hashes (7.7).

Nothing here changes R305, the MCP tool schemas or the run record's existing fields. The journal reads
the trace container fixed by RFC 0066 D2 and does not redefine it. At 4fbc37b (2026-11-11) no trace
writer or reader exists in any crate, so this RFC builds on `yan run --record` and `yan replay` landing
first (study D1).

## Decisions

### D1: what a journal is

The server, seen as a program, has one task and one transport capability: `transport.recv()` returns the
next JSON-RPC request, `transport.send(reply)` takes the reply. A journal records them like any
capability: each `recv` entry holds the full request as canonical `Wire` bytes, each `send` entry holds
the reply's digest and the reply bytes. R305 stores argument digests only; requests are results of
`recv`, so they are stored in full without a change to R305, and the server's own replies are checked on
replay by digest.

Options: (a) the above; (b) a new journal format beside R305; (c) record only the replies. (b) is a
second trace representation (practice 12). (c) cannot replay, because replay needs the requests.
Recommended (a).

- Decision:
- Row:

### D2: when a model is in the loop

Under `yan agent`, the reference loop also calls `Model.generate` (17, R450), which R305 already
records. The journal of an agent run is therefore one trace: `Model` entries and `transport` entries in
the order they occurred, so a session replays with `replay(trace)` standing in for the model (the test
handler of 17) and no network. For a bare `yan mcp` the model is outside the process and only `transport`
entries exist; such a journal replays the server alone.

Options: (a) one trace per run, both capabilities; (b) two traces (model, tools) joined by a key.
(b) needs a join no reader wants. Recommended (a).

- Decision:
- Row:

### D3: what replay reproduces, and the workspace

Tool handlers read and write the workspace (`apply_patch`, `undo`, `run_tests`). Replay MUST NOT touch
the real working tree. The header already holds root item hashes (R305). Replay builds a scratch overlay
at those hashes, re-executes every request in it, and fails with `ReplayDiverged` at the first reply
whose digest differs.

Options: (a) require the current tree to match the header's root hashes, else refuse with the list of
differing items; (b) also store a source snapshot in the journal, so replay works anywhere;
(c) refuse unless every root hash matches and offer `--rev REV` to check the tree out first. (b) puts all
of a project's source in a file that D6 wants to keep safe to share. (a) alone strands journals whose
tree moved on. Recommended (c): (a) plus `--rev`, using the VCS (the op log of 18.3 names the revision
in the header).

- Decision:
- Row:

### D4: the command surface

Add `--journal FILE` to `yan mcp` and `yan agent`; off by default. `yan agent --out` run records gain an
optional `journal` field (path and BLAKE3 hash), which R619 permits ("new fields are optional
additions only"). Replay and diff are `yan replay FILE` and `yan replay --diff A B` as already proposed
(RFC 0066). No new command, so no frozen name changes (practice 15).

For the stateless MCP protocol (21.4) there is no client session id; one journal covers one server
process, that is, one stdio connection.

Options: (a) the above; (b) always journal to `.yan/` and add `yan journal` for management; (c) an
environment variable. (b) adds a command and writes files nobody asked for. (c) is invisible state.
Recommended (a).

- Decision:
- Row:

### D5: diffing two models' sessions

Two sessions of one task from two models share a deterministic server, so with the same start tree the
first divergence between them is always class `request` (RFC 0066 D3): the first call where the models
differ, never a `reply` before that. C1 therefore answers "where did they first differ" unchanged. The
question that C1's first-divergence line does not answer is aggregate: calls per tool, repairs per phase,
rounds to green.

Options: (a) C1 as is, and the aggregate left to a consumer that reads the journal; (b) a `--summary`
flag on `yan replay` printing per-tool call counts, per-phase rounds and codes hit; (c) put aggregates in
the run record only. (c) cannot cover bare `yan mcp`. (b) is small and reads only the journal.
Recommended (b), computed from the journal alone, never stored in it.

- Decision:
- Row:

### D6: tokens and the run record

R619 holds tokens per round as the provider reported them (R567); `Model.generate` entries in the
journal hold the same usage in their results. Two places state one fact (practice 12).

Options: (a) the journal is primary; R619 `tokens` stay as written for the existing harness, and `yan
replay --check-record RUN.json` verifies that they equal the journal's; (b) derive R619 from the journal
and drop its token fields; (c) leave both unchecked. (b) breaks R619's `schema: 1` and the R567
harness. (c) lets the two drift silently, which the gate (23, R567) cannot afford. Recommended (a).

- Decision:
- Row:

### D7: what a journal may contain

A journal holds source code, patches, prompts and model replies, so it is sensitive. Requests and
replies from 21.4 carry no credentials: the provider key lives in the `Model` handler, not in its
operations' arguments, and `Untrusted[T]` replies are recorded as `Wire` bytes without change. 7.6 applies
each capability's redaction hook before recording, so `transport` and `Model` each get a hook.

Proposal: the default hooks record in full (replay needs it), the journal file is created mode 0600, a
journal is never uploaded or attached by any command, and sharing is C6's job: `yan bug` redacts under
default-deny (RFC 0071 D5). `transport` gets no redaction hook that alters requests, since a
redacted request cannot replay.

Options: (a) the above; (b) redact by default, so journals do not replay; (c) refuse to journal when a
`Model` handler declares secret-bearing results. (b) removes the feature. (c) has no declared
signal today. Recommended (a), with the limit that a journal is a local artifact.

- Decision:
- Row:

### D8: size and the ring buffer

A long session can write megabytes. 7.6's production ring buffer writes a window; a window does not
replay (its first request lacks the tree state of D3 and the history that earlier `apply_patch` calls
made).

Options: (a) full journals only, with `--journal-max BYTES` ending the session's journal with a marker
entry rather than dropping entries; (b) allow a ring window and mark it unreplayable but diffable;
(c) no cap. Recommended (a). This leaves a known hole for windowed journals: to be filed as a numbered
item in a gap issue on acceptance (practice 9), not built here.

- Decision:
- Row:

### D9: conformance laws

With `yan agent --model stub:FILE` (R616) a session is a pure function of the task, the stub file and
the tree. Laws, run in CI over the existing agent fixtures: (1) a journaled stub run replays with no
divergence; (2) two stub runs of one fixture produce byte-identical journals; (3) journaling does not
change any tool reply, so a run with and without `--journal` produces the same run record apart from
the `journal` field. Law (3) keeps the journal an observer.

Options: (a) the three laws, proposed as `conform/*` cases (agents never edit `conformance/`);
(b) law (1) only. (3) is the one that catches a recorder that perturbs the session. Recommended (a).

- Decision:
- Row:

## Spec changes

- 21.4: D1, D2 paragraph on journals after the tool table; D7 paragraph on contents and mode.
- 18.1: `--journal FILE` on `yan mcp` and `yan agent`; `yan replay --summary` and `--check-record`
  (D5, D6), M1.
- 21.6 or 18.4 (wherever R619 is stated): optional `journal` field in the run record (D4).
- 7.6: one sentence that the MCP server's `transport` is a capability recorded like any other.
- 24 (conformance suite): D9 laws.

Implementation order: after `yan run --record`, `yan replay` and the RFC 0066 D2 container, add the
`transport` capability to the `yan-mcp` server loop with a recording handler, then `--journal` on both
commands, then replay over a scratch overlay (D3), then `--summary` and `--check-record`. New code goes
in a new module of `yan-mcp` and the CLI, not in the lowering files of practice 5.
