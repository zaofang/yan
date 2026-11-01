## 10. Yong: generics, traits, compile time and generators

### 10.1 Generics

Functions and types take type, `Nat`, axis and dimension parameters, user-defined types included (`SMatrix[R, C, T]`, `Fixed[I, F]`). Bounds are traits. Generic code is type-checked once, generically, and monomorphized per instantiation. `dyn Trait` is the only runtime polymorphism: a value plus a vtable, opted into explicitly.

**Inference budget.** Inference is local and bidirectional, in the surfaces only. There is no overloading on return type, and literal typing is bounded. A case that would need exponential search is E241 asking for an annotation, never a slow compile.

**Named argument packs.** A parameter `**fields: Expr[_]` collects keyword arguments into an anonymous record whose type stage 0 can read, so a library function can return a type computed from argument names (typed `agg`, `select`, `with_columns`).

### 10.2 Traits

- A trait declares functions whose first parameter is `self`, of type `Self`. Default bodies are allowed.
- **Associated types.** A trait may declare type members, `type Item`, which each impl fixes once. There are no generic associated types and no associated constants; lending iterators use `from` (4.5). The iteration protocol is `trait Iterator { type Item; fn next(inout self) -> Option[Item] }`, and `for` desugars through `IntoIter`. Adapters are lazy with guaranteed fusion, and an exhausted iterator keeps returning `None`.
- **Coherence.** `impl Trait for Type` is legal only in the module that declares `Trait` or the one that declares `Type`. Overlapping impls are E220. There is no specialization.
- Trait resolution happens entirely at compile time.
- **Method resolution.** `x.f(...)` always resolves a function, by the order of section 17.2: the current module, then the type's module, then the prelude, first match wins. A function-typed field is called as `(x.f)(...)` (SQ8). Two functions of one name at one step are E250.

### 10.3 Operators

Surface operators resolve to trait functions: arithmetic to `Num` (and `Field` for `/`), `**` to `Pow`, `//` and `%` to `Num`, comparison to `Eq` and `Ord`, `@` to `MatMul`, `& | ^ ~ << >>` on integer types to `Bits`, and `+` on strings and lists to `Concat`. T \* S, S \* T and T / S for a scalar S resolve to Scale\[S\], which std implements for Quantity, Array, Vec and Duration by integers, and which leaves Num for rings and fields (R439). User types may implement these traits, and only these. No other operator overloading exists.

Indexing a user type, `x[k]` and `x[k] = v`, resolves to `Index[K]`, whose `at` is a subscript (4.4, R457).

### 10.4 Compile time (stage 0)

**Code generation** is only specialization of ordinary functions on stage-0 values: `comptime` parameters, `comptime for` (unrolls over a stage-0 sequence) and `comptime if` (checks only the taken branch). There are no quotes, splices, AST values, token streams or syntactic macros in Yong, Jian or Wen. A plain `if` on a stage-0 condition is folded with both branches checked. Every function with a compile-time parameter is monomorphized per distinct stage-0 argument. Its body is checked in two phases (R346): code that does not depend on stage-0 values is type-checked once, generically, at the definition; code that does (a type computed by a stage-0 call, or a branch of an if on a stage-0 condition) is checked at each instantiation, and an error there is E272, reported with the instantiation site and the stage-0 backtrace. Embedded DSLs are `comptime` string parameters parsed at stage 0, with diagnostics placed inside the literal.

**Stage-0 language.** Stage 0 runs checked Yong on the abstract machine (6.1) with target semantics (width, endianness, layout), strict IEEE floats and no `@fast_math`. It rejects `unsafe`, `Ptr`, `extern`, `asm`, atomics and `Vec` intrinsics (E268); `par` runs under `sequential`. A stage-0 panic is E260, with a stage-0 backtrace and the use site.

**Limits.** Deterministic per-evaluation budgets: 50 million stage-0 steps (R487), 1 GiB heap, call depth 1,000 (E261 when exceeded). `@limit(stage0_steps: n, stage0_heap: n, stage0_depth: n)` raises them for one item. There is no wall-clock limit.

**Stage-0 steps (R487).** A stage-0 step is one executed call, one loop iteration or one allocation of checked Yong, counted the same by every evaluator. The step budget, `@limit(stage0_steps: n)` and `--stage0-profile` count these steps, so a compiled stage-0 evaluator raises E261 at the same point as the bytecode interpreter.

**Capabilities.** Stage 0 gets only `alloc` (a stage-0 arena), `embed(path)` restricted to the package root with its content hash recorded (E267 outside it), and `diag`: `compile_error` (E270) and `compile_warn` (W816), optionally at a span inside a string-literal argument. Stage 0 is therefore hermetic and deterministic.

**Materialization.** Only materializable values cross into run time (E262 otherwise): scalars, `Str`, `Bytes`, arrays, `List`, `Map`, `Set`, and records, sums or tuples of them, emitted once as read-only data. A materialized Map or Set carries an index built at compile time with a stable hash, independent of the run-time hash seed (R346). A `Type` crosses only as a monomorphization argument. `Rc`, `Arc`, function values and handlers do not cross.

**Closure-free.** A function value passed as a compile-time parameter is always specialized into its caller and never exists at run time as a closure or indirect call (E263 otherwise).

