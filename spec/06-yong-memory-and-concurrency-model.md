## 6. Yong: memory and concurrency model

This section defines the abstract machine for every layer, including raw pointers, atomics and foreign code. Safe code (outside `unsafe`) cannot observe most of it, but the FFI rules (section 12), std's unsafe internals and the checkers depend on it.

### 6.1 Abstract machine

- Memory is a set of allocations, each with a base, size, alignment and liveness.
- A byte is `Uninit` or `Init(u8, provenance fragment)`.
- A pointer is (address, provenance), where provenance is (allocation id, Tree Borrows tag) or none.
- One machine covers all layers. The reference interpreter's checked mode is its executable definition; prose and interpreter disagreeing is a spec bug. Stage 0 runs on it (section 10.4).
- `conformance/ub/` holds programs that MUST be flagged with a named UB item (6.7) and programs that MUST run clean.

### 6.2 Atomics

The C++20 model: RC11 with the P0668 SC repair. `Relaxed`, `Acquire`, `Release`, `AcqRel` and `SeqCst` map one to one to C11 `memory_order`. There is no `Consume`, no out-of-thin-air values, and load buffering is allowed. Ordering is always explicit.

- `Atomic[T]` exists for `bool`, integers, `Ptr[U]` and padding-free records of those up to 16 bytes, and only where the target makes it lock-free (E418 otherwise; there are no hidden locks).
- Operations: `load`, `store`, `swap`, `compare_exchange` and `compare_exchange_weak` with a separate failure ordering, `fetch_add`, `fetch_sub`, `fetch_and`, `fetch_or`, `fetch_xor`, `fetch_max`, `fetch_min` and `fetch_update`. Integer read-modify-writes wrap.
- `wait`, `notify_one` and `notify_all` on 32-bit atomics (futex, `os_sync_wait_on_address`, `WaitOnAddress`).
- `atomic_load_bytes` and `atomic_store_bytes` for seqlocks.
- Atomics are usable from Jian through std.

### 6.3 Interior mutability

`RawCell[T]` (L6) is the only way to write through a `let` borrow, an `Rc` or an `Arc`. std builds `Cell[T]` (one task), `Atomic[T]`, `Mutex[T]`, `RwLock[T]`, `Once[T]` and `Lazy[T]` on it, all usable from Jian. A type holding a `RawCell` inline is not trivially copyable and always passes by address under `let`. `inout` access is plain exclusive access (`get_mut`). `Mutex` has no poisoning, and `lock()` cannot fail because panics abort. There is no `Condvar` at 1.0.

**Initializers and locks (R393, R395).** A `Once` or `Lazy` initializer reachable from more than one task MUST use no capability except `alloc` (E714). Otherwise it needs `uses nondet`, and its value is a capability result that replay records. An initializer never runs under the handler bindings of the task that touched it. A `Mutex` or `RwLock` guard live across a blocking operation is E323 on every platform. Every lock wait is therefore bounded by a critical section with no suspension in it, so `lock()` stays infallible and is not a cancellation point.

### 6.4 Thread-safety traits

The compiler derives three auto traits:

| Trait | Meaning |
| --- | --- |
| `Send` | a value may move to another task |
| `Sync` | a `let` borrow may be held by two tasks at once |
| `Freeze` | no `RawCell` is reachable (Rc and Arc counts excepted) |

`Rc` is neither Send nor Sync. `Arc[T]` is both iff `T: Send + Sync`. `Cell` and `RawCell` are not Sync. `Atomic` is both. `Mutex[T]` is both iff `T: Send`. `Ptr` is neither. Channel and actor message payloads MUST be Send (E415). A value borrowed by two branches of one `par` MUST be Sync (E417). A closure's auto traits follow its captures: a by-reference var capture is neither Send nor Sync, and a capture of an exclusive capability is not Sync, so E417 also covers a closure run by two branches (R436). `unsafe impl Send` and `unsafe impl Sync` are allowed only at L6 and are listed by `yong audit`. Reflection reports the auto traits read-only, and no derive can implement one.

