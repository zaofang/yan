# Scientific computing: what ships with Yan and what does not

Study and proposal, 2026-11-05. Docs only: no spec rows, no conformance edits, no diagnostic codes.
Items are labelled D1, D2 and so on; none carries a row number. Everything here is non-normative until
an RFC and accepted rows say otherwise.

Brief (lead, 2026-11-05): how Yan gets the capabilities of pandas and polars, numpy, jax and numba,
scikit-learn, statsmodels, and matplotlib, plotly and altair, under Yan's own laws (determinism,
capability safety, concurrency, one core and many surfaces, Yan Agent written by models, the
Python-fidelity policy), and what is batteries included versus a library.

## 1. Answer

Most of the hard part is already decided. The spec has typed labelled arrays (11.2), an Arrow table
with an eager deterministic engine (11.8, 11.9), lazy plans in `@yan/frame`, correctly rounded libm
with determinism tiers (11.1), fixed-tree reductions, `grad` (11.5, R614), `math.ein` (R263), sparse
formats (R615), tile kernels on a `gpu` capability with bit-identical results (R327 to R329), block
`with fast_math:` (R821, R822), counter-based seeded `rand` with saved state (R683 to R685, R823,
R824), and a `py` bridge over the Arrow C Data Interface (17.8). What is missing is the layer people
actually call: statistics, models, plots, optimizers, and the glue that lets a model write
`df.groupby(...)` or `np.mean(x, axis=0)` and get a checked Yan program.

The recommended split, in one line per tier:

- **Compiler (Core transforms).** Whatever must see the program: fusion, `ein`, `grad` and a new
  `vmap` (D3), tile kernels, stage-0 specialization of plans and formulas. There is no JIT, because
  release builds are already ahead-of-time specialized and 11.9 forbids run-time code generation.
- **std.** The shared vocabulary every data program touches and whose definition is frozen:
  `Array`, `Table`, Arrow, CSV, libm, reductions, `rand`, units, decimal, plus a small `std.stats` of
  descriptive statistics with pinned definitions (D5) and a `DynArray` for untyped boundaries (D1).
- **First-party `@yan/*`.** Anything with an evolving algorithm or target: `@yan/frame` (exists),
  `@yan/parquet` (exists), `@yan/linalg`, `@yan/blas` (exists), `@yan/fft`, `@yan/stats`,
  `@yan/learn`, `@yan/optim`, `@yan/ode`, `@yan/plot`, `@yan/rand_distr` (exists), `@yan/prob`
  (exists). Each carries its own semver and ceiling and runs in the ecosystem runs (20.7), which is
  where the determinism goldens catch regressions.
- **Community.** Deep-learning frameworks, gradient boosting, domain science (bio, geo, finance,
  astronomy), dashboards. Until they exist, the `py` bridge reaches the Python originals under `uses
  py` with zero-copy tables and arrays (D11).

## 2. What already exists

| Python library | Yan home today | Rows | Gap |
| --- | --- | --- | --- |
| numpy ndarray | `array`, `Array[axes..., T]`, strided views, named broadcasting, fusion per statement | 11.2, R28 to R37 | no dynamic dtype or rank (D1); no `np.` spellings (D12) |
| numpy einsum, BLAS | `math.ein`, `@yan/blas` | R263, R615, 17.1 | no LAPACK-class `linalg` package named in 17.1 (D8) |
| polars, pandas | `Table[R]` eager in std, `LazyFrame[R]` and `LazyFrame[Dyn]` in `@yan/frame`, windows, incremental views | 11.9, R553, R625 | no pandas-shaped index; statement fusion in eager std (D2) |
| pyarrow | `std.arrow`, `@yan/parquet`, `@yan/adbc`, `@yan/object_store` | 11.8, R38 to R48 | none for this study |
| numba | `par for` over `uses none` bodies, SIMD lanes, `@multiversion`, ahead-of-time release builds | 11.3, R622 to R624 | none: numba's job is done by the compiler |
| jax `grad` | `grad`, `jvp`, `vjp`, `jacobian`, `hessian`, `@derivative` | 11.5, R614 | no `vmap` (D3); no checkpointing policy (D3) |
| jax `jit`, CUDA, Triton | `@kernel` tiles on `gpu`, `DeviceArray`, MLIR lowering, bit-identical strict mode | R327 to R329 | tensor-core speed needs `nondet` today (D4) |
| jax `random` | ChaCha8 by task path, `rand.state` | R683 to R685, R823, R824 | no per-index stream for batched draws (D6) |
| scipy.special, stats.distributions | `math` (C23 list), `@yan/rand_distr` | 11.1, 17.1 | no densities, CDFs, quantile functions (D7) |
| statsmodels, scipy.stats | nothing | | D5, D7 |
| scikit-learn | nothing | | D9 |
| scipy.optimize, integrate | nothing in spec (math design named Clarabel, HiGHS, Tsit5, Rodas5P) | | D8 |
| matplotlib, plotly, altair | nothing | | D10 |
| PyMC, NumPyro | Yan Probabilistic `.ypb` over `@yan/prob` | R546 | out of scope here |
| Jupyter | `yan nb` reactive notebook at M3 | 18.1 | display capability (D10) |

