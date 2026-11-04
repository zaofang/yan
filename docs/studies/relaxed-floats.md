# Block-scoped relaxed floats

Study and proposal, 2026-11-04. Docs only: no spec rows, no conformance edits, no diagnostic codes.
Items are labelled D1, D2 and so on; none carries a row number. Everything here is non-normative until
an RFC and accepted rows say otherwise.

Brief: item F10 of the [language forums study](https://claude.ai/artifact/E8xVSw3fhTwuxFu38DqrtT),
from the Swift forums thread ["Relaxed" floating-point operators](https://forums.swift.org/t/relaxed-floating-point-operators/90050).
Floats are strict IEEE today and only an item marked `@fast_math` or `@unordered` under `nondet`
relaxes them. Swift's argument is locality: relax one expression, not a file or a module. The forums
study recommended allowing `@fast_math` on a block or expression as well as an item, with no new
operators. This study says what the relaxation is, how far it reaches, and how it is spelled.

## 1. Answer

Yes, as one block form, `with fast_math:`, plus a fixed and safe meaning for the word:

    pub fn energy(xs: list[f64], w: list[f64]) -> f64 uses nondet:
        var acc = 0.0
        with fast_math:
            for i in range(len(xs)):
                acc += w[i] * xs[i] * xs[i]
        return acc

- Inside the block, the float operators written in the block may be contracted into FMA,
  reassociated, and treated as if the sign of zero did not matter and `a / b` were `a * (1 / b)`.
  Nothing else changes: NaN and infinity keep their IEEE meaning, subnormals are kept, and calls keep
  their callee's semantics.
- The scope is lexical. A function called from the block is not relaxed, and inlining never carries
  the relaxation into a callee's body or out of the block.
- The block needs `uses nondet` exactly as the item attribute does. The function signature still says
  so, `yan audit` and grep find the word, and the strict default keeps its named exit (syntax
  consolidation F10).
- The item attribute `@fast_math` stays and means "the whole body is one `with fast_math:` block".
- No new operators and no expression form. One-expression relaxation is a one-line block, and the
  one relaxation people most often want per expression, a fused multiply-add, is already written
  `fma(a, b, c)` and stays strict and correctly rounded.

## 2. The gap today

- `@fast_math` attaches only to items (10.6), so the smallest relaxed unit is a function. A numeric
  kernel that wants one relaxed inner loop and a strict accumulation around it must split into two
  functions, and the split is a design artifact, not a design.
- The spec never says what `@fast_math` permits. 3.2 and 7.7 list what strict forbids (contraction,
  reassociation, flush-to-zero), and 11.1 calls the result tier F, but there is no list of the
  allowed rewrites. Read as C's `-ffast-math`, it would include "no NaNs" and "no infinities", which in
  LLVM turn a violated assumption into poison. That is undefined behaviour in safe code, which law 3
  rules out. The gap matters more than the scope question: a block form must not widen a hole.
- Flush-to-zero is a thread mode on x86 and ARM (MXCSR, FPCR), not an instruction property. Allowing
  it per block would bring back the floating-point environment that 11.1 removed.

## 3. Prior art

| Language | Mechanism | Scope | Notes |
| --- | --- | --- | --- |
| C, C++ (Clang) | `#pragma STDC FP_CONTRACT`, `#pragma clang fp reassociate(on)`, `contract(fast)` | compound statement | Per-flag and lexical; the closest model. |
| GCC | `__attribute__((optimize("fast-math")))`, `-ffast-math` | function, translation unit | Function-level flags interact badly with inlining; GCC documents the attribute as not for production use. |
| LLVM IR | flags `reassoc`, `contract`, `nsz`, `arcp`, `afn`, `nnan`, `ninf` per instruction | instruction | Per-instruction flags are what make a lexical scope cheap. `nnan` and `ninf` make violating values poison. |
| Julia | `@fastmath expr` | expression or block, lexical | The macro rewrites operators in its argument; callees are untouched. Widely used, often criticised for `nnan`. |
| Zig | `@setFloatMode(.optimized)` | enclosing block | Lexical, one switch for all flags. |
| Rust | `fadd_fast` and friends (unsafe, UB on non-finite); `algebraic_add` and friends (unstable, safe) | per operation | The algebraic methods allow reassociation and contraction but not the NaN and infinity assumptions, so they stay safe. That split is the one Yan needs. |
| Swift | Swift Numerics `Relaxed.sum`, `Relaxed.product`, `Relaxed.multiplyAdd`; forum pitch for `&+` and `&*` | per operation | The pitch argues locality and that a module flag is too coarse. |
| Numba | `fastmath=True` or a flag set such as `{"contract", "reassoc", "nsz", "arcp"}` | function | Shows that a named safe subset is usable in practice. |
| CUDA, PTX | `-use_fast_math`; `__fadd_rn` and `__fmul_rn` block contraction; `.ftz` per instruction | module, instruction | On GPUs flush-to-zero is per instruction, on CPUs it is not. |
| Java | `strictfp`, made the only behaviour in Java 17 (JEP 306) | removed | The non-strict mode was dropped once hardware made strict cheap. Yan already starts strict. |

What the prior art agrees on: the scope should be lexical, the useful flags are reassociation and
contraction, and the NaN and infinity assumptions are where fast-math becomes a correctness bug.
Per-operation APIs (Rust, Swift) and block switches (Clang, Zig, Julia) both exist; block switches win
on loops, which is where relaxation pays.

## 4. Proposal

### 4.1 What relaxed means

A float operation in a relaxed scope may be evaluated as any expression that is equal to it in real
arithmetic under these rewrites, and nothing else:

1. **Contract**: `a * b + c` and `a * b - c` may become one fused operation.
2. **Reassociate**: `+` and `*` chains may be regrouped and reordered, including across loop
   iterations (vectorised reductions).
3. **Ignore the sign of zero**: `x + 0.0` may become `x`, `-(a - b)` may become `b - a`.
4. **Reciprocal**: `a / b` may become `a * (1 / b)`, and `1 / b` may be hoisted.

Not included: assuming no NaN, assuming no infinity, flush-to-zero, approximate library functions.
A relaxed result is still a float value that some reassociated real expression could have produced
with IEEE rounding at each step; it is never poison, never undefined, and a NaN input still gives a
NaN or a value the rewrites allow. This is Rust's algebraic set. Whether `afn` (approximate libm)
belongs here is D4.

### 4.2 Scope

- `with fast_math:` relaxes the operators written lexically inside the block, including nested
  blocks, closures written in it, and `par for` bodies in it.
- Calls are not relaxed. `sum(xs)` inside the block keeps its 9.3 reduction order; `math.sin` keeps
  tier E. A tier-F std API (`sum(unordered=True)` or a vendor binding) is how a call is relaxed.
- Inlining is semantics-preserving: a callee inlined into a relaxed block keeps strict flags on its
  own operations, and a relaxed callee inlined into strict code keeps its relaxed flags. With LLVM's
  per-instruction flags this is the default behaviour, not extra work.
- Generic code: a generic function instantiated from a relaxed block is not relaxed. Relaxation is a
  property of source text, not of a type.
- Nesting: `with fast_math:` inside `@fast_math` is redundant and gets a lint (D8). There is no
  `with strict:` to switch back; the strict island is a call to a strict function (D7).

### 4.3 Capability and determinism

- A function containing `with fast_math:` uses `nondet`, inferred for private functions and written
  for `pub` ones (7.2), the same rule as the item attribute. Becoming relaxed is a major change for a
  `pub` function (20.6, "become `nondet`").
- Relaxation is a permission, not a requirement. The interpreter and the Cranelift debug backend
  evaluate the block strictly; the LLVM release backend may apply the rewrites. A relaxed function may
  therefore give different bits in debug and release, across targets and across toolchain minors,
  which is exactly why it sits under `nondet`. Within one binary on one machine the result is
  repeatable, so record and replay (7.6) needs no trace entry for it.
- Stage 0 (10.4) rejects `with fast_math:` as it rejects `@fast_math`.
- GPU kernels (17.1, R328) keep the item form; a block form inside a kernel follows the same rules.

### 4.4 `@unordered`

`@unordered` relaxes reduction and iteration order, not float rewrites. A block form for it is
plausible but is a separate question with different users (parallel folds, not numeric kernels), so
this study leaves it as D9 and does not propose it.

### 4.5 Spelling

`with fast_math:` reuses `withstmt` in Yan Agent (13) and plain Yan (15). It is a scope that changes
how the block is evaluated, like `deadline`, `timeout` and the proposed `with shield(d):`; it binds no
handler. Alternatives:

- A statement attribute, `@fast_math` on the line before a block. Neither grammar has statement
  attributes; adding them for one use opens a second place for every attribute.
- Operators (`&+`, `&*`, Swift's pitch). Rejected by the forums study: new glyphs for one job, and
  they do not cover loops.
- Per-operation functions (`relaxed.add`). Verbose in the loops that need them; could come later as
  std sugar over the block (D6).

Yan Core lowers the block to a scoped float-mode marker on each float operation it contains; no new
Core statement is needed if Core ops already carry flags, or one `relaxed { ... }` statement if they
do not (D5).

## 5. Cost

- Spec: define the relaxed set (4.1) once, in 7.7, and point 3.2, 10.6 and 11.1 at it; add the block
  to both `withstmt` rules. The definition is needed even if the block form is declined.
- Compiler: a flag on float ops in the IR, set lexically; the LLVM backend maps it to
  `reassoc contract nsz arcp`. The interpreter and Cranelift ignore it. No M1 work: relaxed code is
  correct when run strictly.
- Tests: conformance needs a witness that NaN and infinity survive a relaxed block and that a callee
  is not relaxed. Those are lead-owned changes in a separate PR.

## 6. Open decisions

| Item | Question | Recommendation |
| --- | --- | --- |
| D1 | Add a block form at all? | Yes: `with fast_math:`, lexical. |
| D2 | Define the relaxed set? | Yes, for item and block alike: contract, reassociate, ignore sign of zero, reciprocal. Needed regardless of D1. |
| D3 | Allow no-NaN and no-infinity assumptions anywhere in safe code? | No. They make poison out of ordinary inputs. If ever wanted, only under `unsafe`. |
| D4 | Allow approximate libm (`afn`) in the set? | No in 1.0; tier-F library functions are named calls. |
| D5 | Yan Core form? | Per-op flag if Core ops carry attributes; otherwise a `relaxed { ... }` statement. |
| D6 | Expression form or operators? | Neither. A one-line block covers it; `fma` covers the common case. Revisit per-op std functions after kernels exist. |
| D7 | A `with strict:` escape inside a relaxed scope? | No; call a strict function. |
| D8 | Lints? | Two warnings: a relaxed block with no float operator in it; a block nested in a relaxed scope. |
| D9 | Block form for `@unordered`? | Not here; a separate study if a user appears. |
| D10 | Flush-to-zero? | Out of the relaxed set on CPUs. On GPUs, item-level `@fast_math` on a kernel may allow it; decide with the GPU rows. |
| D11 | Does `yan view` mark relaxed spans? | Yes: an editor shade and a listing in `yan audit`, from the tree, no new syntax (consolidation F6). |

## 7. Not proposed

- Module, package or command-line fast-math. The point of F10 is that the exception is a word in the
  source next to the code it changes.
- A dynamic (call-stack) scope. It would make a strict library's results depend on its caller.
- A floating-point environment of any kind, including per-block rounding modes. Directed rounding
  stays an explicit argument (11.1).

## 8. Next step

If the lead accepts D1 and D2, an RFC from `rfcs/0000-template.md` states 4.1 and 4.2 as normative
text; D2 alone is worth taking first, because it closes the undefined meaning of the existing
attribute.
