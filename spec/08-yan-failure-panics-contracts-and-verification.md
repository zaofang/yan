## 8. Yan: failure, panics, contracts and verification

Recoverable failure is a typed value. Bugs are panics, and a panic aborts the process. Contracts are one mechanism with three enforcement levels.

### 8.1 Typed failure

A function declared `-> T fail E` returns either a `T` or an `E`. Failure is not a capability.

- The calling convention returns the error in a dedicated register with a flag, as in Swift's typed throws, so the success path costs one predictable branch.
- `Call?` propagates. When the caller's error type is a union that contains the callee's error type exactly (or a union of its members), `?` injects the value with no conversion. Otherwise a `From` implementation MUST exist and is named with `via`; a missing conversion is E310. If both injection and a `From` impl apply, that is E313.
- `E` may be an anonymous union of nominal types, written `A | B`. Yan MAY infer the union on non-`pub` items; Yan Agent MUST spell it out. A union of more than 8 members is lint W805, which suggests a named sum.
- `try Call` reifies the outcome as `Result[T, E]`. A `try` result that is never inspected is E311. More generally, discarding a non-unit result is E312; write `_ = f()` to discard on purpose (in Yan, let \_ = f(); R374).
- `x else err -> h` (both surfaces) evaluates `h` when `x` fails, binding the failure to `err`, or `_` when `x` is an absent `Option`.
- `x.context(msg)` wraps a failure with a message, building a context chain that `Show` prints outermost first. Failure glue (R432): @derive(From) on a sum derives From for each variant with exactly one field of a nominal type, when no two such fields share a type. The std type Error holds any failure that implements Show, with its context chain; ? injects every failure into Error with no From impl, and e is HttpError(status:) downcasts. A pub item of a library package whose fail type contains Error is lint W834; applications and jobs may use it freely. Debug builds capture a backtrace where a failure value is created; release builds do not.
- A failure raised in a non-escaping closure argument leaves the higher-order call as the caller's failure (section 4.6).
- Standard failure types include `Timeout`, `Cancelled`, `Draining` and `Deadlock` (section 9), `OutOfMemory` (5.5) and `QueryError` (11.9).
- When `main` returns a failure, the `Show` of the error goes to stderr and the exit status is 1 (SQ23). main may return () or ExitCode, and a failure type may implement trait Exit (code(self) -> u8) to choose the status of a failing main (R430).

### 8.2 Integer arithmetic in queries

Integer overflow and failed strict casts inside a table query end the query with `fail QueryError`, not an abort; wrapping, saturating and widening forms are explicit expressions (section 11.9). Everywhere else overflow panics.

### 8.3 Panics

The causes are: integer overflow, division by zero, a failed `requires`, `ensures`, `assert` or `invariant`, a failed runtime exclusivity check, `unreachable`, unproven indexing in `check` mode, a shape mismatch, an out-of-range time value, and a global deadlock (R396).

A panic aborts the process. It prints `panic(kind): message` and the panicking task's stack trace, writes the record-and-replay ring buffer if one is bound, and exits with status 101. There is no unwinding, no landing pads and no catch, and drops do not run. Isolation from bugs is a process boundary: std's process supervisor uses it, and `yan test` runs each test file in its own process.

Panic kinds: `overflow`, `div_zero`, `contract`, `exclusivity`, `bounds`, `shape`, `unreachable`, `assert`, `deadlock`.

**Operand fields (R792).** A panic of kind `overflow`, `div_zero`, `bounds`, `shape`, `contract` or `assert` carries named operand fields: index, length, divisor, key hash with key type, the contract clause id, and for a `contract` or `assert` clause the numeric free variables of the predicate. A field is included only when its static type is an integer, float, bool or a unit-carrying number. Text, bytes, collections, records, and any `Secret[T]` or `Untrusted[T]` value are left out and shown as their type name only. Fields are computed on the panic path only, and the record-and-replay buffer stores them as data. A local debug run may add an escaped, length-bounded excerpt of a text operand to the terminal message; it never enters the payload or the buffer.

### 8.4 Contract language

Contracts are expressions in Yan's pure fragment: calls only to functions with no capabilities other than `alloc`, plus `out` (the result, in `ensures`), `old(p)` (the entry value of an `inout` parameter, in `ensures`), and the bounded quantifiers `all i in a..b: P` and `any i in a..b: P`.

| Yan Core | Yan Agent and Yan | Where | Meaning |
| --- | --- | --- | --- |
| `requires P` | `pre P` | function header | caller must establish `P` |
| `ensures P` | `post P` | function header | callee guarantees `P` on normal return |
| `example E` | `ex E` | function header | `E` must evaluate to true; compiled as a test |
| `invariant P` | `invariant P` | loop header | holds on entry to every iteration |
| `decreases M` | `decreases M` | recursive function or loop | a natural-number or lexicographic measure that strictly decreases (E602 when missing on recursion that is not structural) |
| `cost steps <= A, alloc <= B` | `cost` | function header | upper bounds on work and allocation (R610) |

