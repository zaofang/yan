# 0055: `@kernel` functions and E274 body verification

- Status: accepted
- Research: RFC 0016 D3 (Fern, PLDI 2025); RFC 0052 (binding syntax); roadblocks study; issue #674
- Rows: R953 to R955

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
constants). The function takes one parameter of type `TileOrigin`, a std record with one `int` field
per tile axis, `at.i0`, `at.i1`, the origin of the program instance's output tile in the output array.
The body reads and writes array elements (`a[at.i0 + i, at.i1 + j]`), where `i` and `j` are loop
variables bounded by the tile shape. The launch supplies the `TileOrigin`. It has no public constructor, so user code cannot call a
`@kernel` function: the call lacks an argument it cannot build.

Options: (a) implicit origin bindings; (b) an explicit `TileOrigin` parameter, visible in the
signature and in `yan audit`; (c) only the R327 `Tile[M, N, T]` values, with no element access.

- Decision: (b), the lead, 2026-11-09 (card on this thread)
- Row: R953

### D2: the provable index language

An index expression is provable when it is an affine form `sum(c_k * x_k) + c` over integer literals,
comptime constants, the `TileOrigin` fields, and loop variables of `for` loops with affine literal
bounds, and `e / c` or `e % c` of such a form with a positive integer literal `c`. Anything else (a loaded value, a call, a product of two variables, a divisor that is not a literal, a loop with data-dependent
bounds) is E274 at the index, naming why it is not affine. There is no run-time check fallback.

The proof reuses the 8.6 solver: facts are the loop bounds and the tile shape, the goal is the
region's containment inequality, decided by the same Fourier-Motzkin elimination. One solver, one
integer-linear representation (CLAUDE.md practice 12).

Options: (a) affine over the listed terms only, with `/` and `%` in a later RFC; (b) also `i / c` and
`i % c` with literal `c` (needed for blocked layouts and `tile(M, N)`, costs a divisibility case in
the solver). The index language of (b) is the accepted one.

- Decision: (b), affine plus `/` and `%` by a literal, the lead, 2026-11-09 (card on this thread)
- Row: R954

### D3: what each region means, with halos

For an operand with axes `a_0 ... a_{n-1}` and tile origin `t` (the `TileOrigin` fields) and shape `T`,
an access at index vector `x` is inside the region when:

- `elementwise`: `x_k = t_k + i_k` with `0 <= i_k < T_k` for every axis (the operand has the tile's
  rank; a lower-rank operand is E274 at the claim).
- `rows(a)`: axis `a` lies in `[t_a, t_a + T_a)`; every other axis is any in-bounds index.
- `cols(a)`: the same region as `rows(a)`. The two spellings are kept because R627 already names both.
- `tile(M, N)`: both axes lie in a `M x N` block whose origin is `t` rounded down to a multiple of
  `M` and `N`, which D2 provides for.
- `whole`: any in-bounds index; no access can contradict it.

`rows`, `cols` and `tile` take an optional trailing `halo: h`, `h` a non-negative integer literal. The
region widens by `h` elements on each side of every axis it bounds, so `rows(0, halo: 1)` is
`[t_0 - 1, t_0 + T_0 + 1)` on axis 0. A halo does not move an access past the array: an index outside
the array is E501 as everywhere (8.6). `elementwise` and `whole` take no halo (E284). This extends the
RFC 0052 grammar: `halo` is the one named argument a region accepts, and a negative, non-literal or
repeated halo is E284.

The `out` operand is the write region and the others are read regions. Reading outside a read region
or writing outside the write region is E274, and `out` takes no halo (a write halo would let two
tiles write the same element, which breaks determinism under `par`; E284).

Options: (a) regions without halos, a stencil declaring `whole`; (b) halo offsets as above.

- Decision: (b), the lead, 2026-11-09 (card on this thread)
- Row: R955

### D4: staging and the toolchain

The prover comes first, on a synthetic IR, then the surface. Order: (1) the affine prover and region
containment check over a small index IR (affine forms, `/` and `%` by literal, loop bounds, tile shape,
region, halo), with unit tests; it reuses the 8.6 `Lin` and solver, and E274 and its message are
produced from the IR; (2) `@kernel`, `TileOrigin` and `Array` element access in kernel bodies lowered
to that IR, interpreter first; (3) native lowering, with the same program on interpreter and native
(CLAUDE.md practice 1). Conformance cases go on a `conform/*` PR, not here.

Options: (a) surface, then prover, then native; (b) prover first on a synthetic IR. Recommended
(a): a prover with no producible input cannot be tested end to end.

- Decision: (b), the lead, 2026-11-09 (card on this thread), against the recommendation. The stage-1
  prover is reached only from its unit tests until stage 2 lands, and is not wired into any
  diagnostic until then.
- Row: none (process)

## Spec changes

Section 11.2 (access patterns paragraph) gains the tile bindings, the region meanings and the `halo` argument (with the RFC 0052 grammar in the Binding paragraph); section
17.1 (R320, R327) states that `@kernel` bodies may use element access under D1 to D3 beside
`Tile` values; the diagnostics catalogue keeps E274, adds the "not affine" wording to its meaning, and extends E284
to a malformed halo.
