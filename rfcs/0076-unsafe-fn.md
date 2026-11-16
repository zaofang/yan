# 0076: `unsafe fn` in Yan

- Status: draft
- Research: [Design: `run --ub`, `Ptr`, `dyn`, `Arena`/`Ref`](../docs/research/2026-11-14-ub-ptr-dyn-design.md),
  [Capability security review](../docs/research/2026-11-16-capability-security-review.md)
- Issue: #1242 item 16
- Rows: filled in when accepted

## Summary

Spec 6.7 item 16, 6.8 and 6.9 define `unsafe fn` and its `pre` contracts, and Yan Core (3) has
`["unsafe"] "fn"`, but the Yan grammar (15.2) has no way to write one. This RFC adds the production
and fixes the rules the other sections leave open: where an `unsafe fn` may appear, what a call and
its body need, how `pre` is checked in each build mode and under `--ub`, whether an `unsafe fn` is a
value, and what `yan audit` lists. The lead decided the direction on 2026-11-16 (#1242): add
`unsafe fn` to 15.2 by RFC as a contradiction fix, allow `Ptr` parameters, `pre` is the precondition,
and `--ub` checks `pre` at each call. D1, D3 and D6 record that decision; the rest are the open
details.

## Decisions

### D1: grammar

The `unsafe` prefix goes on the item, not on `fundef`:

```
item      = {attr} ["pub"] (["unsafe"] fundef | gendef | subdef | lemmadef | typedef | traitdef
            | impldef | constdef | externdef) | usedecl
```

so `pub unsafe get(p: Ptr[int], i: int) -> int = ...`. After `unsafe` at item level the next token
decides: `impl` or `extern` keep their meaning, a NAME starts a function. One token of lookahead is
enough and `unsafestmt` is not reachable at item level, so there is no ambiguity.

Putting it on `item` rather than `fundef` keeps it out of `impldef`. Every Yan `impl` implements a
trait, and trait signatures (15.2 `traitdef`, Core `Sig`) carry no `unsafe`; an `unsafe` impl method
would let safe callers reach it through the trait. Generators, `sub` items and lemmas stay safe: Core
`Gen` has no `unsafe`, and none of them has a use for it.

Recommendation: as above. Unsafe trait methods and unsafe generators are non-goals for this RFC.

- Decision: accepted as recommended (lead, card, 2026-11-16)
- Row:

### D2: where an `unsafe fn` may appear

Only in std and `ffi` modules, as every other use of `unsafe` (2 law 2, 12, E901). Never in Yan
Agent (L6, 14.1). No new code: E901 covers it, reported at the `unsafe` keyword.

Recommendation: as above.

- Decision:
- Row:

### D3: calling an `unsafe fn`

A call to an `unsafe fn` is an unsafe operation and is legal only inside an `unsafe` block (E903), as
for an `unsafe` extern item (12.1). The caller's block is its assertion that every `pre` holds.

Recommendation: as above.

- Decision: lead, 2026-11-16 (#1242), implied by "`pre` is its precondition"
- Row:

### D4: the body is not an `unsafe` block

Options:

1. The body of an `unsafe fn` is an implicit `unsafe` block (Rust before 2024).
2. The body is ordinary code; each unsafe operation in it still needs its own `unsafe` block (E903),
   as Rust 2024 makes the default (`unsafe_op_in_unsafe_fn`).

Recommendation: 2. `unsafe` then has one meaning per position: on a function it states an obligation
for callers, on a block it discharges one. Each unsafe operation stays visible at its own site, so
`yan audit` and `yan review` (R667) count real unsafe sites rather than whole bodies, and adding an
operation to an `unsafe fn` body is a visible change. The cost is one more indent in short wrappers.

- Decision:
- Row:

### D5: `pre` across build modes

6.8 says an `unsafe fn`'s preconditions are its `pre` contracts, evaluated by the checked interpreter.
8.5 checks `requires` at runtime in `check` and, unless proven, in `release`. Applied unchanged, 8.5
would check every `unsafe fn` precondition in release builds, which defeats the reason to write one
(`get_unchecked(i)` with `pre i < len` would pay the bounds check it exists to skip), and would make
6.7 item 16 unreachable.

Options:

1. 8.5 unchanged: `pre` of an `unsafe fn` is checked like any `requires`; violation is a `contract`
   panic, never UB.
2. Checked in `check` (a `contract` panic, as 8.5), trusted in `release` (not checked, no checked
   entry for `pub` callers, R381 does not apply), reported as `ub(16)` under `--ub`.
3. Only `--ub` checks it; `check` and `release` both trust it.

Recommendation: 2. A precondition the caller asserted in an `unsafe` block is trusted where speed is
the point and checked wherever checking is cheap to opt into, which is what a debug build is for.
Under `--ub` the report names 6.7 item 16, the function and the failing clause by its position among
the item's `requires` and `ensures` (as R882 names a clause), with a `yan replay` trace, instead of a `contract`
panic, so the checker's output says which obligation the unsafe caller broke. `@prove` modules prove
`pre` at each call site as for any function; a proven call is trusted at every level. R340 is
unchanged: a safety check in the callee is never removed on an unchecked `unsafe fn` `pre`.

A `pre` is a pure predicate (8.4) in every case, so it cannot use a capability or allocate
observably. Obligations that no pure predicate can state (provenance, liveness, aliasing) are already
6.7 items 2 to 5, which `--ub` checks directly; a checker-only predicate such as "valid for reads of
`n` elements" is left to a later RFC if a use appears.

- Decision:
- Row:

### D6: `Ptr` in the signature

3 already allows a `Ptr` to be built and passed wherever `unsafe` is permitted, so `Ptr` parameters
and results are legal in any function of an `ffi` module or std, `unsafe` or not; only `read`,
`write` and the other dereferencing operations need an `unsafe` block. Today `Ptr` in a parameter is
E017 (#1242 survey); that is a toolchain gap, not a spec rule, and the implementation removes it. An
`unsafe fn` is the usual place for a `Ptr` parameter because validity of the pointee is a caller
obligation.

Recommendation: no spec change beyond a sentence in 15 pointing at 3.

- Decision: lead, 2026-11-16 (#1242), "`Ptr` params are allowed in an `unsafe fn`"
- Row:

### D7: an `unsafe fn` is not a value

Options:

1. Naming an `unsafe fn` anywhere but in the callee position of a call is an error (a new E9xx code).
   There is no `unsafe` function type.
2. Add `unsafe` function types (`unsafe (int) -> int`), whose calls need an `unsafe` block, and let
   an `unsafe fn` coerce only to them.

Recommendation: 1. It is sound with no new type, and no current use needs an unsafe callback: C
callbacks are typed by the extern item (`scoped` and `escaping` function parameters, 12.5) and are
always safe Yan functions. 2 can be added later without breaking any program that 1 accepts. Without
either, an `unsafe fn` passed as a safe `fn` value would let safe code call it, so the rule is
required, not optional.

- Decision:
- Row:

### D8: audit, review and the FFI consent

`yan audit` (12.10) lists every `unsafe fn` with its `pre` lines and every call site with the
enclosing `unsafe` block. `yan review` already reports `unsafe` gained or lost (R667) and treats
added or removed contract lines on an item that had any as frozen (R668); an item becoming or ceasing
to be `unsafe` counts as `unsafe` gained or lost.

Capability safety: an `unsafe fn` has a `uses` list like any function, and `unsafe` grants no
capability; the ceiling (E314) and `uses none` (12.2) apply unchanged. The capability review (F2,
#1389) found that the `allow_ffi` consent is not enforced and that "ffi module" is detected by a
directory name. Today 12 says `allow_ffi` is consent that a dependency contains extern blocks. An
`ffi` module with `unsafe fn` items and no extern block can break every guarantee of law 2 just as
an extern block can, so the consent should cover any `ffi` module, not only extern blocks.

Recommendation: the audit and review rules above, and widen the 12 sentence to "the root package's
`allow_ffi` list is its consent that a dependency contains `ffi` modules". The enforcement itself
stays with #1389.

- Decision:
- Row:

## Spec changes

- 15.2: the `item` production (D1). 15.3 Layers: one sentence that `unsafe fn` is L6 and `Ptr` may
  appear in signatures where 3 allows it (D2, D6).
- 3: rules on the grammar: a call to an `unsafe fn` needs `unsafe` (D3); the body is not an `unsafe`
  block (D4); an `unsafe fn` is not a value (D7).
- 6.8: the `pre` sentence gains the build-mode rule and the `ub(16)` report (D5). 6.9: the UB checker
  bullet names `unsafe fn` `pre` at each call (D5).
- 8.5: an `unsafe fn`'s `requires` row follows D5 (checked in `check`, trusted in `release`, no
  checked `pub` entry).
- 12: the `allow_ffi` sentence (D8). 12.10: `yan audit` lists `unsafe fn` items and their call sites
  (D8).
- 25.11: one new code for D7.

## Conformance cases

Proposed on branch `conform/unsafe-fn` (the guard keeps `conformance/` out of this PR). They use only
existing codes and land with the rows; the D7 case waits for its code.

| Case | Item | Expects |
| --- | --- | --- |
| `conformance/yn/fail/unsafe/e901_unsafe_fn_outside_ffi.yn` | D2 | E901 at the `unsafe` keyword |
| `conformance/yn/fail/ffi/e903_unsafe_fn_call_outside_block.yn` | D3 | E903 at the call |
| `conformance/yn/fail/ffi/e903_unsafe_fn_body_not_unsafe.yn` | D4 | E903 at the `read` |
| `conformance/yn/fail/ffi/unsafe_fn_ptr_params_accepted.yn` | D1, D3, D6 | no diagnostics |
| `conformance/ub/unsafe_fn_pre_holds.yn` | D5 | `ub: none` |
| `conformance/ub/unsafe_fn_pre_violated.yn` | D5 | `ub: 16`, on a call whose access is in bounds |
| (after numbering) `conformance/yn/fail/ffi/eNNN_unsafe_fn_as_value.yn` | D7 | the new code |

## Implementation (after acceptance, separate PRs under #1242)

1. Parser and elaboration: the production, E901, E903 for calls and in bodies, the D7 check.
2. Remove E017 for `Ptr` in signatures of `ffi` and std functions; lower `Ptr` parameters on
   interpreter, `yan-machine` and native.
3. `yan-machine`: evaluate `pre` at each `unsafe fn` call, report `ub(16)`.
4. Build modes: `check` evaluates `pre`; `release` skips it and emits no checked `pub` entry.
5. `yan audit` listing when `yan audit` lands (M2).
