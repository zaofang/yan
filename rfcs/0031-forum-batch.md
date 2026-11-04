# 0031: Numeric panic operands, item-level deny, a longer cool-off, target-typed variants as a non-goal

- Status: accepted (decisions only; rows pending)
- Research: [Language forums study](https://claude.ai/artifact/E8xVSw3fhTwuxFu38DqrtT) items F1, F3 and F7;
  [Python Ideas and PEPs study](https://claude.ai/artifact/R1t3UGwQ9VKSaNbGjHPfVQ) items I2 and I4
- Rows: filled in when accepted

## Summary

Three small changes the lead adopted from the forums study. A panic payload carries the operand values
of the failing check, limited to numbers so a crash report can never leak text or a wrapped secret (F1,
which narrows Python I2). An item can deny a lint with `@deny(code)`, the mirror of `@expect` (F3; Python
I4 needs nothing new, since `@expect(code, reason)` already covers it). The dependency cool-off grows
from 3 to 7 days, and a forced move off a yanked or malicious version skips it (F7). Variant names resolved from the expected type become a recorded non-goal (F14). Two related
properties are recorded in docs, not here: no build scripts and no syntactic macros as a supply-chain
property (F13, `docs/architecture.md`), and why the agent rules are mechanical (F15, `CONTRIBUTING.md`).

## Decisions

### D1: numeric operand fields in panic payloads

Today a panic prints `panic(kind): message` and a stack trace (8.3); a `contract` payload names the
clause. Python I2 proposed printing the operand values of the failed check, as pytest does for `assert`.
The Rust internals thread "Stop including string content in index panics?" shows the cost: Zed's crash
collector received user buffers, API keys included, because `str` index panics echo the string. The fix
under review prints byte index and length only.

Proposal: each panic of kind `overflow`, `div_zero`, `bounds`, `shape`, `contract` or `assert` carries a
list of named operand fields. A field is included only when its static type is an integer, float, bool or
a unit-carrying number; text, bytes, collections, records, and any value of type `Secret[T]` or
`Untrusted[T]` are left out and shown as their type name only. Typical fields are index, length, divisor, key
hash with key type, and the contract clause id; for a `contract` or `assert` clause they are the numeric
free variables of the predicate, for example `i = 3, len(xs) = 3`, the shape the undecided-checks rule
already uses for a static counterexample. Local debug runs may add an escaped, length-bounded excerpt of a
text operand to the terminal message; it never enters the payload or the replay buffer. Fields are computed only on the panic path, so the
success path has no cost; the record-and-replay buffer stores them as data.

Options weighed: (a) no operands, as today; (b) every operand through its formatting trait, as I2 first
proposed; (c) numbers only. (b) breaks the Secret and Untrusted guarantees on the one path with no
review. (c) keeps most of the debugging value, since bounds, overflow and shape failures are numeric.

Recommendation: (c).

- Decision: (c) numeric operand fields only (lead, 2026-11-04)
- Row:

### D2: item-level `@deny(code)`

`@expect(code, reason="...")` is the only suppression (18.2). Lints warn by default and deny only under
`--strict` and `--level release`, so a package that wants one lint fatal in every build mode, such as the
exposed-Secret-into-log lint in an auth module, has no way to say it.

Proposal: `@deny(code)` on an item makes that lint an error inside the item in every build mode. It takes
no reason, since it only tightens. An `@expect` of the same code inside a `@deny` item is rejected, so a
denial cannot be undone further in (Rust's `forbid`, without a second attribute). A `@deny` naming a code
that is not a W8xx lint is an error. Dependency capping is unchanged: a dependency's `@deny` stays a
warning in the consumer's build, as its other diagnostics do.

Options weighed: (a) nothing, rely on `--strict`; (b) a manifest-level deny list; (c) item-level
`@deny`. (b) is coarse and lives far from the code it guards. (c) mirrors `@expect` and reads in place.

Recommendation: (c). New diagnostic codes for the two errors are allocated only when accepted.

- Decision: (c) item-level `@deny(code)` (lead, 2026-11-04)
- Row:

### D3: 7-day cool-off, and forced moves skip it

19 already has a cool-off: `yan add` and `yan update` skip versions younger than `min_release_age`
(default 3 days) unless the version fixes an advisory affecting the lock or a human pins it with
`@=x.y.z`. The forums study (Go's dependency cooldown issue, Cargo's min publish age RFC 3923) found that most
malicious releases are caught within days, so a 3-day default misses many of them. The study missed the
existing setting, so this is a change to it, not a new one.

Proposal: raise the default `min_release_age` to 7 days. Keep both existing exemptions. Add a third:
when the locked version is yanked or marked malicious, the move to the nearest clean version ignores the
cool-off, since staying is the worse risk. The setting stays per project and can be lowered.

Options weighed: 3 days (today), 7 days, 14 days. 14 delays real fixes that carry no advisory; 7 covers
the window the study's threads report.

Recommendation: 7 days with the yank exemption.

- Decision: 7 days, plus yank/malicious exemption (lead, 2026-11-04)
- Row:

### D4: target-typed variant names are a non-goal

Kotlin's context-sensitive resolution proposal (KEEP issue 379) resolves a bare variant name from the
expected type, so `Red` stands for `Color.Red` where a `Color` is expected. Its critics' main point is
that a new declaration elsewhere can silently change what existing code resolves to. That breaks law 3
(one meaning per text) and makes agent edits non-local; the Python study reached the same place on
leading-dot patterns.

Proposal: add to the section 26 non-goals list: "No variant names resolved from the expected type in
Yan or Yan Agent; a variant is written with its type name, as in `Color.Red`." Other surfaces
may still offer it in their own syntax where it lowers to the qualified name. If variant-name clashes
show up in model runs, qualified `Type.Variant` stays the follow-up.

Recommendation: record it as a non-goal.

- Decision: record as non-goal (lead, 2026-11-04)
- Row:

## Spec changes

- D1: 8.3 Panics (payload fields), 14 Undecided checks (same field shape), 17 Secret and Untrusted (not
  shown in panics).
- D2: 10 attribute table, 18.2 Lints, 25 two new error codes.
- D3: 19 Cool-off.
- D4: 26 non-goals list.
