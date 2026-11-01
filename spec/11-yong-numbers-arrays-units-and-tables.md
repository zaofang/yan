## 11. Yong: numbers, arrays, units and tables

Shape and unit errors are type errors. All shape-label and unit information MUST be erased before code generation.

### 11.1 Floating point and the math library

- **No floating-point environment.** There is no global rounding mode and there are no exception flags. Directed rounding is an explicit argument: libm functions and `add`, `sub`, `mul`, `div`, `sqrt` and `fma` take `round: Round = NearestEven()`; a constant argument is specialized at compile time.
- **libm scope.** The bundled libm covers the full C23 7.12 list and is correctly rounded in all four rounding modes for `f16`, `bf16`, `f32` and `f64`. `f16` and `bf16` results are correctly rounded directly, not by way of `f32`. Arithmetic on `f16` and `bf16` rounds after each operation. `f128` is software (section 3.2).
- **Determinism tiers.** Every math API declares a tier. **E**: correctly rounded. **R**: a specified algorithm, bitwise reproducible across platforms, threads, SIMD widths and backends; tier-R algorithms are pinned by `math_edition` in `yong.lock`. **F**: opt-in, through `@fast_math`, `@unordered` or vendor bindings, under `nondet`.
- **Reductions.** `sum` and `dot` use the reduction order of 9.3 (R382). `sum_exact` and `dot_exact` are correctly rounded (superaccumulator). `matmul` accumulates sequential fma chains over a fixed `kc` and splits k only at nodes of a fixed tree.
- **FMA baseline.** Release targets require hardware FMA (x86-64-v3, ARMv8). Other targets use a correctly rounded software fma.
- **Random numbers (R377).** The default seeded generator is ChaCha8 by task path (7.7): `rand = seeded(seed)` gives every task its own stream, so `main` and each `par` branch draw deterministically. `seeded(Pcg64Dxsm, seed)` must be named, gives one stream and is not splittable, so using it in two branches that may run at once is E402. Both algorithms are versioned so seeded streams never change.

### 11.2 Array types

`Array[a1, ..., an, T]` lists axes, then a numeric element type. Each axis is positional (a `Nat`: literal or size variable) or labelled (any type whose values are coordinates, such as `PlainDate` or `Str`).

**Shape checking.**

- Labelled axes match as a set keyed by label; their written order is irrelevant to checking but fixes the memory order.
- Positional axes match as an ordered list and must agree exactly.
- Size variables are solved by unification. Any not solved statically are checked once, at entry to the function that introduces them (panic `shape`).

**Broadcasting by name.** For elementwise operations, the result's labelled axes are the union of the operands' labelled axes, and each operand is broadcast along axes it lacks. Positional axes broadcast only against scalars (E520 otherwise). `positional(x)` drops labels in memory order; `label(x, A, B, ...)` attaches them.

**Indexing.** `x[A: i]` selects along labelled axis `A` by position (integer) or by coordinate (a value of type `A`). Positional axes index by integer. Ranges `a..b` slice. Slices, transposes and broadcasts are strided views (4.5) and never alias mutable data. Indexing follows the rule for undecided goals (8.6): a refuted index is E501, and an index that cannot be proven compiles to a bounds check (with lint W820 in Xin).

**Representation.** `Buffer` pointer, shape and strides, row-major by default. Coordinates of a labelled axis are stored once as an index array beside the buffer.

**Fusion.** Within one statement, elementwise operations and the reductions consuming them MUST be fused into a single loop nest. Loops whose body has no capabilities SHOULD be vectorized.

**Index equations (R263).** `math.ein(eq, operands...)` takes a stage-0 string such as `"C[i, k] = A[i, j] * B[j, k]"`, checks it against the operands' axes, and emits a loop nest, or a call to a matching kernel (GEMM, GEMV, batched dot product, transpose). An index that appears only on the right is summed over, and `max_j`, `min_j` and `prod_j` name the other reductions. Index letters bind labelled axes, and a label mismatch is a stage-0 diagnostic. Sizes the solver cannot prove equal add one `LengthMismatch` check before the loops. `into: c` writes into a destination; otherwise the result allocates, with a `try_ein` sibling. Loop order and tiling are chosen at stage 0 and can be overridden with `@schedule`. Reduction order is fixed, and `par: true` splits only over non-reduced indices, so results are deterministic.

### 11.3 SIMD

`Vec[N, S]` is a fixed-width SIMD vector of scalar `S` in `std.simd` (R386). Lane arithmetic, comparisons, masks, `select`, lane reductions, constant shuffles, and loads and stores through a `View` or `MutView` with a checked base are safe and belong to L4 (Wen only). Target-specific intrinsics stay L6 and legal only inside `unsafe`. None is available at stage 0. `@multiversion("x86-64-v4", "x86-64-v3", ...)` compiles a function once per listed target and selects one at process start; all versions MUST give identical results, which strict IEEE lane operations guarantee, and an approximate intrinsic needs `nondet`. W821 flags a `@multiversion` function whose versions compile to the same code. Portable code may still rely on fusion and vectorization (11.2).

### 11.4 Units

`Quantity[T, D]` pairs a numeric type with a dimension.