## 3. The placement rule, applied

17.1 puts a module in std only if it is shared vocabulary, a capability handler or needed by the M1
evaluation; if its target is frozen; and if one implementation beats many. Applied to this domain:

- **Containers and layout pass all three.** Every data package exchanges `Array`, `Table` and Arrow
  buffers, the Arrow format is frozen, and two incompatible array types would split the ecosystem the
  way numpy, torch and jax tensors split Python's. They stay in std.
- **Descriptive statistics pass, if their definitions are pinned.** `mean`, `var`, `quantile` and
  `corr` are vocabulary; their definitions are frozen once the variant is named (`ddof`, the quantile
  method). One implementation beats many because the reduction order is part of the result. Models,
  tests and regressions fail test 2: their algorithms and defaults change across versions, as
  scikit-learn's `solver` defaults did.
- **Linear algebra, FFT and optimizers fail test 2.** Blocking, pivoting and planning evolve, and
  tier-R results are pinned by `math_edition`; a package carries its own edition more easily than std.
- **Plotting fails tests 1 and 2.** Charts are not shared vocabulary between packages, and the target
  (renderers, browser specs) moves.

A fourth test, specific to Yan Agent, decides names rather than placement: the API names models reach
for (numpy, pandas, polars, scikit-learn) should resolve, either as the real Yan name or as a
lenient-reader note, per the 2026-11-04 policy that no Python spelling ends in a bare E200 (D12).

## 4. Design topics

### 4.1 Array and table core types

The typed `Array` and `Table[R]` are right for code that is written once and checked. Two boundaries
need an untyped form, which Yan already accepts for plans (`LazyFrame[Dyn]`):

- **Loading.** `np.load`, a `.npy` file, an Arrow tensor or a `py` array arrives with a run-time dtype
  and shape. A `DynArray` (dtype tag, shape, strides, shared buffer) is the boundary type, and
  `x.typed[Array[Row: n, Col: m, f64]]()` checks once and returns the typed view or `fail
  ShapeMismatch`. Tables already have the equivalent: `yan schema gen` plus `fail SchemaMismatch`.
- **Exploration in notebooks.** `yan nb` cells want `Table[Dyn]` so a model can write `t["price"]`
  before a record type exists. `Table[Dyn]` columns are `DynArray`-like Arrow arrays; every
  operation is checked at run time and fails with `QueryError`. Promotion to `Table[R]` is the same
  `yan schema gen` path.

Neither type is an escape from shape checking in packages: a public function of a published package
that takes or returns `DynArray` or `Table[Dyn]` gets a lint, so the dynamic forms stay at edges.

pandas' row index is not adopted. Polars dropped it for good reasons (alignment surprises, ambiguous
`loc`/`iloc`), and labelled `Array` axes already give coordinate indexing where it is meaningful,
including `pivot` into a labelled array (11.9). The lenient reader maps `reset_index()` to nothing
and `set_index(c)` to a note pointing at `pivot` or `index_by` (D12).

### 4.2 Eager and lazy

