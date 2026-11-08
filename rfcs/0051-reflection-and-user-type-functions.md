# 0051: Reflection and user type functions

- Status: accepted
- Research: none; source reading of `crates/yan-elab/src/typefn.rs`, `crates/yan-elab/src/stage0.rs` and
  `crates/yan-interp/src/lib.rs` (`Value`, `stage0`), against 10.4 (R135, R136, R143, R346)
- Rows: assigned in the implementation PRs (one per stage below)

## Summary

10.4 already decides that a function returning `Type` is a pure, memoized type function whose result has identity
(function, stage-0 argument hash), that new nominal types come from `record_type`, `sum_type` and `newtype` over
reflection records, and that `info(T)` returns a non-exhaustive `TypeInfo` sum. The toolchain implements only the
built-in `session.protocol`, with literal arguments, in `type X = f(...)` (issue #690 items 5 and 6, E017 today).
This RFC fixes what 10.4 leaves open: how a `Type` exists at stage 0, the shape of `TypeInfo`, the reifier
signatures, where an application may appear and what its arguments may be, and which surfaces may define one.
Each item extends the stage-0 machinery that `let x: T = #e` already runs on (the interpreter with stage-0
budgets), so there is one evaluator, not a second one for types.

## Decisions

### D1: `Type` is a stage-0 handle value

At stage 0 a `Type` is an opaque handle: a resolved type (a nominal path with type arguments, a built-in with its
arguments, or a reified type with its identity). Handles compare by type identity, so `f(Alias)` and `f(Target)`
are one application, and they hash structurally for the memo key. A `Type` crosses into run time only as a
monomorphization argument (unchanged, E262 otherwise). Rejected: representing a type as its `TypeInfo` tree.
That loses nominal identity (two records with equal fields would be one type) and makes recursive types infinite.

- Decision: accepted; Antonio, standing approval of 2026-11-08
- Row: stage 1

### D2: the shape of `TypeInfo` (`std.meta`)

`info(t: Type) -> TypeInfo` is a stage-0 function in `std.meta`. `TypeInfo` is a non-exhaustive sum, so a `match`
on it needs a wildcard arm, so adding a variant later is not a breaking change:

- `Record(RecordInfo)`: `name: Str`, `params: List[Type]`, `fields: List[FieldInfo]`, `annotations`, `layout`.
- `Sum(SumInfo)`: `name`, `params`, `variants: List[VariantInfo]` (each `name` and `fields`), `annotations`,
  `layout`.
- `Newtype(NewtypeInfo)`: `name`, `inner: Type`, `annotations`.
- `Scalar(ScalarInfo)`: `name`, `bits: Int`, `signed: Bool`, `float: Bool`.
- `Array(ArrayInfo)`: `elem: Type`, `len: Int`. `View(ViewInfo)`: `elem: Type`, `mutable: Bool`.
- `Optional(Type)`, `Tuple(List[Type])`, `Fn(FnInfo)` (parameters with names and types, return, `fail`, `uses`).
- `Builtin(BuiltinInfo)`: `name: Str` and `args: List[Type]` for `Str`, `List`, `Map`, `Set` and the other std
  containers, which have no fields to reflect.
- `Trait(TraitInfo)`: `name` and method signatures, for `info` on a trait.

`FieldInfo` is `name: Str`, `ty: Type`, `public: Bool`, `annotations: List[Annotation]`. `layout` is
`Layout(size: Int, align: Int)` under the target's layout (10.4 stage-0 target semantics). Fields are listed in
declaration order. A field the call site cannot see is left out of `fields`, and `x.(f)` on it is E264, so
reflection never widens access. Reading a `TypeInfo` field is what R143 tracks for incremental evaluation.

- Decision: accepted; Antonio, standing approval of 2026-11-08
- Row: stage 1

### D3: reifiers take the reflection records

`record_type(r: RecordInfo) -> Type`, `sum_type(s: SumInfo) -> Type` and `newtype(n: NewtypeInfo) -> Type`, so the
idiom is read, transform, reify: `record_type(RecordInfo(name: "Opt", fields: [f with ty: Option[f.ty] for f in
info(T).fields]))`. `layout` in the argument is ignored (it is computed); `annotations` are kept and checked as
typed annotations. The result is checked like a declared type (duplicate fields, unknown types, recursion through a
non-indirect field). A reified type is nominal: its identity is the application's (function, argument hash), and
`name` is its display name. Its sum variants join the module's flat variant namespace as `{name}_{label}` in Yan
Core and are resolved by the scrutinee's type at the surface, the scheme #740 chose for choice results. A reified
name that is taken is E265 (unchanged).

- Decision: accepted; Antonio, standing approval of 2026-11-08
- Row: stage 2

### D4: applications anywhere in type position, arguments any stage-0 expression (#690 item 6)

A type-function application may appear in any type position: field and parameter types, returns, type arguments
(`List[f(T)]`) and arguments of another application. Its arguments are stage-0 expressions: literals, stage-0
constants, `Type` arguments (a type expression in argument position), and calls of stage-0 functions. Before
types are resolved the elaborator collects the applications of a module, evaluates each distinct key once on the
stage-0 interpreter in dependency order, and declares the reified types. An application that depends on its own
result is E261 (the depth budget), not a hang. `type X = f(...)` stays the way to name one.

- Decision: accepted; Antonio, standing approval of 2026-11-08
- Row: stage 3

### D5: checking a type function

A function whose return type is `Type` is a stage-0 function: its body is checked once at the definition, its
calls are allowed only at stage 0 (in type position, in another stage-0 function, or under `#`), and a run-time
call is E262. It may use only the stage-0 capabilities (`alloc`, `embed`, `diag`, E268 for the rest). A
`compile_error` in it is E270 at the application, with the stage-0 backtrace.

- Decision: accepted; Antonio, standing approval of 2026-11-08
- Row: stage 2

### D6: surfaces

Yan Core and Yan define type functions. Yan Agent applies them (built-in and imported) but does not define one: a
Yan Agent function returning `Type` is the Yan Agent level error of 14 ("defining reflective code" is not in Yan
Agent), not E017. This keeps 14 as written. Rejected: allowing type functions in Yan Agent; it would contradict the
L5 placement of reflection and the agent-surface goal of one obvious way.

- Decision: accepted; Antonio, standing approval of 2026-11-08
- Row: stage 2

## Stages

1. `Type` handle in the interpreter, `std.meta` with `TypeInfo`, `info(T)` (D1, D2).
2. Reifiers, `-> Type` definitions in Yan Core and Yan, the Yan Agent diagnostic (D3, D5, D6). Closes #690 item 5.
3. Applications in any type position with stage-0 arguments (D4). Closes #690 item 6.

## Spec changes

10.4 (type functions, reflection), 14 (Not in Yan Agent), 17 (`std.meta`), 25 (codes, if a stage needs one).
