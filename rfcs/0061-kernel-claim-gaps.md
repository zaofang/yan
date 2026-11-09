# 0061: `halo` on extern claims, and a code for a malformed `@kernel`

- Status: draft
- Research: RFC 0052 (D1), RFC 0055 (D1, D3), RFC 0058; issue #674 (gaps 5 and 6)
- Rows: filled in when accepted

## Summary

Two holes are left from #674. First, R955 lets `rows`, `cols` and `tile` take `halo: h` in any
`@access` claim, and a `@kernel` claim accepts it, but an `extern` claim rejects it as E284: the
claim is carried into Core as a `ConstVal::Call`, whose arguments are positional, and the Core grammar
of 3.x does not list a call form at all. Second, R953 fixes the form of a `@kernel` function (a
`tile:` shape of positive constants, one `TileOrigin` parameter, an `@access` claim) and says only a
launch calls a kernel or builds its origin, but no code is reserved for breaking those rules, so each
is E017 ("not supported by this toolchain yet") with a message. E017 says the program may become valid
later; these programs never will (Practice 13, fail loud).

## Decisions

### D1: how Core spells a region with a halo

Options:

- (a) Named arguments in a Core constant call: `rows(0, halo = 1)`. `ConstVal ::= ... | Path "("
  [AttrArg {"," AttrArg}] ")"`, reusing the existing `AttrArg ::= [Name "="] ConstVal`, so Core
  attributes and constant calls name arguments the same way. The claim reads in Core and in
  `yan audit` as it is written in source, and the call form, already produced since RFC 0052, enters
  the grammar.
- (b) A wrapper constructor: `halo(rows(0), 1)`. No named arguments, but Core then has a region name
  the surface lacks, and every reader of a claim unwraps it.
- (c) No halo on extern claims: narrow R955 so an extern stencil claims `whole` (or `rows` without the
  halo). No Core change, but the optimizer loses the region of every extern stencil, which is what
  the claim is for (R627: fuse without seeing the body).

Recommendation: (a). It keeps one representation of a region from source to Core, fixes the grammar
gap, and costs one field in `ConstVal::Call`.

- Decision:
- Row:

### D2: the code for a malformed or misused `@kernel`

Rules in question, all from R953 and RFC 0058:

1. The decorator is not `@kernel(tile: M, N, ...)` with positive integer constants.
2. The function has not exactly one `TileOrigin` parameter.
3. The function has no `@access` claim.
4. User code calls a `@kernel` function, or builds a `TileOrigin`, outside `gpu.launch`.

Options:

- (a) Two new codes: one for a malformed declaration (1 to 3), reported at the function, and one for
  a misuse (4), reported at the call or the record literal. They have different readers (the kernel's
  author and its caller) and different fixes, as E326 separates a handler built outside the platform
  from a malformed handler.
- (b) One new code for every R953 rule, the message naming which.
- (c) Widen E284 to "malformed `@access` or `@kernel` declaration", with misuse kept as E017 or folded
  in as well.

Recommendation: (a). A declaration error and a use error sit at different places in the program;
(c) mixes a claim error with a declaration error and leaves misuse unanswered.

- Decision:
- Row:

## Spec changes

- D1: 3 (`ConstVal` gains the call form with `AttrArg` arguments); 11.2 (R955: the halo holds on
  `extern` claims and is carried to Core as `halo = h`).
- D2: 11.2 (R953 names the code or codes); 25 (catalogue entries).
