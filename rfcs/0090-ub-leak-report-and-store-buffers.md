# 0090: Leak report and store-buffer weak memory under `--ub`

- Status: draft
- Research: code reading for #1665 (no study; facts cited below); atomics design D7 (`docs/research/2026-11-16-atomics-mutex-design.md`)
- Rows: numbered with the cases of each slice (CASE WITH ROW), as the final commit after merging main

## Summary

Spec 6.9 lists "store-buffer weak memory" and "a leak report" as parts of `yan test --ub`.
Neither can carry a conformance case today, so neither is closeable as a bugfix. Stage claimed:
working, then right.

- Leak report: `yan_machine::run` returns the allocations live when `main` returns, and
  `yan-cli/src/main.rs` (`machine`) prints `leak: N allocations, B bytes, live when main returned`
  on stderr under `--ub` and exits 0. No spec row defines the text, the exit status or the
  allocations excluded (string literals, the scratch block of `rt.rs`), no test exercises it, and
  `tools/conform.py` can assert only an exit status, stdout and a `ub(N)` code, so no case can
  expect "leaked" or "not leaked".
- Store buffers: `Inst::Atomic` (`yan-machine/src/lib.rs`) executes as one step on one word and
  every load returns the newest store. Outcomes only weak memory allows (store buffering, load
  buffering through relaxed loads) are never produced. D7 of the atomics design defers the fix
  to slice S6 and says it does not change any clock of D5.

## Decisions

### D1: the leak report is a diagnostic with a code, not free text

A leak is not UB (6.7), so it is not `ub(N)`. The report becomes a warning-level diagnostic with
its own W code (family chosen by `tools/rows.py next-code W8` at numbering time), printed once
per run on stderr, listing each leaked allocation by size and allocation site, and the exit status
stays 0 unless `--ub=deny-leaks` is given, which exits with the UB status. Excluded: string
literals, runtime-owned blocks (scratch, arena headers), and allocations still reachable from a
`static`. Native and the interpreter never report it (checked machine only), as for item 8.

Alternatives weighed:

- *Keep free text.* Rejected: nothing can assert it, and the text is already unspecified.
- *Make a leak ub(N).* Rejected: contradicts 6.7, which lists what is UB.
- *Report only a count (today).* Rejected: gives no site, so it cannot be acted on.

- Decision: (lead)
- Row: numbered in the first slice

### D2: `tools/conform.py` learns a `-- warn:` header

To let a case expect the leak code, the harness gets a `-- warn: W8xx` header matched against
the stderr of `run --ub`, with `catalogue.toml` entry required (practice 3). `tools/` and
`conformance/` are lead-owned, so this is the lead's change or a paired PR.

Alternative: assert the text in an `expect.toml`. Rejected: ties a case to wording.

- Decision: (lead)
- Row: numbered in the first slice

### D3: store buffers as a per-location bounded history, chosen by the run's seed

Per atomic location the machine keeps the last K stores (K = 4) each with its writer's vector
clock and its position in the modification order. A load returns any store that coherence, the
reader's view of that location and happens-before allow; the choice is drawn from the run's
seed, so `yan replay` reproduces it. RMWs and failed-CAS reads see the newest store (C++20).
SeqCst loads see the newest store at or after the global SC clock. Nothing in `race.rs` changes:
the clocks D5 assigns to an access are those of the store the load chose. `explore(bound)`
(6.9) enumerates the choices instead of drawing them.

Alternatives weighed:

- *Unbounded history.* Rejected: memory grows with every store in a loop.
- *Draw from the seed per load without coherence checks.* Rejected: produces outcomes RC11
  forbids, so a report could be a false positive.
- *Do nothing until `explore` exists.* Rejected: 6.9 promises it under `--ub`, and
  newest-store loads make the checker pass programs that fail on weak hardware.

- Decision: (lead)
- Row: numbered in the first slice

### D4: slices and cases

| Slice | Scope | Cases (via `conform/*`) |
|---|---|---|
| S1 | D1 leak diagnostic and its tests in `yan-machine` and `yan-cli` | `ub/leak_reported`, `ub/no_leak_when_freed` (need D2) |
| S2 | D3 store history and seeded choice | `ub/atomic_store_buffering_allowed` (the (0, 0) outcome appears under some seed), `ub/atomic_release_acquire_forbids_stale` (never appears) |

S2 depends on atomics slice S4 (clocks) and on D2 for any seed-dependent case: a case that
passes for some seeds and not others needs `-- seeds:` or an `explore` run, so S2's cases wait
for the `explore` handler or a lead-approved `-- seeds:` header.

## Spec changes

6.9: define the leak report (D1, with the exclusions) and state that atomic loads under `--ub`
may return any RC11-allowed store (D3). Appendix A rows and the W code are numbered with S1.
