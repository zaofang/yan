# 0071: Trace shrinking and repro bundles: `yan replay --shrink`, `yan bug`

- Status: draft
- Research: Yan tooling study: borrow and champion,
  champion C6; tracker #1004, under #449.
- Rows: filled in when accepted

## Summary

Section 7.6 and R305 specify recording and replay: a failing run recorded in production replays
exactly. A recorded failure is still large (every capability operation of every task), bound to one
build, and may carry data that must not leave the machine. This RFC adds two things on top of
`yan replay`:

- `yan replay f.trace --shrink` searches for a smaller trace that fails the same way. It treats the
  recorded capability replies and schedule choices as the choice sequence of R303, which already
  shrinks generator bytes for property tests, and shrinks that.
- `yan bug f.trace` packs the shrunk trace, the toolchain and the item hashes into one shareable
  file, after redacting data-bearing replies and proving the failure still reproduces.

Nearest prior art: Hypothesis shrinks generated inputs only, and rr and Pernosco traces are whole
process recordings that are huge and leak every byte the process saw. Yan can do this because a run
is a function of its capability replies (7.7), so the trace is the complete input.

Dependencies: `yan run --record` and `yan replay` are unbuilt (checked against
`crates/yan-cli/src/main.rs` at 4fbc37b, 2026-11-11). This RFC reads the trace container fixed by
RFC 0066 D2 and does not redefine it (practice 12). It does not use the diff of RFC 0066 beyond D6
below.

## Decisions

### D1: what a candidate is, and when it counts

R305 replay is strict: a request that differs from the next entry of its task fails with
`ReplayDiverged`. A shrunk trace cannot be replayed that way, because changing one reply may change
what the program asks next.

Proposal: add a second replay mode, guided replay, used only by the shrinker. A candidate is a
modified trace. Guided replay feeds each task its candidate stream in order. It accepts a candidate
when all of these hold:

1. every request the program makes matches the next entry of its task (capability, operation,
   argument digest), as under R305;
2. a task may make fewer requests than its stream holds (the surplus is dropped), never more;
3. the run fails with the same failure signature as the original.

Anything else, including a request mismatch or a task running past its stream, rejects the candidate.
Nothing is invented: there is no default reply, so no per-capability "simplest value" hook exists.

Failure signature: the failure kind (panic, uncaught `fail` variant, `ReplayDiverged` excluded), the
hash of the item that raised it, and the task-tree path of the raising task, not source lines or
messages. Messages carry values that shrinking is meant to change.

Options: (a) the above; (b) fill missing replies with a per-capability default, which needs a new
handler hook and can hide an infinite loop behind a default; (c) strict replay only, with removal of
whole entries, which almost never replays. Recommended (a).

- Decision:
- Row:

### D2: moves, order and budget

Moves work on the canonical `Wire` form of an entry, so no capability implements anything:

1. **Truncate**: drop every entry after the failure in each task, and every task that ended before it.
2. **Schedule to default**: replace each schedule choice (`uses nondet`, `explore`, `pct`,
   cancellation, timer race) by the default scheduler's choice at that point.
3. **Simplify replies**: for each reply in recorded order, try the simplest value of the same `Wire`
   type, then smaller ones toward it: integers toward 0, `str` and `bytes` and lists toward shorter
   and then elementwise simpler, variants toward the first, options toward `None` where the type
   allows. A `fail` reply stays a `fail` reply.
4. **Redact**: replace a data-bearing reply (D5) by the placeholder of its type. This is a case of
   move 3, listed apart because D5 gives it an extra guarantee.

Moves repeat in this order until a pass changes nothing. A candidate is smaller when it has fewer
entries, then fewer total reply bytes, then is lexicographically earlier. The pass order and
candidate order are fixed and the shrinker draws no random numbers, so the same trace on the same
build gives the same output on every run and every machine.

