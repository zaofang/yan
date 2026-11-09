# 0055: `@kernel` functions and E274 body verification

- Status: draft
- Research: RFC 0016 D3 (Fern, PLDI 2025); RFC 0052 (binding syntax); roadblocks study
  (https://claude.ai/artifact/728U7arS1GVNBzdWy5T3jq); issue #674
- Rows: filled in when accepted

## Summary

An `@access` claim on a function with a body is checked against the body, and a contradicting access
is E274. Only `extern` claims exist today (RFC 0052). Nothing checks a body because nothing can be
checked: there is no `@kernel` item, no tile-origin binding, and element access on an `Array` is
`E017` ("not supported by this toolchain yet"). The lead decided on 2026-11-09 that the tile model is
an affine index proof, else E274, with no run-time fallback. This RFC fixes the surface that proof
needs: how a kernel names its tile, which index expressions are provable, what each region means
relative to the tile, and how this meets the `Tile[M, N, T]` model of 17.1 (R327).

## Decisions

### D1: how a kernel names its tile

`@kernel(tile: M, N)` on a function declares the tile shape (positive integer literals or `Nat`
constants) and gives the body one implicit `int` binding per tile axis, `t0`, `t1`, the origin of the
program instance's output tile in the output array. The body reads and writes array elements
(`a[t0 + i, t1 + j]`), where `i` and `j` are loop variables bounded by the tile shape.

Options: (a) implicit origin bindings `t0`, `t1` (above); (b) an explicit leading parameter of a
`TileOrigin` record type, which is visible in the signature and in `yan audit` but adds a type and a
call-site rule; (c) only the R327 `Tile[M, N, T]` values (`load`, `store`), with no element access.
Recommended: (a). The origin is not an argument a caller supplies, so a parameter misrepresents it.
(c) cannot state or check an `@access` region because `load` takes whole tiles.

- Decision: (lead's answer, date)
- Row: (R number once accepted)

### D2: the provable index language

An index expression is provable when it is an affine form `sum(c_k * x_k) + c` over integer literals,
comptime constants, the origin bindings, and loop variables of `for` loops with affine literal
bounds. Anything else (a loaded value, a call, a product of two variables, a loop with data-dependent
bounds) is E274 at the index, naming why it is not affine. There is no run-time check fallback.

The proof reuses the 8.6 solver: facts are the loop bounds and the tile shape, the goal is the
region's containment inequality, decided by the same Fourier-Motzkin elimination. One solver, one
integer-linear representation (CLAUDE.md practice 12).

Options: (a) affine over the listed terms (above); (b) also `i / c` and `i % c` with literal `c`
(needed for blocked layouts, costs a divisibility case in the solver); (c) affine only, with (b) as
a later RFC. Recommended: (c) now, so the first implementation is complete for its language.

- Decision: (lead's answer, date)
- Row: (R number once accepted)

### D3: what each region means

For an operand with axes `a_0 ... a_{n-1}` and tile origin `t` and shape `T`, an access at index
vector `x` is inside the region when:

- `elementwise`: `x_k = t_k + i_k` with `0 <= i_k < T_k` for every axis (the operand has the tile's
  rank; a lower-rank operand is E274 at the claim).
- `rows(a)`: axis `a` lies in `[t_a, t_a + T_a)`; every other axis is any in-bounds index.
- `cols(a)`: the same region as `rows(a)`. The two spellings are kept because R627 already names both.
- `tile(M, N)`: both axes lie in a `M x N` block whose origin is `t` rounded down to a multiple of
  `M` and `N`. Needs the divisibility case of D2 (b), so it is E274 on every access until D2 (b).
- `whole`: any in-bounds index; no access can contradict it.

The `out` operand is the write region and the others are read regions. Reading outside a read region
or writing outside the write region is E274; an access in bounds of the array but outside the region
is E274, and out of bounds of the array is E501 as everywhere (8.6).

Options: (a) as above; (b) regions as sets of indices relative to the origin with a user-written
offset (`rows(0, halo: 1)`), which stencils need. Recommended: (a) now; stencil halos are a later RFC
and, until then, a stencil declares `whole`.

- Decision: (lead's answer, date)
- Row: (R number once accepted)

### D4: staging and the toolchain

Element read and write on `Array` is supported only inside `@kernel` bodies first, so no general
array semantics are decided by this RFC. Order: (1) parse and elaborate `@kernel`, tile bindings and
`Array` element access, interpreter result, with the `kernel` profile restrictions of R320 left to
their own work; (2) the affine prover and E274, with its crate symbol and catalogue entry (E274 is
already catalogued); (3) native lowering with the same program on interpreter and native (CLAUDE.md
practice 1). Conformance cases go on a `conform/*` PR, not here. The E274 catalogue entry stays.

Options: (a) the order above; (b) prover first on a synthetic IR, then the surface. Recommended: (a);
(b) would add an analysis with no input the toolchain can produce.

- Decision: (lead's answer, date)
- Row: (R number once accepted)

## Spec changes

Section 11.2 (access patterns paragraph) gains the tile bindings and the region meanings; section
17.1 (R320, R327) states that `@kernel` bodies may use element access under D1 to D3 beside
`Tile` values; the diagnostics catalogue keeps E274 and, if the lead accepts D2, adds the
"not affine" wording to its meaning.