- A dimension is a rational exponent vector over base dimensions: `length`, `mass`, `time`, `current`, `temperature`, `amount`, `luminosity`, `information` (bytes), plus one per declared currency.
- A unit is a dimension plus a compile-time exact rational scale. Units MUST be declared (`unit km = 1000 * m`); std declares SI units, data sizes, `pct` (0.01), `bp` (0.0001) and durations from `ns` to `day`.
- Addition and comparison require equal dimensions (E530). Multiplication and division combine dimensions. Conversion between units of one dimension is explicit (`to(x, km_per_h)`); the scale factor is folded at compile time.
- `QuantityPoint` is an affine quantity (temperatures, positions): point minus point is a quantity, point plus quantity is a point, and point plus point is E530.
- Quantity kinds distinguish quantities of one dimension that must not mix (torque and energy).
- Each currency is its own base dimension; converting between currencies requires an explicit rate of type `Quantity[float, eur / usd]`.

### 11.5 Automatic differentiation

`grad`, `jvp`, `vjp`, `jacobian` and `hessian` apply to pure functions (no capability besides `sched`); differentiating an impure function is E319. `@derivative(f)` defines a custom rule. Forward mode ships first as a dual-number library; reverse mode is a transform on Yong IR after monomorphization and inlining (Enzyme's algorithm, no Enzyme dependency). The intrinsics are an M4 preview.

### 11.6 Time

`Instant`, `Duration` and `MonoInstant` are i128 nanoseconds; `Duration` is `Quantity[i128, ns]` and exact. Until units are implemented, `Duration` is an opaque type over i128 nanoseconds with the API of 17.5, which it keeps as a `Quantity` (R593). Every time type covers years -9999 to 9999; leaving the range is overflow and panics with kind `overflow` (operators panic, `checked_` forms return None). `Period` holds only years, months, weeks and days with one sign; calendar differences return `(Period, Duration)`; `Instant + Period` and `PlainDate + Duration` are type errors. `ZonedDateTime` has structural equality (instant and zone) and no ordering; order by `.instant`. Bundled zones and `ZonedDateTime` are in `core`. The full API is section 17.5.

### 11.7 Decimal and money

`dec128` is IEEE 754-2008 decimal128. Rounding defaults to half-even, and division and `quantize` take an explicit `RoundingMode`. Decimal literals are exact, and there is no implicit conversion between floats and decimals. `Money` is `Quantity[dec128, Currency]`: amounts in different currencies do not add, and conversion takes an explicit rate. ISO 4217 minor units come from a table pinned in `yong.lock`. The columnar fixed-point type is `Decimal(p, s)` (11.8).

### 11.8 Columnar memory

`alloc` owns `Buffer` and `Bitmap`, shared with `Array`. `std.arrow` is the Arrow columnar format with view, dictionary and run-end encodings and the C Data and C Stream interfaces. `StringView` is the default string column. A field typed `T | None` is a nullable column; other fields carry no validity bitmap, and `is_null` on a non-optional column is E544. Arrow `Timestamp` columns that Yong creates default to microseconds; nanoseconds are opt-in; reads keep the file's unit.

### 11.9 Tables

`Table[R]` stores record type `R` column by column in the Arrow layout. `List[R]` stores the same records row by row, with the same operations as eager functions.

- **std engine.** `Table[R]` operations (`filter`, `select`, `with_columns`, `group_by`, `agg`, `join`, `pivot`, `sort`) are eager vectorized kernels, push-based and morsel-driven (2048-row vectors, 65536-row morsels), executed under `sched`. `pivot` produces a labelled `Array` whose axes are the types of the pivot columns. Lazy plans, the optimizer, streaming, SQL and `explain` live in `@yong/frame`, which adds `.lazy()` through the dot form and the typed `LazyFrame[R]` and `LazyFrame[Dyn]` plans.
- **Deterministic queries.** Query results, including float aggregates and row order, MUST NOT depend on thread count, ISA or memory budget. Morsel boundaries depend only on the input, partials merge in index order, `group_by` emits first-appearance order and `sort` is stable. `@unordered` relaxes order only.
- **Query arithmetic.** Integer overflow and failed strict casts inside a query end it with `fail QueryError`. Wrapping, saturating and widening forms are explicit expressions.
- **Query compilation.** A `LazyFrame[R]` plan whose shape is a stage-0 constant is compiled in release builds by specializing the tier-1 executor on it (first Futamura projection), so its results equal tier-1 execution bit for bit by construction; debug builds interpret it. `LazyFrame[Dyn]` and run-time plans stay on tier 1. No Yong program generates machine code at run time.
- **Limits and windows (R553).** `@yong/frame` adds `head(n)` and window expressions `e.over(partition_by=[...], order_by=[...])` with `row_number`, `rank`, `dense_rank`, `lag(e, k)`, `lead(e, k)`, `cum_sum`, `cum_count`, `cum_min` and `cum_max`. Order within a partition is the stable sort, ties keep input order, and the determinism rule above applies. Top N per group is `filter(row_number().over(...) <= n)`.
- **Spilling.** Query spilling uses the `tmp` capability and nothing else.
- **Schema-generated tables.** `yong schema gen data.parquet` writes the row record type as ordinary Xin source plus the schema's hash. The output is checked in, and compilation never reads data files or the network to resolve a type. At run time, a file whose Arrow schema differs from the generated one is `fail SchemaMismatch`, carrying a column-level diff. Parquet I/O lives in `@yong/parquet`; std keeps `read_csv`.
