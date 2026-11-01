## 5. Yong: memory, layout and allocation

There is no tracing garbage collector and no implicit reference counting. Every value has exactly one owner and is destroyed at a statically known point. Reference counting exists only inside the library types `Rc[T]` and `Arc[T]`, which a program uses explicitly. Under mutable value semantics no reference cycle can be built without `Rc` or `Arc`; `Weak[T]` breaks cycles where a program builds them.

### 5.1 Representation

- A type is *trivially copyable* if it contains no heap-backed component and no `RawCell`. Such values live inline and are copied bitwise.
- Heap-backed values (`Str` above 23 bytes, `Bytes`, `List`, `Map`, `Set`, array buffers, boxed recursive variants, closures with heap captures) are a pointer to a block with no count header. `copy x` allocates and deep-copies; nothing else copies a heap block. Mutation is always in place.
- `Rc[T]` and `Arc[T]` hold an immutable `T` behind a count (`Arc`'s is atomic). `make_mut(&r)` copies the block if the count is above 1; this is the only copy-on-write in Yong and it is visible at the call.
- Sums use niche optimization: `Option[T]` costs no extra space when `T` has an invalid bit pattern (`bool`, `char`, refined integers, `Ptr`, Owned). Only the `Option` layouts of section 12.4 are guaranteed.
- `Map` and `Set` iterate in insertion order (section 7.7).

### 5.2 Layout

Default layout is unspecified: the compiler reorders fields to minimize padding. Layout attributes (Wen L4) fix it:

| Attribute | Layout |
| --- | --- |
| `@layout(c)` | the target C compiler's layout, quirks included; required for every type that crosses an FFI boundary |
| `@layout(ordered)` | declaration order with natural alignment, identical on every target |
| `@layout(transparent)` | same layout and ABI as the single non-zero-size field |
| `@layout(packed(n))` | declaration order, alignment capped at n |
| `@layout(align(n))` | alignment raised to n |
| `@layout(bits(uN))` | fields packed LSB first into a `uN`; field widths MUST sum to N (E908) |

C bitfields are reached only through generated accessors (section 12.3).

### 5.3 Ownership passes

The optimizer MUST run these on checked Yong, in order:

1. **Borrow lowering.** `let` parameters and views pass by pointer or by value as section 4.1's lowering column says, with no ownership operations.
2. **Drop elaboration.** Drops are placed at scope exit in reverse declaration order, temporaries at end of statement; moved-from values are not dropped, with drop flags only where a move is conditional (SQ14).
3. **Drop specialization.** Inline the drop of a statically known variant.
4. **Reuse analysis.** When a uniquely owned block of size n is dropped and a block of size n is then allocated in the same function, reuse it in place (`reuse x as Ctor`). Unique ownership makes this check static.

### 5.4 Drop order and cleanup

Locals drop in reverse declaration order, record fields in declaration order, and collection elements front to back. The children of a `par` block drop before the block exits. A panic aborts without running drops (section 8.3). Cleanup that can fail is an explicit `close() -> () fail E`. Dropping a value of a `@linear` type that was not closed is E412 (R443), except on a path that is already propagating a failure, where the handler's release intrinsic frees the native resource without user I/O.

**Drop (R343).** A type gets custom destruction only by implementing `trait Drop` with `def drop(var self)`, which runs once when the value's owner drops it. A `drop` body is infallible, uses no capability except `alloc`, does not block and is not a cancellation point (E421 otherwise). Releasing a native resource, such as closing a file descriptor, is an intrinsic of the handler that created the resource, not a capability call. A `@linear` type MUST NOT implement `Drop` (E421). Because a drop has no effect other than freeing memory and native resources, which task runs the final `Rc` or `Arc` drop is not observable.

### 5.5 Allocators and arenas

All allocation goes through the `alloc` capability (section 7.1), which decides whether code may allocate at all. `alloc` is ambient: passed like any capability but never written in `uses`; `uses none` forbids it.

- Every `alloc` collection takes a type parameter `A: Allocator = Global`. `Global` is zero-size and forwards to the bound `alloc` handler.
- std ships `Arena`, `FixedBuffer`, `PoolAlloc`, `Limited` and `Tracking` allocators.
- `Arena` (Wen L4) is an ordinary value (R441). `arena.alloc(v)` takes the arena by `let` and returns `Ref[T] from arena` (4.7). A collection built over `arena.allocator()` is second-class `from arena` too, so neither may escape or outlive the arena (E414); `copy x` deep-copies a value into the enclosing allocator. Dropping the arena frees all of it in one step. Xin has no arenas.

**Allocation failure.** The default `alloc` handler aborts the process on exhaustion. Every growable container operation (`append`, `insert`, `extend`, `reserve`, `with_capacity`) and every whole-input read (`read_all`, `read_to_str`) has a `try_` sibling returning `fail OutOfMemory`. A function whose allocation is bounded at compile time needs none. Freestanding code binds its own `alloc`.

**Allocation is not observable (R385).** The number, size and order of `alloc` operations are unspecified. The optimizer MAY elide an allocation whose block is never read after it is freed, merge allocations, reuse blocks (5.3), and place a block on the stack when it does not escape the function and the solver bounds its size by 4 KiB. A `Tracking` or `Limited` handler reports what happened; tests MUST NOT assert exact allocation counts except through `@forbid(alloc)`. Elision can remove an `OutOfMemory`, never cause one. R480 makes one elision mandatory: a list comprehension that is only iterated is never built.

### 5.6 Data-oriented containers

`alloc` provides, beside the collections of section 17:

- `Soa[T]` (Xin `soa[T]`) for any record `T`, generated at stage 0: one allocation, field segments 64-byte aligned; `view`, `span` and per-field accessors return second-class views; growth has `try_` siblings.
- `Pool[T, Tag]`, `DensePool[T]` and `Handle[T]` (u32 index, u32 generation). `get` on a stale handle returns None. A slot whose generation would overflow retires instead of wrapping.
- `Buffer` and `Bitmap`, the aligned columnar buffers shared by `Array` and `std.arrow` (section 11.8).

**Column expressions (R261).** An arithmetic, comparison or cast operator applied to a `Soa` field view returns a `ColExpr`: a second-class, non-allocating expression tree typed by its operands, so the view rules of 4.5 and E414 apply. A `ColExpr` is consumed by a reduction (`sum`, `min`, `max`, `count`, `any`, `all`), by `assign_into`, or by `collect`, which allocates and has a `try_` sibling. Consumption runs one fused loop. Overflow is checked (3.4) and reductions use the fixed tree of 9.3, so results are bit-identical at every width. Operands come from one `Soa`, or have lengths the solver proves equal; otherwise consumption fails with `LengthMismatch` before any element is read. Tables, table kernels and @yong/frame use this same ColExpr type through one trait, Subject\[R\], which Wen's implicit subject targets (R448).

**Keyed tables (R261).** `Keyed[K, T]` is a `Soa[T]` with a unique index on one field, chosen at stage 0, as in `Keyed[Str, Quote](key: .sym)`. Indexing it with a column yields a `ColExpr` of `Option[T]` rows, `or_fail(e)` turns the first miss into `fail e`, and inserting a duplicate key fails with `DuplicateKey`. Joins, grouping and query plans stay with tables (11.9).

### 5.7 Proof attributes (Wen L4)

| Attribute | The compiler MUST prove, or report each violating line (E900) |
| --- | --- |
| `@forbid(rc)` | no `Rc` or `Arc` count operation is reachable |
| `@forbid(alloc)` | no use of the `alloc` capability |
| `@forbid(copy)` | no `copy` of a heap-backed value remains |
| `@forbid(panic)` | no reachable panic (bounds, overflow, contracts, exclusivity) |

**In-place functions (R456).** `@fip` is a proof attribute like those above (Wen L4). The compiler MUST prove that every allocation in the function is a `reuse x as Ctor` (5.3) of a block the function consumed, and that its stack is constant, with recursion only through `become`, including its constructor form (3.5). Each violating line is E900. An `@fip` function MUST NOT take an `Rc` or `Arc` by `sink`, since their blocks are not uniquely owned.
