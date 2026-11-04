## 4. Yan: ownership, exclusivity and views

Yan has mutable value semantics with unique ownership. Every value has exactly one owner; assignment and argument passing move or borrow, two names never share mutable state, and references are second-class. There are no lifetime annotations anywhere except the `from` clause on functions that return a view (4.5) and `Ref` at L4 (4.7).

### 4.1 Parameter conventions

| Convention | Callee may | Caller's variable after the call | Typical lowering |
| --- | --- | --- | --- |
| `let` | read only | unchanged and usable | by value if trivially copyable and at most 2 words, else by pointer |
| `inout` | read and write | holds the callee's final value | pointer, marked `noalias` |
| `sink` | own and consume | dead: any later use is E401 | by value (move) |

The receiver of a method is a parameter like any other; it is exempt from E210 (SQ9). A type that holds a `RawCell` inline is not trivially copyable and always passes by address under `let` (section 6.3).

**`inout` and failure (R765).** When the callee fails, the caller's `inout` argument still holds the callee's final value, as on a normal return. No copy is made before the call.

**Bindings.** `let` bindings are immutable. Only `var` bindings and `inout` parameters may be targets of `Place = Rhs` (E403 otherwise). A field or element of a mutable place is itself mutable.

**Move and return elision (R384).** A `sink` argument larger than 2 words is passed as a pointer to the caller's storage, which the callee then owns in place. A returned value larger than 2 words is built in the caller's destination, including through `ret` of a named local. `append`, `insert` and record construction build the value in its final slot. A `fail E` payload larger than 2 words goes through a caller-provided slot, so the success path stays in registers. No move of a local into a call or a return emits a copy.

### 4.2 Copies, moves and sharing

Surfaces never write `move`; the elaborator inserts moves and trivial copies by last-use analysis:

1. A use of a variable that is its last use on every path becomes `move`.
2. Any other use passed to `sink`, stored into a container or record, or bound to a new name becomes a bitwise copy if the type is trivially copyable (contains no heap-backed component).
3. For a heap-backed type such a use is E413 unless the source writes `copy x` (Yan Agent `x.copy()`), a deep copy costing O(size). E413's fix inserts `copy`, or `share` when the type implements `Share`, so `autofix` resolves it without a model generation.

`copy x` is always a deep copy. Cheap aliasing is the trait `Share`, implemented by `Rc`, `Arc`, `Weak`, `Sender`, `Receiver` and actor handles: `share x` (Yan Agent `x.share()`) is an O(1) alias. `copy` on a `Share` type is E416, with the fix `share`. `copy v` on a view returns the owned type (`View[T]` gives `List[T]`, `StrView` gives `Str`).

**Consumption is written (R769).** In checked Yan Core a non-copy value leaves its local only by `move`, a `sink` argument, `ret`, a constructor field, a `sink` capture or (after optimization) `drop`; a bare bind `let y: T = x` of a non-copy `x` is a validator error. Ownership facts are not stored beside the text. `yan-check` owns one analysis over Yan, per function and per place (fields tracked separately): definitely initialized, maybe initialized, moved. It serves `yan-check`'s own use-after-move and exclusivity checks and `yan-opt`'s drop elaboration, and is recomputed on demand, never stored or hashed. The elaborator's last-use moves (rule 1) must agree with it; a disagreement is an internal error.

**`let` of a projection** copies if the projected type is trivially copyable, moves at last use, and is E413 otherwise; borrowing part of a buffer is a view (SQ10).

**Partial moves.** Fields may be moved out of an owned local, tracked per field. Not out of a type with a custom drop or `@linear`, nor through a view, `Rc` or `Arc` (SQ13).

### 4.3 The law of exclusivity

For each statement, consider every place accessed through `inout` or `MutView`, every place read, and every place passed `sink`. Then:

- No place accessed through `inout` may overlap any other place accessed in the same statement (E402).
- Two places overlap if one is a prefix of the other, or if they share a root and their paths differ only in index positions that are not provably distinct.
- Index distinctness is decided by the linear-arithmetic solver (section 8.6). If it cannot decide, the compiler MUST insert one `exclusive_check(i, j)`; a failed check panics with kind `exclusivity`.
- The same rule applies across the branches of a `par` block, treating the whole block as one statement.
- A capability is a place whose access depends on its sharing class (7.1, R342). An *exclusive* capability operation is an `inout` access, so an exclusive capability used in two `par` branches is E402. A *shared* capability is a `let` access, so branches use it at once. A *split* capability gives each branch its own handle automatically; `io`, `log` and `metrics` buffer per branch and commit in source order at the join (SQ15), and `io.split(n)` remains for splitting outside `par`. *Ambient* capabilities (`alloc`, `sched`) are never written in `uses` and are always allowed.

