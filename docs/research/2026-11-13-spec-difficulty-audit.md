# Spec difficulty audit, 2026-11-13

Issue #1129. Evidence at `b65d22a` (main of 2026-11-13; the one later commit on main touches only CLI/diag code). Do not edit the figures by hand; regenerate them.

Question (Antonio): is the project doing only easy rows, and can agents deliver the full spec?

## Reproduce

```
cargo build -p yan-cli --release
uv run python tools/conform.py --executor interp=$PWD/target/release/yan --json c.json --report-only
uv run python -m tools.difftest --yan target/release/yan --json d.json --report-only
uv run python -m tools.spec_audit --conform-json c.json --difftest-json d.json \
    --csv docs/research/2026-11-13-spec-difficulty-audit.csv --summary
```

The CSV has one line for each of the 879 `R` rows (the `tools.status` spec_debt population; `SQ` rows are excluded). Columns:

| Column | Source | Kind |
|---|---|---|
| `cases` | conformance cases whose `rows` header cites the row | mechanical |
| `interp_cases` | verdict of those cases on the interpreter (`pass`, `fail`, `none`) | mechanical |
| `native_cases` | difftest verdict of the cited `run` programs on Cranelift (`ok`, `refused`, `n/a`) | mechanical |
| `cited_in` | crates whose source mentions the row number | mechanical |
| `section_cases` | cases citing any section of the row's entry | mechanical |
| `status` | `native` (cased, passes on both backends), `interp` (cased, passes), `native-uncased` / `interp-uncased` (cited in code, no case), `case-failing`, `none` | derived |
| `difficulty`, `hard`, `why` | `CLASSES` in `tools/spec_audit.py`, one class for each `rows.toml` entry | judgement |

"Done" below means `native` or `interp`: a case that cites the row passes. It is the only status that survives an independent run.

## Result

Of 879 rows: done 221 (25%), cited in code with no case 107, case failing 5, no evidence 546.

Hard-core rows (ownership, concurrency, effects, verification, staging, distributed, gpu/kernel, self-host/LLVM, formal, research track):

- 59 of 227 done (26%). Excluding the 32 research-track rows, which are post-1.0: 59 of 195 (30%).
- Not-hard rows: 162 of 652 done (25%). Hard rows are being finished at the same rate as the rest.
- Of the 168 hard rows not done: 25 are cited in code with no case, 2 have a failing case, 141 have no evidence (109 excluding research).

| Hard category | Done / rows |
|---|---|
| verification (contracts, solver, proofs) | 17 / 27 |
| distributed (sim, actors, remote) | 14 / 28 |
| concurrency | 13 / 66 |
| ownership | 6 / 14 |
| effects (capabilities, handlers, determinism) | 5 / 19 |
| gpu/kernel | 4 / 15 |
| staging | 0 / 20 |
| self-host / LLVM | 0 / 3 |
| formal (Lean) | 0 / 3 |
| research track (post-1.0) | 0 / 32 |

| Difficulty | Done / rows |
|---|---|
| 1 policy, naming | 0 / 30 |
| 2 front-end rule, std, tooling | 100 / 320 |
| 3 subsystem | 52 / 217 |
| 4 hard semantics | 49 / 235 |
| 5 research-grade | 20 / 77 |

Difficulty 4 and 5 rows are not trailing the easy ones: 69 of 312 (22%) against 100 of 320 (31%) at difficulty 2. The queue is not avoiding hard work; the hard work it has done is concentrated in verification, actors and simulation.

Backend: difftest runs 237 programs on Cranelift against the interpreter with 0 unexplained and 0 refused. Rows cited by a passing `run` case are native-ok by that run.

## Top 10 hard untouched (no evidence), non-research

Ordered by difficulty, then by what blocks the rest. Entries group rows that share one class.

1. R967 self-hosting stages S0 to S5 with byte-equality gates (5, selfhost).
2. R969 compiler written in Yan (5, selfhost).
3. R968 LLVM C API backend for debug and release (5, llvm).
4. R680 Lean 4 model of the core, with R764 and R765 (5, formal).
5. R607 to R609 invariant inference and proof power (5, verification).
6. R404 to R419 `cluster` capability and location-transparent `Remote` handles, 10 rows (5, distributed).
7. R960 `cluster = sim(nodes, seed)` in-process nodes (5, distributed).
8. R130 to R144 compile time, with reflection R939 to R942: 0 of 20 staging rows (4, staging).
9. R110 to R124 memory and concurrency model, 15 rows (4, concurrency).
10. R150 to R167 networking and async I/O, 18 rows (4, concurrency).

Also dark: R320 to R329 embedded and GPU profiles (9), R450 to R459 model calls and durable resume (5), R673 to R677 security model (host handlers).

## Honest assessment

What the evidence supports. No hard row has been attempted and failed: only 2 hard rows have a failing case. Verification (17/27), actors and simulation (14/28) and part of concurrency are done at the same rate as easy rows, so agents have delivered work of that kind. The risk is untouched work, not failed work.

What the evidence cannot say. `none` means no case and no code comment cites the row. It overstates the gap: R3, R4 (`par concurrent`) and R961 (`yan-flow`) are `none` though the feature exists, because the cases cite sections, not rows. 141 rows (41 hard) have no case on any of their sections either; those are the genuinely dark ones. Difficulty is one reviewer's judgement per `rows.toml` entry, not measured effort; check `CLASSES` before quoting a difficulty figure.

What looks beyond current agent capability, with reasons. This is judgement, not measurement.

- Self-hosting and LLVM (R967 to R969). Not hard per construct; hard because the gate is byte-equality of a bootstrapped compiler across stages, a long chain where one divergence blocks all later work, and it needs an LLVM toolchain and many weeks of coherent architecture. Within reach only as a decomposed, gated ladder with a lead owning the order. Highest risk.
- Lean core model (R680). A faithful model plus preservation proofs is long-horizon proof engineering; agents manage lemmas, but keeping a theorem-sized development consistent with a moving spec has no precedent in this repo. High risk.
- Invariant inference and the exact solver (R605 to R609, the exact solver and proof power, all with no evidence). Open-ended heuristic tuning with no crisp stopping test; cases pass, but whether it is good enough is judged against real programs. Medium to high.
- Real multi-node `cluster` (R404 to R419). The simulated version is in reach and partly done (R960 aside); real partitions and network behaviour need multi-machine infrastructure the cloud sessions do not have. Medium for sim, high for real.
- GPU and embedded profiles (R320 to R329). Needs hardware the agent sessions lack, so correctness cannot be checked there. Blocked by environment, not skill. The kernel rows that are testable without a device (R971 to R973) are already native-ok.

Within reach, but large: staging (0/20), the memory/concurrency model rows, networking and async I/O. These are volume and sequencing problems; none is a research problem.

Recommendation: add row citations to existing cases so `none` becomes real (a conform PR; agents may not edit `conformance/`), then re-run this script. Pick one hard 0/N category (staging) as the next probe: it is the largest hard block with no evidence and not blocked by hardware.
