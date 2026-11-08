# Formal core (Lean 4)

A Lean 4 model of Yan-K, the fragment of Yan Core text in RFC 0022, and from phase 1 on the proofs of
the formal core study. It never ships and nothing in the Cargo workspace depends on it (R680).

| File | What it is |
| --- | --- |
| `YanCore/Syntax.lean` | abstract syntax of Yan-K |
| `YanCore/Parse.lean` | a reader for the Yan-K subset of the 3.5 grammar, independent of `yan-core` |
| `YanCore/Eval.lean` | the executable semantics: `inout` as copy-in, copy-out; `par` in source order; panics at once |
| `Main.lean` | `yancore run FILE`, with the exit statuses of the conformance runner (0, 1, 101; 2 for unsupported) |
| `drt.py` | differential testing against `yan run` |

## Use

```
cd formal && lake build                       # needs elan; lean-toolchain pins the version
cargo build --release -p yan-cli
uv run python formal/drt.py cases             # conformance/yir/run on both
uv run python formal/drt.py gen --count 1000 --seed 1 --keep /tmp/drt
```

`gen` writes random Yan-K programs over `i64`, a sum, a failure record, `inout`, `try` and `?`,
join points, loops, `par`, asserts, `requires` (a failed one is a `contract` panic), and recursive
functions, self or mutual, with `decreases` (E602 when it is left out of a module that has contracts, R940;
contract-free modules recurse without it). The model checks E602 before it runs, as `yan run` does (R901).
`ensures` clauses are emitted by default; `--no-ensures` turns them off. Each prints branch markers and residues of its values, so
two runs agree only if they took the same path and computed the same numbers. A program either side
reports as unsupported (exit 2) is skipped. Any difference is a bug in one implementation or a gap
in the spec, and `--keep` saves the program.

## Phases

Phase 0 (this) is the executable model and the differential tests. Phases 1 to 4 add an inductive
semantics, proved equal to this interpreter on Yan-K, and the theorems T1 to T5 of the study. They
start after the M1 gate. A proof with `sorry` never merges.
