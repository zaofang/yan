# 0058: launching a `@kernel` function

- Status: accepted
- Research: RFC 0055 (D1 to D4); spec 17.1 (R327 to R329); issue #674
- Rows: R971 to R973

## Summary

RFC 0055 stage 3 runs a proved `@kernel` function on the interpreter and native. R953 says the launch
supplies the `TileOrigin` and user code cannot construct one, and R329 says launches are operations of
the `gpu` capability, whose `cpu` handler is always available. Nothing says how a program writes a
launch, how the grid of program instances is formed, or what happens when the output shape is not a
whole number of tiles. Without those, a proved kernel cannot be called and stays E017. This RFC fixes
the three. Host-side `Array` values (grid literals, R463; element reads, 11.2) need no decision and
are built in the same stage.

## Decisions

### D1: the launch surface

`gpu.launch(k, a, b)` runs the `@kernel` function `k` once per program instance and returns its
`out` array. The arguments are `k`'s parameters other than the `TileOrigin`, in order. It needs
`uses gpu`; the `cpu` handler is bound at `main` like the other default handlers, and runs the
instances on the host. The result type is `k`'s result type, and the call fails with `KernelFault`
(program id and span) when a bounds check in the body traps (R327).

Options: (a) `gpu.launch(k, args...)` as above; (b) a bare call `k(a, b)` that omits the
`TileOrigin` and launches implicitly, with no capability; (c) a `par for at in tiles(...)` loop that
hands the body a `TileOrigin`. (b) hides an I/O-handler operation behind a call and drops the `gpu`
capability that R329 requires; (c) lets user code hold a `TileOrigin`, which R953 forbids.
Recommended (a).

- Decision: (a), the lead, 2026-11-09 (card on the project thread)
- Row: R971

### D2: the grid and the write region

Program instances may run in any order and at once on a hardware handler, so two instances must
never write the same element. The grid is formed from the `out` region: on each axis the region
bounds, instances step by the extent it bounds (the tile size for `elementwise`, `rows(a)` and
`cols(a)`; `M` or `N` for `tile(M, N)`), with origins `0, s, 2s, ...`; on an axis the region leaves
free there is one instance, origin 0; `whole` on `out` is one instance. Write regions of distinct
instances are then disjoint, and the result is the same in every order, which R328 needs.

Options: (a) the grid follows the write region as above; (b) `out` must be `elementwise`, any other
write region is E284; (c) one instance per tile on every axis, run in row-major order, with later
writes winning. (c) is deterministic only on `cpu` and breaks R328 on hardware; (b) rejects
row-reduction kernels such as a row sum into a column. Recommended (a).

- Decision: (a), the lead, 2026-11-09 (card on the project thread)
- Row: R972

### D3: output shapes that are not whole tiles

The `out` sizes on axes the grid steps over must be positional literals divisible by the step.
Otherwise the launch is E521 (tile shape mismatch, 17.1) at compile time. An operand whose size is not
literal is still accepted; an access the checker refutes is E501 and any other out-of-bounds access
traps at run time as `KernelFault`.

Options: (a) E521 at the launch for a partial edge tile; (b) run partial edge tiles and let the
bounds check fault their out-of-range writes. Under (b) every kernel over a ragged shape faults unless
its body guards each access, and the prover does not read guards as facts. Recommended (a); a later
RFC can add guarded edge tiles.

- Decision: (a), the lead, 2026-11-09 (card on the project thread)
- Row: R973

## Spec changes

Section 17.1 (R327, R329) gains `gpu.launch`, the grid rule and the E521 condition; section 7.1's
capability table lists `launch` under `gpu`; the diagnostics catalogue's E521 meaning adds "output
shape not a whole number of tiles at a launch".