Budget: a count of guided replays, `--budget N` (default 2000), since wall time is unobservable
(7.7). `--for D` is accepted as in R303 and converted by a calibration replay, and the output states
the count actually used. A run that exhausts the budget writes the smallest candidate found and
exits 3.

Options: (a) the above; (b) delta debugging over entries; (c) random mutation. (b) removes entries
the program then asks for again, so most candidates are rejected by D1. (c) is not reproducible and
needs a seed to be recorded. Recommended (a).

- Decision:
- Row:

### D3: output and composition

`--shrink` writes `f.shrunk.trace` beside the input (`-o` overrides) in the RFC 0066 D2 container,
with a header field naming the input trace hash, the failure signature and the counts (entries and
reply bytes, before and after). It prints those counts in one line.

`--shrink` composes with the existing `--to-test` (R305): `yan replay f.trace --shrink --to-test`
shrinks and then proposes the test through the 18.4 / R300 path, binding the shrunk trace under
`tests/traces/`. Agents never edit `tests/`, so this stays a proposal. A failure found by a fuzz or
`explore` run that already wrote a shrunk schedule to `.yan/found/` (R303, R304) is the same kind of
trace and takes the same path.

The shrunk trace replays under R305 strict mode, because the accepted candidate's requests matched
its own entries. Strict replay is therefore the check, and no second reader exists.

Cache: the shrink result is keyed by (input trace hash, root item hashes, failure signature, budget)
and stored in the verdict cache of R19, so a repeat run is a lookup.

Options: (a) the above; (b) overwrite the input. (b) destroys the only full recording. Recommended
(a).

- Decision:
- Row:

### D4: `yan bug` and its container

`yan bug f.trace -o name.bug` makes a bundle. A bundle is a trace file (RFC 0066 D2 container) with
extra leading records: a manifest, then the shrunk and redacted trace. The manifest holds the toolchain
version and minor, edition, root item hashes and the hashes of every item the failure reaches, the
failure signature, the handler bindings by name, and the counts of D3. The same bundle reader serves
`yan replay name.bug`, which replays it directly.

Naming. The study named the command `yan bug`. Research note 2026-11-10 item 10 proposed a
`yan bug-report` command that bundles version, commit and input hash on an internal compiler error.
These are one need: report a failure reproducibly. Options: (a) one command, `yan bug`, which also
accepts the compiler's own failure input (the ICE case, a source tree and no trace); (b) two
commands; (c) `yan bug-report` for both. (b) leaves two bundle formats (practice 12). Recommended
(a). Whether the file takes a new extension is the lead's call, since names are frozen (practice 15):
the proposal is none, so a bundle is a `.trace` file and no new name is registered; `.bug` above is
a placeholder.

- Decision:
- Row:

### D5: what is redacted, by default

R305 applies each capability's redaction hook before recording, and `Secret[T]` values appear hashed
in traces (17.4). That protects a trace kept on its owner's machine. A bundle leaves it, so the bar
is higher, and a hash of a low-entropy secret (a PIN, a short token) can be brute-forced; the same
holds for argument digests of such values.

Proposal: default deny by capability. A bundle keeps real replies only from capabilities that return
no data about the outside world: `clock`, `rand`, schedule choices, and `log` and `metrics` (output
only). Every other capability's replies (`fs`, `net`, `env`, `process`, `db`, model calls, and any
third-party capability) are replaced by the placeholder of their type, and `Secret` entries and
their argument digests carry no hash at all in a bundle, only the placeholder. `--keep CAP` retains
the real replies of the named capability and is recorded in the manifest, so the receiver sees what
was kept. Redaction happens as shrink move 4, so the shrinker first learns whether the failure
needs the real value at all; most do not.

This is the speculative part of the champion: it assumes a failure rarely needs the data it saw.
When it does, D6 refuses rather than ships data.

Options: (a) the above; (b) default allow, with the existing hooks only: a capability author who
forgets the hook leaks. (c) typed marking only, redact `Secret[T]` and leave other replies: file
contents and network payloads leak. Recommended (a).