11.9 keeps the eager engine in std and lazy plans in `@yan/frame`. That split stays right: eager is
what models write (pandas style), it is easy to replay, and the optimizer is the part that evolves.
One gap is worth closing in std. A chain `t.filter(p).select(cs).with_columns(e)` in one statement
materializes three tables. 11.2 already requires statement fusion for array expressions; the same
rule for table chains (fuse projections and filters inside one statement, no reordering, no cost
model, no join reordering) gets most of polars' eager-to-lazy win with no optimizer in std (D2).
Results are identical to the unfused chain by construction, so determinism is unaffected.

### 4.3 What is a compiler feature

- **JIT (numba, `jax.jit`).** Not adopted. Release builds monomorphize and specialize at stage 0;
  `par for` over `uses none` bodies vectorizes and parallelizes; `LazyFrame[R]` plans with a constant
  shape are compiled by partial evaluation. A run-time JIT would break 11.9's rule that no program
  generates machine code at run time, and with it replay and audit. Debug builds interpret, which is
  what a notebook needs.
- **`grad`.** Already a Core transform (R614), correctly placed: a library cannot see through user
  functions, and the derivative rule per primitive is the only extension point.
- **`vmap`.** Should join `grad` as a Core transform with the same shape: each primitive gets a
  batching rule, `@batching(f)` defines a custom one, and the transform runs on checked Yan Core
  after monomorphization. Without it, per-example gradients and batched model evaluation are written
  as explicit axes, which is fine in Yan but not what models reach for from jax. With labelled axes,
  `vmap(f, over=Batch)` adds a labelled axis rather than axis 0, so broadcasting by name stays sound
  (D3). Checkpointing (`jax.checkpoint`) is a `@remat` attribute on a function inside a reverse-mode
  region, decided with reverse mode at M4.
- **Kernels and GPU.** R327 to R329 already make `@kernel` tiles and `DeviceArray` part of the
  language and std; vendor libraries (cuBLAS, cuDNN) belong behind `extern` in packages, at tier F.
- **Stage-0 DSLs.** Formulas (`"y ~ x1 + C(g)"`), `ein` equations and chart encodings are checked at
  stage 0 against record fields, as `rel.program` and `math.ein` are. This is the Yan answer to
  string-typed Python APIs: the string stays (models know it), the error moves to compile time.

### 4.4 Float determinism

The tiers of 11.1 carry over unchanged: E for libm and exact sums, R for every named algorithm with
its edition pinned in `yan.lock`, F only under `nondet` through `@fast_math`, `with fast_math:` or a
vendor binding. Three consequences for this domain:

- **Training and fitting are reproducible by default.** `sum`, `dot` and `matmul` use the fixed tree
  across threads, SIMD widths and devices, so `fit` gives the same coefficients on a laptop and a
  64-core server. This is the selling point against the Python stack, where `n_jobs`, BLAS threading
  and GPU atomics all leak into results.
- **Tensor cores are the hard case.** R328 puts TF32, BF16 and FP8 `dot` under `@fast_math`, so
  `nondet`. But a fixed-tile MMA without atomics or split-k is reproducible on one device family; it
  only differs across vendors and from the `cpu` oracle, whose emulation of each vendor's internal
  accumulation order is not guaranteed. A middle level, *handler-pinned* (bitwise reproducible given
  the handler and device generation recorded in the trace header and cache key, no `nondet`), would
  let reproducible training use tensor cores. It weakens the "same on every handler" promise for
  exactly the operations that opt in (D4).
- **Approximate libm on CPUs stays out.** R821 allows approximate math only inside the relaxed set;
  `@yan/learn` and `@yan/stats` should never require it, and their goldens run at tier R.

Randomness: ChaCha8 is counter-based, so a stream can be indexed. Batched and parallel code wants the
draw for example `i` to be the same whatever the batch size or thread count, which task-path splitting
alone does not give inside one task. `rand.at(i)` (a substream keyed by the parent stream and an
integer, O(1) to open) gives jax's `fold_in` with Yan's determinism, and `vmap` uses it to batch
random functions (D6). `rand.state` (R823) already covers checkpointing a training run.

### 4.5 Capabilities for data I/O and plotting

The existing model fits: computing is `uses none`, reading needs `fs`, `net` or a library capability,
spilling needs `tmp` (11.9), Python needs `py`. Two additions:

