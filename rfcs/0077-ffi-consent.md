# 0077: FFI consent, enforced at the root

- Status: draft
- Research: [capability security review, 2026-11-16](../docs/research/2026-11-16-capability-security-review.md) (finding F2, #1389 item c)
- Rows: filled in when accepted

## Summary

12 says the root package's `allow_ffi` list is its consent that a package contains foreign code
(widened to every `ffi` module by RFC 0076 D8), and 12.2 says a library needs that consent even when
all its items are `uses none`. Nothing checks it: the manifest key is accepted and ignored. This RFC
fixes the rule and where it is enforced. Lead decision 2026-11-16 (card on #1389): root-gated, with its
own diagnostic.

## Decisions

### D1: who needs consent

Every package that contains an `ffi` module or an extern block, the root package included, must be
named in the root's `allow_ffi`. An entry is a package name (`@org/name`), not a version: a version
bump keeps the consent. A package with no `ffi` module and no extern block needs no entry. `allow_ffi`
in a non-root package is ignored, as for `overrides`.

Options weighed: (a) dependencies only, the root's own `ffi/` implicit; (b) every package including the
root. The recommendation is (b): one rule, nothing implicit, and the root's own foreign code is listed
beside its ceiling entry for the library.

- Decision: (lead, card, 2026-11-16: root-gated)
- Row:

### D2: where it is enforced

Two places, one rule, one diagnostic:

1. Compiling the root package (`yan check`, `run`, `build`, `test`): a module in an `ffi` directory of
   the root, or any extern block, with the root absent from its own `allow_ffi`.
2. Resolving dependencies (`yan lock`, `add`, `update`, and a locked build): a chosen package whose
   computed `.info` (19.2) lists an `ffi` module or extern library and is not named in the root's
   `allow_ffi`. An update that gains the first `ffi` module of a package is refused the same way, so a
   dependency cannot acquire foreign code silently.

The diagnostic is an error; it names the package and the first `ffi` module or library, and the fix
(add the package to `allow_ffi`). It is not suppressible. It is new: no existing code covers consent
(E314 is the ceiling, E902 is placement).

Recommendation: both places, with the one new code in the E3 family.

- Decision:
- Row:

### D3: libraries stay ceiling entries

Consent does not replace the ceiling. Every extern library, `uses none` or not, is also a capability in
the package ceiling (E314, 7.4, 12.2), and a ceiling reaches dependencies transitively (19.1). Code for
the per-package part landed in #1411; the transitive part is part of this RFC's implementation.

- Decision:
- Row:

## Spec changes

12 (the `allow_ffi` sentence and 12.2), 19.1 (`allow_ffi` row), 19.2 (resolution check), 25 (the new
code).

## Conformance cases

One `pkg/` case per place in D2 (consent present, consent missing for the root, consent missing for a
dependency, update that gains an `ffi` module) on a `conform/*` branch, in the same PR set as the rows.

## Implementation (after acceptance)

Parse `allow_ffi` into the manifest (`yan-elab`), the root check in the package compile path, the
resolution check in `yan-cli` lock and deps, the crate symbol and catalogue entry, the cases above.
Closes #1389.