- Decision:
- Row:

### D6: reproduction gate

A bundle is written only after `yan bug` replays the redacted, shrunk trace in strict mode (D3) on a
clean tree at the manifest's root hashes and reproduces the failure signature.

If the redacted trace does not reproduce, `yan bug` writes nothing and exits 2, naming the
capabilities whose real replies the failure depends on and the `--keep` that would be needed. It
never falls back to an unredacted bundle (practice 13). The user may pass `--keep` knowingly.

Options: (a) the above; (b) write the bundle with a warning. (b) ships a file that does not repro, or
ships secrets. Recommended (a).

- Decision:
- Row:

### D7: source and toolchain in the bundle

A bundle holds hashes, not code: string literals are as sensitive as replies. The receiver resolves
hashes from their own tree, a registry or a vendored copy, and `yan replay name.bug` reports the
hashes it cannot resolve. `--with-source` adds the canonical Core text (3.6) of the reached items, which
is what the hashes cover, and the manifest says so. The toolchain version is recorded, not
bundled: a trace replays on the minor that wrote it (20.2), and a different minor is a refusal with
the version named.

Options: (a) the above; (b) always include the Core text; (c) never. (b) leaks by default, (c) makes the
bundle useless to a maintainer without the tree. Recommended (a).

- Decision:
- Row:

### D8: inputs: full traces only, for now

7.6 says production mode keeps a ring buffer and writes it on panic. A window does not start at
program start, so replay from its first entry has no state to start from; R305 does not say how a
window replays. Guided replay needs the whole run.

Options: (a) `--shrink` and `yan bug` accept only complete traces and refuse a window with a
message that names the window and says a full recording is needed; (b) specify checkpointed windows
first. (b) is a separate design and blocks the champion. Recommended (a), and a numbered item in
the gap issue for window replay when this RFC is accepted (practice 9).

- Decision:
- Row:

### D9: conformance and laws

Cases proposed through `conform/*` in a separate PR (agents never edit `conformance/`):

- Shrink is deterministic: the same trace shrinks to byte-identical output twice.
- Shrink never grows: entries and reply bytes are monotone non-increasing, and the output replays
  strict and fails with the original signature.
- Backend equality: for each conformance failure, a trace recorded on the interpreter shrinks to the
  same output when guided replay runs on native (once native replay lands, M2a; the law of RFC 0066
  D8 covers the recording side).
- Redaction: a planted `Secret`, a planted file content and a planted network payload never appear
  in bundle bytes, searched raw and in hashed form, unless `--keep` names their capability.
- Gate: a failure that needs a redacted value makes `yan bug` exit 2 and write nothing.

Options: (a) the above; (b) tests of the shrinker alone. (b) misses the redaction law, which is
the champion's safety claim. Recommended (a).

- Decision:
- Row:

## Spec changes

- 7.6: guided replay (D1) after R305; D5 redaction rule for bundles; D8 sentence on windows.
- 18.1: `yan replay --shrink [--budget N] [-o FILE]`, `yan bug [--keep CAP] [--with-source]`
  in the command table, M1 (interpreter).
- 18.4: D2 moves and order, D3 output and cache, D6 gate, beside R303 and R306; `--shrink --to-test`.
- 24 (conformance suite): D9.

Diagnostics: none proposed. A rejected candidate, a non-reproducing bundle and a refused window are
tool results with exit statuses (D2, D6, D8), not program diagnostics, as in RFC 0066 D7.

Implementation order: `yan run --record` and `yan replay` on the interpreter (R305 as written), then
the RFC 0066 D2 container, then guided replay, then the moves, then `--shrink`, then redaction and
`yan bug`. New code goes in a new module of `yan-interp` and the CLI, not in the lowering files of
practice 5.

Python-fidelity policy: not touched; no surface or builtin changes. Frozen names: no frozen name
changes; `yan bug` and `--shrink` are new names, and D4 asks whether a new extension is wanted.