- **Packages that touch files take readers, not paths.** `@yan/parquet` and `@yan/plot` functions
  take a `Reader` or `Writer` (17.3) or return bytes, so the package ceiling stays `none` and only the
  root decides where data lives. This is a convention for first-party packages, not a new rule.
- **Showing a chart is a capability.** Building a chart and rendering it to SVG bytes are pure, given
  bundled fonts. Showing it (a notebook cell, a browser tab, a window) is an effect with no current
  capability. A `display` capability with handlers `nb` (bound by `yan nb`), `file(dir)`, `browser`
  and a test fake that records specs keeps `show()` out of pure code and makes notebook output
  replayable (D10).

### 4.6 Stats and ML API shape

The scikit-learn protocol is the vocabulary models know: `fit`, `predict`, `transform`, `score`,
pipelines, hyperparameters as constructor arguments. Keep the names, change three things:

- **`fit` returns a value.** `m = LinearRegression(alpha=0.1).fit(x, y)` returns a `Fitted` value
  of its own type; the spec record is unchanged. `predict` exists only on the fitted type, so
  predict-before-fit is a type error rather than `NotFittedError`. The lenient reader rewrites the
  Python statement `model.fit(x, y)` (result discarded, `model` used later) into `model =
  model.fit(x, y)` with a note (D12).
- **Effects are in the signature.** Stochastic estimators take `uses rand`; there is no
  `random_state` argument and no global seed. Parallelism is `sched` and deterministic, so `n_jobs`
  is accepted and ignored by the lenient reader with a note. Fitted models derive `Wire`, so they save
  as JSON, CBOR or a data-profile file; there is no pickle.
- **Inputs are typed tables.** Estimators take `Array[Row: n, Feature: k, f64]` or a `Table[R]` with
  a column selection, so a pipeline is a typed composition `Table[R] -> Table[S] -> Fitted`. A
  `ColumnTransformer` is a record of column-to-transform pairs checked against `R` at stage 0.

statsmodels' value is inference, not fitting: standard errors, tests, and the formula interface. In
`@yan/stats`, `ols("y ~ x1 + C(g)", t)` parses the formula at stage 0 against `t`'s record fields
(R's Wilkinson notation, patsy's spelling), and the result is a record with `params`, `bse`,
`pvalues`, `conf_int()` and a `summary()` that prints the familiar table. Distributions with `pdf`,
`cdf`, `ppf`, `logpdf` and `sample(uses rand)` live there too, sharing the special functions of
`math`.

    pub def fit_price(t: Table[Listing]) -> stats.OlsResult uses none:
        """Hedonic price model; the formula is checked against Listing's fields."""
        return stats.ols("log(price) ~ area + C(district)", t)

### 4.7 Plotting as declarative data

matplotlib's imperative state machine (`plt.plot`, then `plt.xlabel`, then `plt.show`) is hidden
mutable global state and the opposite of Yan. The altair and Vega-Lite model fits: a chart is a value.

- **The spec is a `Wire` record.** `Chart` is a grammar-of-graphics record (data, mark, encodings,
  scales, facets, layers) modelled on Vega-Lite 5. Because it derives `Wire`, it dumps to Vega-Lite
  JSON for browsers and plotly-style front ends, to the data profile (R826 to R829) for charts checked
  into a repository, and it diffs one line per change.
- **Encodings are checked.** `chart(t).mark_line().encode(x="date", y="price", color="district")`
  checks the field names and their types against `R` at stage 0 (temporal, quantitative, nominal
  inferred from the field type), the same way as formulas.
- **Rendering is pure and deterministic.** `@yan/plot` renders a `Chart` to SVG and PNG in Yan with
  bundled fonts and fixed float formatting, so a chart is snapshot-testable by its test hash (R300)
  and identical across machines. Interactivity (zoom, hover, selections) is delegated to the browser
  through the Vega-Lite export, not reimplemented.
- **A matplotlib-shaped builder for models.** Models write `plt.plot(x, y)`. A small `plt` module in
  `@yan/plot` with `figure()`, `plot`, `scatter`, `bar`, `hist`, `xlabel` and `title` as methods on a
  figure value (not global state) builds the same `Chart`, so the lenient reader only needs to turn
  `plt.` calls into calls on one figure value (D10, D12).