**Channel handles and reference counts (R391, R392).** `Sender` and `Receiver` are Send, not Sync. `send`, `try_send`, `recv`, `try_recv` and `close` take the handle `inout`, so using one handle in two branches is E402, and a second producer or consumer exists only through `share` (E709). `Arc.get_mut`, `make_mut`, `try_unwrap`, `into_inner`, `strong_count`, `weak_count` and `Weak.upgrade` read a count another task may change, so they require `uses nondet` (E709). The `Rc` forms need nothing, because an `Rc` never leaves its task.

### 6.5 Guarantees and synchronization

**Guarantees.** Safe code is data-race free. DRF-SC holds: a race-free execution whose atomics are all SeqCst is sequentially consistent. Determinism (section 7.7) is enforced: sharing a non-Freeze value between tasks that may run at once, `share` of a `Sender` or a `Receiver`, `try_send`, `try_recv` and `select` require `uses nondet` (E709). Channels with one sender and one receiver and blocking `send` and `recv`, `par for` and `par_fold` need nothing (R342).

**Lattice cells (R454).** A `Lub[L]` (9.3) shared by `let` borrow into the branches of a plain `par` block, or into the items of a collecting `par for`, is exempt from the `nondet` rule above, because its writes commute and it is read only after the block. Shared any other way, such as through `Arc`, a channel or an actor, its `get()` requires `uses nondet` (E709).

**Happens-before.** Only these create happens-before edges in safe code:

- `par` fork and join;
- a channel send before the matching recv; close before the end is seen;
- unlock before the next lock;
- `Once` and `Lazy` initialization before every observation;
- every `Rc` or `Arc` use before the final drop (Release decrement, Acquire fence);
- an actor send before its handler runs; the reply before the caller resumes;
- I/O submission before the operation starts; completion before the waiter resumes (all handlers).

`wait` and `notify` add none.

**Scheduling semantics.** A program means the executions the atomics model allows under a fair scheduler. A `sched` handler picks among them and never adds any: `sequential` picks source order, `seeded(seed)` picks uniformly or by PCT, `pool` leaves it to OS threads, and `explore(bound)` enumerates all of them (6.9). Plain `par` has parallel forward progress; W814 flags spin loops on atomics shared by plain `par` branches. `par concurrent` has concurrent forward progress (E708 under a handler that cannot give it).

**Heartbeat pool (R481).** Under `pool`, plain `par` branches and `par for` bodies run inline on the forking worker, and the oldest unstarted one is offered to an idle worker only at a heartbeat, every 100 µs by default (`pool(heartbeat = d)`). An unpromoted branch costs no more than a call, and there is no grain-size parameter. Heartbeat polls are a thread-local flag check at loop back-edges and calls on paths reachable from a `par` body; other code has none. `par concurrent` starts its branches eagerly, since it needs concurrent progress. Results are unaffected (law 7, R382).

### 6.6 Aliasing and provenance

**Aliasing is Tree Borrows.** Retags happen only at function entry (`inout`: Unique, protected; `let` by address: Frozen over Freeze bytes, Cell over RawCell bytes, protected) and at view creation (`View`: Frozen, `MutView`: Unique, until last use). There is no retag on moves, projections or `Ptr` operations; a derived `Ptr` inherits its place's tag. Owned heap blocks are not retagged on move. This licenses `noalias` on `inout` and `MutView`, `readonly` on `let` and `View` of Freeze types, and read-read reordering.

**Code-generation facts (R383).** The LLVM backend MUST emit: `noalias` on `inout`, `MutView`, and Freeze `let` or `View` passed by address; `captures(none)` on every borrow parameter, since views are second-class; `nonnull`, `dereferenceable`, `align` and `noundef` where the type implies them; range metadata from refinements, niches and enum tags; `nounwind` on every function; `memory(...)` derived from `uses` and parameter conventions (a `uses none` function with only `let` parameters is `memory(argmem: read)`); and `willreturn` only where `decreases` is proven. The Cranelift backend is exempt.

