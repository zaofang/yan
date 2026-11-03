# 0011: Proof and test power

- Status: accepted (the lead chose "Accept all three" on 2026-11-01)
- Research: [Theory and compiler frontier survey](https://claude.ai/artifact/YNj73yYvBWphMVzAdNLABE), items 5, 6 and 7
- Rows: R607 to R609

## Summary

Prove mode (8.7) needs hand-written loop invariants and trusts the external SMT solver's answer, and
the test runner finds failing inputs only by fuzzing. This RFC adds invariant inference as a tool
whose output is ordinary source, proof certificates checked in-tree, and symbolic search over the
reference interpreter.

## Decisions

### D1: invariant inference written back as source

`yan fix --infer [item]` and the MCP tool `infer_invariants` encode a failed verification
condition as constrained Horn clauses, ask a CHC solver (and optionally a model) for loop invariants,
check each candidate by SMT under the deterministic rlimit, and write the accepted ones into the
source as `invariant` clauses. Checking never infers: a proof depends only on written, hashed text.
Sources: Spacer, Golem; Lemur (ICLR 2024).

- Decision: accepted, 2026-11-01
- Row: R607

### D2: proof certificates

Prove mode stores a certificate with each proven result, in the Alethe format, keyed as 8.7 keys
results. `yan check` verifies certificates with an in-tree checker (Carcara or a port) and runs no
solver. A rejected certificate is V402. A goal whose proof uses a rule the checker lacks is marked
`trusted-solver` and `yan audit` lists it. The registry re-checks certificates at publish and
records each `pub` item's proven contracts beside its computed capability set.

- Decision: accepted, 2026-11-01
- Row: R608

### D3: symbolic search over the reference interpreter

`yan test --symbolic[=bound] [item]` evaluates items on the checked interpreter with symbolic
inputs and symbolic handlers for capabilities, as Rosette's symbolic VM does, and searches within the
bound for inputs that violate a `post`, an `assert` or any panic kind. Each input found is written
to `.yan/found/` as a proposed test, as `--fuzz` does. For a `pub` Yan Agent function with no `ex`, it
proposes `ex` lines for a person to approve. If R557 is adopted, `yan diff --semantic` uses it
for bounded equivalence outside the decidable fragment.

- Decision: accepted, 2026-11-01
- Row: R609

## Spec changes

8.7 (D1, D2); 18.1 (`yan fix --infer`, `yan test --symbolic`); 18.4 (D3); 19 (registry records
proven contracts); 21.4 (`infer_invariants`); 25 (V402).