**Type functions.** A function returning `Type` is pure and memoized; it can be used in type position, and type identity is (function, stage-0 argument hash). New nominal types come from `record_type`, `sum_type` and `newtype`, which take reflection records and are checked like declared types.

**Reflection.** `info(T)` returns a non-exhaustive `TypeInfo` sum (`Record`, `Sum`, `Newtype`, `Scalar`, `Array`, `View`, `Fn`, `Trait` and so on, with annotations and layout). `info(f)` gives a signature, parameter names, `uses`, `fail` and doc string, never a body. `info(module)` lists items. `implements(T, Tr)` tests conformance, including the auto traits read-only. `x.(f)` accesses a reflected field. Reflection is access-checked at the call site (E264).

**Derives.** A trait may define one `derived` body over reflection. `@derive(Tr)` instantiates it for the type with the defining module's access, then specializes it. `Eq`, `Ord`, `Hash`, `Show`, `Default`, `Wire` and `Num` (for newtypes over numeric types) are std traits with `derived` bodies. Deriving a trait without a `derived` body, or an auto trait, is E269.

**Attributes.** A user attribute is either a typed annotation (an inert materializable value read through reflection) or an `@attribute` function `Fn(DeclInfo) -> List[Decl]` that may only add impls, reified types and const items, which are then checked like any others. It never rewrites or removes the declaration and cannot emit extern items, exports or built-in attributes (E266).

**Hygiene.** Names in `derived` bodies and attribute functions resolve where they are written. Reified names are collision-checked (E265). Generated items have stable paths (`Point.derived.Eq`); `yong expand` and the MCP `query` tool print them.

**Incremental evaluation.** Each stage-0 call is a query keyed by the callee body hash, argument hashes, embedded file hashes and the hash of every `TypeInfo` field read, with early cutoff and local and remote caching. The 1.0 evaluator is a bytecode interpreter; `yong check --stage0-profile` reports steps and cache hits.

**Surfaces.** Yong spells stage 0 `comptime`; Wen writes `#for`, `#if` and `#expr` (L5). Jian has compile-time parameters in generic brackets and may apply derives, annotations and attribute functions, but cannot define reflective code.

### 10.5 Generators

Generators are the only control handlers. A generator (Wen L5 `gen name(params) -> T = body`, Yong `gen`) yields `T` with `yield e` and ends with `return`. Calling it returns an `Iterator` with `Item = T`, second-class when it borrows a parameter. It compiles to a state machine and yields only from its own body (E141 for `yield` elsewhere). There are no general one-shot continuations, no fiber stacks, and no `effect`, `handle` or `resume`; user capabilities are `@capability` traits bound with `with`. Jian MUST NOT declare generators. A generator cannot fail: Iterator.next has no failure type, so a fallible step yields a Result (R379).

### 10.6 Built-in attributes

| Attribute | Effect | Layer |
| --- | --- | --- |
| `@derive(T, ...)` | instantiate `derived` bodies | L1 |
| `@layout(...)` | fix layout (5.2) | L4 |
| `@export`, `@export(name=...)` | export with the C profile (12.7) | L0 |
| `@weak` | weak import, gives `Option[fn ...]` (12.8) | L6 |
| `@capability` | the trait may appear in `uses` | L2 |
| `@splittable` | capability splits per `par` branch | L3 |
| `@shared` | capability is used by concurrent branches at once, by let access (7.1) | L3 |
| `@linear` | values must be consumed exactly once by a `sink` function such as `close` (E412), except on a path already propagating a failure, where the handler's release intrinsic frees them (R443) | L4 |
| `@non_exhaustive` | outside the package, matches need a wildcard and literals and destructuring need `..` (20.6) | L1 |
| `@prove` | module (written @@prove at the top of the file, R466) or function uses the `prove` level | Contracts |
| `@forbid(rc, alloc, copy, panic)`, any subset (R447) | proof obligations (5.7) | L4 |
| `@inline`, `@cold` | optimization hints | L1 |
| `@unordered`, `@fast_math` | relax determinism; require `uses nondet` | L3 |
| `nonblocking` | marks a foreign call that does not block; every other foreign call blocks (9.7, R447) | L6 |
| `@reentrant` | actor handler may interleave (9.6) | L3 |
| `@explore` | run the test under `explore` (6.9) | L3 |
| `@limit(name: n, ...)` | raise a hard limit for one item | L1 |
| @multiversion(target, ...) | compile once per target, select at process start (11.3, R386) | L4 |
| `@expect(code, reason="...")` | the only lint suppression; W810 if it suppresses nothing | L0 |
| `@deprecated(since, note)` | warn at uses (W815); `yong fix` inlines forwarders (20.5) | L0 |
| `@preview("name")` | unstable; use needs opt-in (20.6) | L0 |
| `@since("1.x")` | std API version (20.3) | std |
| `@derivative(f)` | custom differentiation rule (11.5) | L1 |
| `@attribute` | the function is an attribute function | L5 |
| other std attributes | @kernel, @interrupt, @task, @stack (17.1); @cases, @test\_support, @trace, @schedule (18.4); @plan (11.9); @messages, @mf2\_function (17.9); @diagnostic, @trusted\_input, @wire, @cli, @durable, @remote and @fip are each defined in the section that introduces them (R377) | as defined there |
