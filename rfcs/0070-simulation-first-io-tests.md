# 0070: Simulation-first I/O tests: `yan test --sim`

- Status: draft
- Research: Yan tooling study: borrow and champion,
  champion C5; tracker #1004, under #449.
- Rows: filled in when accepted

## Summary

A test that binds a real capability with `with` is an I/O test: it is never cached, runs last under a
wall-clock timeout and may fail without a code change (18.4, R309). The spec already gives the pieces
that remove the flakiness: `std.sim` hosts with their own `net`, `fs`, `time` and `rand`, one seeded
scheduler and virtual clock, `fault.maybe(name)` points, PCT scheduling and `sim.check_trace` (17.7),
`mem_net` and the one test handler set (7.3). What is missing is the toolchain contract around them: a
test that runs under `std.sim` is a pure function of its seed and fault plan, so it can be cached,
swept over many seeds, reported with a repro command and minimized, and none of that is specified.

This RFC adds `yan test --sim[=N]` and the verdict, cache, seed and fault-plan rules that make a
simulated I/O test a first-class test. FoundationDB, TigerBeetle and Antithesis show the method; each is
built per project or sold as a hypervisor. In Yan it needs no hypervisor because every effect already
goes through a handler.

At 4fbc37b (2026-11-11) `std.sim` hosts, `mem_net`, `fault` and `pct` exist on the interpreter. `pct`
and `check_trace` are unsupported in Cranelift native and fail loud, there is no actor-event trace
recorder, and `check_trace` does not match messages across actors (gap items 1 to 3 of #992). The
parts of this RFC that read a recorded trace wait for that recorder, and each says so.

Whether simulation becomes the default handler for I/O tests, so a real network needs an explicit
opt-in, is open (D4). The lead deferred it until #992 closes. D1 to D3 and D5 to D8 are written so they
hold whichever way D4 goes.

## Decisions

### D1: a simulated test is cacheable

Proposal: a test whose every I/O capability is bound to a `std.sim` host or a test handler (`mem_net`,
`fake_fs`, `virtual` time, `seeded` rand) is not an I/O test in the R309 sense; it is classified by what
it binds, found from its body as R309 already finds I/O tests. Its verdict key is the R19 key (test
hash, identity hashes of every item it reaches, toolchain hash) extended with the handler bindings, the
seed and the fault plan (D2). A test that binds any real handler stays an I/O test: never cached, run
last, wall-clock timeout, as today. Mixing a real and a simulated capability in one test makes it an I/O
test.

The text of 18.4 already says the key will extend "until handler bindings and seeds vary"; this
decision supplies the extension.

Options: (a) the above; (b) cache only under `--sim`; (c) leave classification as is and add a flag that
forces sim. (b) gives the same test two cache identities by flag. (c) keeps tests that are already
deterministic in the never-cached bucket. Recommended (a).

- Decision:
- Row:

### D2: fault plan as data

Proposal: a run's fault plan is the list of `fault.maybe(name)` points that returned true, each as
`(name, occurrence number, simulated time)`, derived from the seed by the `sim` handler (17.7) or
supplied explicitly with `--faults plan.yir`, a Core literal of that list. The plan is part of the
verdict key (D1) and is printed on every failure. `yan test --sim --list-faults` runs each selected
test once and lists the fault point names it reached and how often, so a point that is never reached is
visible. Point names are the strings the program passes to `maybe`; the toolchain adds none.

Options: (a) the above; (b) seeds only, no explicit plan; (c) a plan DSL with probabilities per name.
(b) cannot replay a minimized failure (D3) because a shrunk run is no seed's output. (c) adds a second
configuration language beside `sim`'s own parameters (practice 12: one representation). Recommended (a).

- Decision:
- Row:

### D3: seed sweep, repro and shrinking

Proposal: `--sim=N` runs a selected test under N seeds, default 1. Seeds are derived from the test hash
and an index, so the sweep is the same on every machine and every run; `--seed S` runs one. A failure
prints one line in the R309 format: test id, seed, fault plan hash, and `| repro: yan test --sim --seed S
--faults plan.yir <id>`. `--shrink` then minimizes the failing run's fault plan by removing fired
faults while the same failure (same panic site or same violated check) holds, and writes the result to
`.yan/found/` as a proposed test, the place R300 and R303 already use for minimized fuzz inputs. Shrinking
the capability event sequence itself is champion C6 and stays there; this RFC shrinks only the plan.

Options: (a) the above; (b) random seeds per run, as most simulation harnesses do. (b) gives a different
result on each CI run, which is the flakiness this feature exists to remove. Recommended (a).

- Decision:
- Row:

### D4: sim as the default handler for I/O tests

Open. The lead deferred this on 2026-11-11 until #992 closes and will be asked again then. This RFC
proposes nothing for it. For the record, the question is whether a test that needs `net`, `fs` or `time`
and binds nothing runs under `std.sim` instead of failing as it does now (the test handler set has no
`net`, R792), and whether a real network then needs an explicit opt-in. D1 to D3 and D5 to D8 do not
depend on the answer.

- Decision: deferred (lead, 2026-11-11), to be re-asked after #992 closes
- Row:

### D5: sweep size in the gate

Proposal: the seed count a test is approved at is part of its approved test hash (R300), recorded in
`tests.lock` beside the test. `yan test --gate` runs every approved simulated test at its recorded
count. Lowering the count, or removing a fault point from a test, changes the test hash and needs the
protected-target approval of 19.4 (P006) like any edit to an approved test. Raising it is allowed
without approval. CI may run more seeds than the recorded count with `--sim=N` and never fewer.

Options: (a) the above; (b) count set by CI configuration; (c) a fixed count for every test. (b) lets
the agent that writes the code also lower the sweep. (c) is too few for a distributed test and too many
for a pure one. Recommended (a).

- Decision:
- Row:

### D6: determinism check

Proposal: `yan test --sim --verify` runs each selected test twice per seed and compares the run trace
hash (17.7 already has CI sample seeds this way). A mismatch is reported as a toolchain
nondeterminism, with the first differing event, the verdict is not cached and the exit status is
distinct from a test failure. It is never reported as a flaky test: the spec has no retries and no
`@flaky` (R309), and a sim test that differs between two runs of one seed has found a Yan bug.
Reading the trace needs the recorder in #992 item 2; until it exists the check compares the test result
and the captured output.

Options: (a) the above; (b) trust determinism and skip the check. (b) leaves the one property the whole
feature rests on untested. Recommended (a).

- Decision:
- Row:

### D7: backends and ceilings

Proposal: the interpreter is the reference. Native `--sim` is not claimed until `pct` and `check_trace`
have matching native results (#992 item 1; practice 1 of CLAUDE.md), and a `--sim` run on native fails
with a diagnostic naming that gap, never falls back to the interpreter silently (practice 13). Each
simulated test runs under the deterministic step budget of R309, not a wall clock, since virtual time
makes a wall clock meaningless. `--sim` on `par concurrent` programs uses the sched handler the test
binds; selecting `pct` is the test's choice, and `cluster = sim` scheduling is as 17.7 and R988 say.

Options: (a) the above; (b) interpreter only, native unspecified. (b) leaves the second backend without a
stated contract. Recommended (a).

- Decision:
- Row:

### D8: conformance

Proposal: `conformance/sim-test/` gains cases, proposed as `conform/*` changes after acceptance (agents
do not write `conformance/`): a test over `mem_net` that is cached on the second run and rerun when an
item it reaches changes; a test with a planted bug reachable only under one fault, found by a sweep and
shrunk to a one-fault plan; the same seed run twice with equal trace hash; a test that binds one real
handler and is not cached; a gate run that rejects a lowered seed count. The differential run
(`tools.difftest`) covers interpreter against native once D7 allows it.

Options: (a) the above; (b) no fixtures. (b) leaves the false cache hit, the dangerous result, untested.
Recommended (a).

- Decision:
- Row:

## Spec changes

- 7.3: the test handler set paragraph says simulated tests are classified by their bindings (D1).
- 17.7: seed derivation, fault plan, `--list-faults` (D2, D3).
- 18.1: `yan test` row gains `--sim[=N]`, `--seed`, `--faults`, `--shrink`, `--verify`.
- 18.4: verdict key extension (D1); sweep size in approved tests (D5); determinism check (D6).
- 18.4: R309 I/O-test definition names the simulated exception (D1).
- 24 (conformance suite): `sim-test/` cases (D8).

Implementation order: key extension and classification on the interpreter (D1), seeds and repro line
(D3), fault plan and `--list-faults` (D2), `--shrink`, the gate rule (D5), then `--verify` once the
#992 recorder exists (D6). Native follows #992 item 1 (D7). New code goes in a new module of
`yan-cli`, not in the lowering files of practice 5.