**Provenance is strict.** `Ptr[T]` is never null; nullable is `Option[Ptr[T]]`. `addr() -> Addr` drops provenance; `with_addr` and `map_addr` keep it; `without_provenance` and `dangling` never dereference. `expose_provenance` and `with_exposed_provenance` follow PNVI-ae-udi (ISO/IEC TS 6010:2025) and are legal only in `ffi` modules (E419); pointers received from C are exposed. Pointer `==` compares addresses. Byte copies preserve provenance; transmuting a pointer to an integer yields the address unexposed, and an integer to a pointer yields no provenance.

**CHERI readiness.** `Addr` is the address width; `size_of[Ptr[T]]` may exceed it, and no code may assume a pointer fits in `Addr`. Lengths and indices are never `Addr`. `Atomic[Ptr]` is pointer width. On CHERI targets (M4) views lower to bounded capabilities.

### 6.7 Undefined behavior

The list is closed; anything else is defined. Only unsafe code, foreign code or a wrong `unsafe impl` can cause UB:

1. a data race, including mixed-size atomics;
2. an access outside a pointer's provenance or to a dead allocation;
3. an offset outside the allocation, except one past the end;
4. a misaligned typed access;
5. a Tree Borrows aliasing violation (6.6);
6. an invalid value: `bool`, `char` or discriminant out of range, null `Ptr`, a false refinement, a dead view, `Uninit` outside `Uninit[T]` and padding;
7. mutating immutable memory;
8. a double drop or use of a moved-from place;
9. a forged capability, handler, `Secret` or second-class value;
10. a call through a mismatched signature or ABI;
11. use of an unsupported target feature;
12. inline assembly breaking its operands or options (section 12.9);
13. unwinding or `longjmp` across Yong frames;
14. a foreign callee violating a `safe` extern declaration (12.1);
15. `Atomic.wait` on non-atomic or misaligned storage;
16. violating an `unsafe fn` precondition.

Not UB: leaks; deadlock (a global deadlock panics and a partial one may hang until a deadline, R396); overflow and division by zero (they panic), races between atomics, infinite loops, reading padding as `Uninit`, comparing pointers across allocations.

### 6.8 The safety contract

`unsafe` code may assume: values are valid for their type, including refinements; `inout` is disjoint from everything else the callee reaches; `let` Freeze bytes are unchanged during the call; view targets are live (and exclusive for `MutView`) until last use; Send and Sync are honored; no capability is forged; borrowed places do not move.

It may not assume: that drops run (leaks are safe); that owned values keep their address across moves (there is no `Pin`); that safe `Ord`, `Eq` and `Hash` impls are lawful; any schedule, or that plain `par` runs concurrently; any layout without `@layout`; any ordering from `Relaxed`.

`unsafe fn` preconditions are `pre` contracts, evaluated by the checked interpreter and listed by `yong audit`.

**Volatile and fences.** `read_volatile` and `write_volatile` are never elided, merged or reordered against each other; they are not atomic and do not synchronize; aligned scalars up to the machine word do not tear. `fence` and `compiler_fence` are as in C++. A non-terminating loop is not UB; there is no `mustprogress`.

### 6.9 Checkers

- **UB checker.** `yong test --ub` runs tests in the checked interpreter: abstract bytes, allocations, Tree Borrows with tag GC, vector-clock race detection, store-buffer weak memory, `pre` contracts, capability forgery and a leak report. Reports name the 6.7 item and print a `yong replay` trace. Every unsafe block in std needs tests passing under `--ub`. The Rust implementation adds `--sanitize=address,thread` in M2; a native Tree Borrows sanitizer is an M4 candidate.
- **Concurrency checker.** The `explore(bound)` sched handler does exhaustive stateless model checking with optimal DPOR (TruSt) over RC11 in the reference interpreter, intercepting atomics, `Mutex`, channels, `Arc` and `Once`, with loops bounded by `bound`. `yong test --explore[=N]` runs `@explore` tests; `--estimate` reports the execution count; failures print a shrunk schedule as a `yong replay` trace. `seeded(seed)` stays the randomized tester.