**Cost bounds (R610).** Each bound is a linear expression over integer parameters and `len` of parameters. `steps` counts stage-0 steps (R487) on the abstract machine, and `alloc` counts `alloc` operations before optimization, since R385 makes the real count unobservable. Either bound may be left out. Bounds are checked by automatic amortized resource analysis reduced to a linear program, with callees' `cost` clauses as their specifications. A refuted bound is E618 with a witness input. An unproven bound is W851 and compiles, in `checked-all`, to a step and allocation counter that panics with kind `contract` when exceeded, as R211 does for other undecided checks. Tools show inferred bounds as inlays on functions with no clause, in every surface. The lead admitted the Yan Agent spelling on 2026-11-01 ahead of the R566 probe.

`ex` clauses are part of an item's hash and are protected against implement patches (section 21.1). A `pub` Yan Agent function needs at least one `ex`, or a `test_` function that calls it; the requirement is E601 and is enforced only by `yan publish` and `--level release`, never by `yan check` (R284; Yan: lint W813).

### 8.5 Enforcement levels and contract semantics

| Level | Build mode | `requires` | `ensures`, `invariant`, `decreases` | Default for |
| --- | --- | --- | --- | --- |
| `check` | `checked-all` | runtime | runtime, including proven bounds and exclusivity checks | debug builds and tests |
| `release` | `unchecked-proven` | runtime at each call site unless proven, then trusted in the callee; `pub` functions keep a checked entry for callers outside the build (R381) | not checked unless `@prove` | release builds |
| `prove` | either | static, falling back to runtime with W420 | static, falling back to runtime with W420 | modules marked `@prove` (fallback is E603 there) |

Anything proven is erased at every level except `checked-all`, which guards against prover bugs. Rules (normative):

1. **Pure predicates.** A contract predicate uses no capabilities and has no observable allocation effects. A predicate that panics counts as a violation.
2. **Named build modes only.** The build mode changes whether a check runs, never typing or reachability.
3. **No silent ignore.** An unproven `requires` on a `pub` function is never skipped in release builds. A log-and-continue observe mode exists only under `yan test`.
4. **One violation path.** A violation always panics with kind `contract` and a payload naming the clause. There is no user-replaceable handler.
5. **Substitution.** Contracts on trait methods, including through `dyn Trait`, belong to the trait. An implementation may weaken a precondition and strengthen a postcondition, and nothing else.

**Trusted facts (R340).** A safety check (bounds, overflow, exclusivity, refinement or niche validity) may be removed only on facts derived from the code, or on contracts that are statically proven or checked at run time on every path to the check in the current build mode. When removal would rely on a contract that the mode does not check, such as a non-`pub` `requires`, an `ensures` or a loop `invariant` in `release`, the compiler keeps the safety check. A refinement predicate is checked at every construction of, and conversion into, the refined type in every build mode unless it is proven, and a failed check panics with kind `contract`. A false refinement can therefore arise only in unsafe code (6.7).

### 8.6 The decidable fragment

Refinement predicates, index bounds and exclusivity disjointness use only: integer variables and constants, `+`, `-`, multiplication by a constant, `len(x)`, comparisons, `and`, `or`, `not`, `in a..b`, `x & m` and `x % c` for a constant `c > 0`. Each `x % c` introduces fresh integers `q` and `r` with `x = c*q + r` and `0 <= r < c` (floored), and `x & m` with `m = 2^k - 1` is read as `x % 2^k` (R605). Any other `x & m` with `m >= 0` introduces a fresh variable bounded by `0 <= x & m <= m`. The compiler MUST include a solver for this fragment (the procedure below, R345), always on, with no external dependency.

**Normative procedure (R345).** Whether a goal is proven is a function of the program alone, identical in every implementation.