### 4.8 FFI to existing libraries

- **Port or bind by tier.** Code whose results must be tier R is ported to Yan (faer-class linalg,
  FFTW-style codelets with deterministic planning, Clarabel), following the math design. Code used
  for speed at tier F is bound through `lib` capabilities (12.2): OpenBLAS or Accelerate in
  `@yan/blas`, HiGHS, cuBLAS. A tier-F binding is used under `nondet`, or under a reproducible vendor
  mode (MKL CNR) declared by the binding and checked against goldens in ecosystem runs.
- **Native libraries ship with the toolchain.** The packaging decisions (RFC 0020) keep packages
  C-only and bundle first-party native libraries; the numeric bindings follow that.
- **Zero-copy interchange.** Tables cross through the Arrow C Data and C Stream interfaces (11.8).
  Arrays need the equivalent: DLPack, the protocol numpy, torch, jax and cupy share. `DynArray` gets
  `from_dlpack` and `to_dlpack` in std, and the `py` bridge gets `to_array` and `from_array` beside
  `to_table` and `from_table` (D11). An array viewed from Python is a read-only `SharedArray`, as Arrow
  imports already are.

### 4.9 Packages and the registry

The first-party list in 17.1 grows by `@yan/linalg`, `@yan/fft`, `@yan/stats`, `@yan/learn`,
`@yan/optim`, `@yan/ode` and `@yan/plot` (D8). Each is a single maintainer's burden, so the order
matters more than the list: stats and plot first (they make the data story usable), then linalg and
optim (they unblock learn), then learn, then ode and fft. Each package declares its `math_edition`
range, so a tier-R algorithm change is a semver-major bump rather than a silent result change.
Ecosystem runs (20.7) carry golden outputs for every first-party numeric package, on every `gpu`
handler where it applies.

### 4.10 Deferred until after M1

M1 ships the interpreter and the M1 std row (math, csv, rand, and the rest of 23). Nothing in this
study blocks it. The order after M1:

- **M2 (full std).** `DynArray` and DLPack (D1, D11), statement fusion for table chains (D2),
  `std.stats` (D5), `rand.at` (D6). These touch std and are cheaper to land with it.
- **Between M2 and M3.** `@yan/stats` and `@yan/plot` with the SVG renderer; `@yan/linalg`. The
  `display` capability lands with `yan nb` at M3 (D10).
- **M4.** `vmap` beside reverse-mode `grad` (D3), handler-pinned tensor cores with the GPU stretch
  (D4), `@yan/learn`, `@yan/optim`, `@yan/ode`, `@yan/fft`.
- **After 1.0.** Deep-learning frameworks, distributed arrays over `uses cluster`, sparse autodiff.

## 5. Recommended split

| Tier | Contents | Why here |
| --- | --- | --- |
| Compiler and Core | statement fusion (arrays and table chains), `ein`, `grad` family, `vmap`, `@kernel` tiles, stage-0 DSL checking (formulas, encodings), `@multiversion` | needs to see user code; one implementation |
| std | `Array`, `DynArray`, `Table[R]`, `Table[Dyn]`, Arrow, CSV, `math`, reductions, `rand` with `rand.at`, units, decimal, `std.stats` descriptive set, DLPack, `display` capability trait | shared vocabulary, frozen definitions, determinism needs one implementation |
| First-party `@yan/*` | `frame`, `parquet`, `blas`, `linalg`, `fft`, `stats`, `learn`, `optim`, `ode`, `plot`, `rand_distr`, `prob` | evolving algorithms and targets, own `math_edition` and semver, goldens in ecosystem runs |
| Community | deep learning, boosting, domain science, dashboards, vendor-specific kernels | open-ended; `py` bridge until they exist |

## 6. Decisions, in the order to ask them

- **D1. Dynamic boundary types.** Add `DynArray` and `Table[Dyn]` to std, with checked promotion to
  typed forms and a lint on dynamic types in published public signatures. Recommend: yes.
- **D2. Statement fusion for table chains.** Fuse filters and projections within one statement in
  the std eager engine, no reordering, results identical to unfused. Recommend: yes.