### 4.4 Projections

`a[i]`, `r.f` and accessor functions yield a borrowed place, valid only within the statement that produces it. A projection MUST NOT be stored, returned or captured. A borrow that outlives the statement is a view.

**Subscripts (R457).** A `sub` item defines a projection. It has the header of a function, and its body contains `yield p` for a place `p` reachable from its parameters (Yan `sub name(self, ...) -> T =`). The body MUST yield exactly once on every path (E422). Code after the `yield` runs when the caller's statement ends. One definition serves both access modes: a read binds `self` and the place as `let`, and an assignment target, an augmented assignment or an `&` argument binds them as `inout`. The body is checked under each mode it is used in, and E423 names the mode that fails. E422 is reported at the subscript's name and E423 at the yielded place (R473). The yielded place is a projection under the rule above, valid for one statement, so subscripts need no lifetimes. At 1.0 a subscript is always inlined and MUST NOT be recursive. `trait Index[K]` with `sub at(self, k: K) -> V` gives a user type `x[k]` and `x[k] = v` (10.3). Yan Agent may use any `Index` type but MUST NOT define a subscript.

### 4.5 Views

`View[T]` (read) and `MutView[T]` (read and write) borrow a contiguous part of a value: a slice of a `List`, `Bytes`, `Vec`, array buffer or `Soa` segment. The view of a `Str` is `StrView`; `View[u8]` is for bytes.

- **Second-class.** Views follow the same rule as capabilities (section 7.2): a view MUST NOT be stored in a record, container or closure that escapes, and MUST NOT outlive the value it borrows (E414). Any type that contains a view (tuple, `Option`, `List`, sum, record, closure, iterator adapter) is second-class and carries the view's sources.
- **Returning views.** A function returns a view only with a `from` clause naming the parameters it borrows: `def head(xs: View[u8]) -> View[u8] from xs`, or `from a, b` for several. The caller treats each named parameter as borrowed (`let` for `View`, `inout` for `MutView`) until the result's last use. `-> List[View[u8]] from text` is legal.
- **Exclusivity.** The law of exclusivity applies to views as to `inout` places, so two `MutView`s over overlapping ranges in one statement are E402.
- **Lending iterators.** `next(self: inout) -> View[T] from self` gives lending iterators; there are no generic associated types.

Views are in Yan Agent and Yan. They cover zero-copy parsing, borrowing iterators and in-place algorithms without lifetime parameters.

### 4.6 Closures

A closure captures by copy at creation (bitwise for trivially copyable values; a heap-backed capture needs `copy` or `share`, or is moved at last use).

- A closure that escapes (is stored, returned, or passed to a `sink` parameter) MUST NOT capture an `inout` parameter, a `var` binding by reference, a view or a capability (E406).
- A non-escaping closure argument may use any capability and any `var` of the enclosing scope. This is how higher-order functions stay effect-polymorphic without effect variables: the higher-order function is monomorphized per closure.
- **Fail-transparent closures.** A non-escaping closure argument may use `?` and `fail`. A failure raised inside it propagates out of the higher-order call as a failure of the caller, which must admit that error type. The higher-order function needs no annotation.
- Iterator adapters (`map`, `filter`, `take_while`, `zip`, `enumerate`, `flat_map`) are second-class values under the view rule, so a lazy chain that never escapes is not an escape. Concurrent callbacks (R436): a function type may be prefixed par, as in par (Request) -> Response. A closure passed to a par parameter may capture only let bindings of Sync types and may use only shared and split capabilities; one that captures Sync but non-Freeze state (Mutex, Atomic) still needs uses nondet (6.5), and its split-capability output follows R398. Calling a non-par function parameter from more than one branch of a par, or from a par for body, is E716.

### 4.7 Explicit references (Yan L4)

`Ref[T]` is a reference into an arena (section 5.5), returned by `arena.alloc(v)` as `Ref[T] from arena` (R441). It is second-class like a view (4.5): it may be stored only in data that is itself `from` the same arena, and it MUST NOT outlive the arena (E414). Creating a `Ref` retags its target as creating a view does (6.6).