1. **Facts.** At each point the solver knows: the refinements and integer ranges of the types in scope; the condition of each enclosing `if`, `elif`, `while` or `match` guard on its taken edge and its negation on the other, including after an arm that ends in `return`, `fail`, `break` or `continue`; `a <= i < b` for `for i in range(a, b)` and `0 <= i < len(xs)` for `for i, x in enumerate(xs)`; `len(xs)` after `append` as one more, and after `pop` as one less, than before; len(xs) = n for xs bound to a list literal of n elements (R470); `k in d` after `if k in d`, `d[k] = v` or `d.setdefault(k, v)`; the predicate of each assert that dominates the point (R381); and contracts trusted under R340 (8.5).
2. **Kills.** An assignment `x = x + d`, `x += d` or `x -= d`, where `d` is linear and does not mention `x`, replaces `x` by `x - d` in every fact (R761). Any other assignment to a variable, or any `inout` use of it, eliminates it and its `len` from the linear facts by one Fourier-Motzkin step, which keeps the facts they imply about other terms, except the length updates of step 1. An elimination that would derive more than 64 facts removes every fact that mentions the variable instead (R760). Any `inout` use of `d` other than `d[k] = v` removes its `k in d` facts.
   **Joins (R762).** Where paths merge, a linear fact holds after the merge when it is a fact on one path and the facts of every other path imply it, as decided by step 3.
   **Loop invariants (R763).** For a `while` loop whose body has no `continue` to it or to an outer loop, the candidates are the linear facts at entry that mention a variable the body assigns. The body is analyzed from the entry facts after its kills, plus the candidates and the loop condition, and a candidate that the facts at the end of the body do not imply is removed. This repeats until none is removed. A body that cannot reach its end keeps every candidate. The remaining candidates are facts at the loop head, in the body and after the loop. They are proven, so they hold under `release` as well, unlike written invariants (R340).
3. **Decision (R605).** Facts and the negated goal form a conjunction of linear integer constraints. A disjunction splits into cases, at most 8 per goal. Each case is decided over the integers by the Omega test: equalities are eliminated exactly, then each variable, in order of first appearance, is eliminated by its real shadow; where the real and dark shadows differ, the case splits into splinters, taken in increasing order of their constant. The goal is proven when every case is infeasible, and refuted with the integer solution found otherwise. At most 10,000 derived constraints are generated per goal, splinters included, after which the goal is unproven.

**Exhaustiveness (R606).** Match coverage uses the same solver. Each arm's patterns, guards and the refinements and integer ranges of the scrutinee's type are facts; a match is exhaustive when the solver proves that the values no arm covers form an empty set, so `case n if n < 0` followed by `case n if n >= 0` covers `int` with no wildcard. A guard outside the fragment counts as covering nothing. E503 names a witness value that no arm covers, and its fix inserts an arm for it. An arm that can match no value left by the arms before it is W853, with a fix that removes it.

**Loop versioning (R381).** In `release`, a loop whose undecided checks have indices affine in the induction variable MUST be versioned: one range test before the loop selects an unchecked copy when it holds and the checked loop otherwise, so the panic point stays exact.

A goal the solver refutes is E501, reported with the counterexample. A goal it can neither prove nor refute compiles to a runtime check; in Yan Agent that check also raises lint W820, denied under `--strict` and `--level release` (R211, 14.1), and in Yan it raises nothing. `conformance/solver/` holds accept and reject cases at each boundary of these rules.

### 8.7 Proving

In `prove` mode the compiler generates verification conditions by weakest precondition over Yan, using callees' contracts as their specifications. Mutable value semantics means there is no heap model and no frame conditions. The conditions go to an external SMT solver (Z3 or cvc5) as SMT-LIB2 over a subprocess, with a per-item deterministic resource limit (the solver's rlimit, never wall-clock time), so a proof succeeds or fails identically on every machine. Results are cached by item hash plus the hashes of the contracts relied on. A failed proof reports the solver's counterexample as concrete values (V401).

**Invariant inference (R607).** `yan fix --infer [item]` and the MCP tool `infer_invariants` encode a failed verification condition as constrained Horn clauses, ask a CHC solver, and optionally a model, for loop invariants, check each candidate by SMT under the item's rlimit, and write the accepted ones into the source as `invariant` clauses. Checking never infers: a proof depends only on text that is written and hashed.

**Proof certificates (R608).** Every proven result is stored with a certificate in the Alethe format, keyed as results are. `yan check` verifies certificates with the toolchain's own checker and runs no solver; a rejected certificate is V402. A goal whose proof uses a rule the checker does not support is marked `trusted-solver`, and `yan audit` lists it. The registry re-checks certificates at publish (19.2).

**Ghost code and lemmas (R458).** `ghost` bindings and parameters exist only for proofs and are erased before code generation. Ghost code may read any value; non-ghost code that reads a ghost value is E604. A `lemma` item has `pre` and `post` clauses and a body that is a proof: calls to other lemmas, `assert`, case splits and induction through `decreases`. A lemma uses no capability (E605) and is never compiled; calling one adds its `post`, under its `pre`, as a fact for the prover. In `@prove` modules `assert P` is both an obligation and a fact. `@opaque` on a pure function hides its body from the prover until a proof writes `reveal f`, which keeps results stable under the rlimit. Ghost code and lemma bodies never change behaviour, so an implement-step patch MAY add or change them (21.1); changing a lemma's `pre` or `post` is a protected edit (P006). `ghost` and `lemma` are soft keywords in Yan Agent and Yan (R347), and Yan Agent may use them, and proof asserts, only in `@prove` modules. A module is marked by a @@prove line at the top of the file, before the docstring and the items, in both surfaces (R466).