- **D3. `vmap` as a Core transform.** Batching rules per primitive, `@batching(f)`, labelled
  `over=` axis, M4 with reverse mode; `@remat` decided then. Recommend: yes.
- **D4. Handler-pinned determinism for tensor cores.** A level between R and F: reproducible given
  the recorded handler and device generation, no `nondet`, opt-in per call. Options: adopt; keep
  R328 as is (tensor cores always `nondet`). Recommend: adopt, behind the M4 GPU stretch.
- **D5. `std.stats` descriptive set.** `mean`, `var(ddof=)`, `std`, `median`, `quantile(method=)`
  with numpy's linear method as default and all nine Hyndman-Fan methods named, `histogram`, `cov`,
  `corr`, `describe` for tables. Recommend: yes, at M2.
- **D6. Indexed random substreams.** `rand.at(i)` keyed by the parent stream and an integer.
  Recommend: yes, at M2.
- **D7. Distributions in `@yan/stats`, not `@yan/rand_distr`.** `rand_distr` keeps sampling only;
  densities, CDFs, quantile functions and fitting go in `@yan/stats`. Recommend: yes.
- **D8. First-party numeric packages.** Add `@yan/linalg`, `@yan/fft`, `@yan/stats`, `@yan/learn`,
  `@yan/optim`, `@yan/ode`, `@yan/plot` to 17.1 in the order of 4.9. Recommend: yes.
- **D9. Estimator protocol.** scikit-learn names; `fit` returns a fitted value of its own type; `uses
  rand` instead of `random_state`; `Wire` instead of pickle. Recommend: yes.
- **D10. Plotting.** `Chart` as a `Wire` record modelled on Vega-Lite, pure SVG and PNG rendering
  with bundled fonts, a value-based `plt` builder, and a std `display` capability for `show`.
  Options: Vega-Lite export only (no Yan renderer); full renderer. Recommend: as stated, renderer
  static only.
- **D11. Zero-copy arrays.** DLPack in std and `to_array`, `from_array` in the `py` bridge.
  Recommend: yes, at M2.
- **D12. Python data-API side table.** Extend the lenient reader's foreign-name table to numpy,
  pandas, polars, scikit-learn and matplotlib spellings models reach for (`np.mean(x, axis=0)`,
  `df.groupby`, discarded `fit` results, `plt.` calls, `n_jobs`, `random_state`), each a real
  function or an N-code rewrite per the fidelity policy, sized by an R567-style probe on data tasks.
  Recommend: yes, after D1, D9 and D10 fix the targets.
- **D13. No run-time JIT.** Record numba- and `jax.jit`-style run-time compilation as a non-goal
  (26.2), pointing at stage-0 specialization. Recommend: yes.

## 7. Risks

- **Scope.** Seven new first-party packages is years of work for one part-time maintainer with
  agents. Mitigation: the order in 4.9, and the `py` bridge as the honest fallback; ship stats and plot
  before anything else.
- **Speed against vendor stacks.** Strict tier-R kernels will lose to cuBLAS and MKL in the default
  mode, and benchmarks against PyTorch will be quoted in tier F. D4 narrows the gap; the remainder is
  the price of reproducibility and must be stated, not hidden.
- **Dynamic types leaking.** `DynArray` and `Table[Dyn]` are convenient enough that published
  packages may standardize on them. The lint in D1 and ecosystem-run checks are the guard.
- **Python name drift.** pandas 3, polars and scikit-learn rename APIs every year. The side table in
  D12 needs an owner and a probe to stay useful, or it decays into wrong rewrites.
- **Vega-Lite as a moving target.** Pinning the record to Vega-Lite 5 couples `@yan/plot` to its
  schema. A Yan-owned `Chart` with a versioned exporter is safer; this is why the spec type is Yan's
  and the JSON is an export.
- **Fonts and text layout.** Deterministic rendering needs bundled fonts, shaping and float
  formatting fixed; without them snapshot tests flake across machines.
- **Handler-pinned results.** D4 makes some results depend on device generation; caches and traces
  must key on it, or replay on a newer GPU silently diverges.
- **Formula and encoding DSLs.** Stage-0 checking of strings needs good spans inside the string, or
  errors there will be worse than Python's run-time `KeyError`.
