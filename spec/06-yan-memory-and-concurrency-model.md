## 6. Yan: memory and concurrency model

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
- `wait`, `notify_one` and `notify_all` on `Atomic[u32]` and `Atomic[i32]` (futex, `os_sync_wait_on_address`, `WaitOnAddress`). `wait(self, expected, o: LoadOrdering) -> None fail Cancelled | Timeout` loads the word with `o` and returns at once when it is not `expected`; otherwise the task parks on the word's address until a `notify` releases it, and it may then return with the word still equal to `expected`. `notify_one` releases the task parked longest on the word and `notify_all` every one. The three need `uses nondet` (E709), because what they observe and release depends on other tasks (R1101). The interpreter and the checked machine release in first-in, first-out order; native code may release any waiter. A task parked in `wait` counts for the deadlock detector (R396, R1102). In a cancelled task `wait` fails `Cancelled` (R1103).
- `atomic_load_bytes` and `atomic_store_bytes` for seqlocks.
- Atomics are usable from Yan Agent through std.

### 6.3 Interior mutability

`RawCell[T]` (L6) is the only way to write through a `let` borrow, an `Rc` or an `Arc`. std builds `Cell[T]` (one task), `Atomic[T]`, `Mutex[T]`, `RwLock[T]`, `Once[T]` and `Lazy[T]` on it, all usable from Yan Agent. A type holding a `RawCell` inline is not trivially copyable and always passes by address under `let`. `inout` access is plain exclusive access (`get_mut`). `Mutex` has no poisoning, and `lock()` cannot fail because panics abort. There is no `Condvar` at 0.1.0.

**Initializers and locks (R393, R395).** A `Once` or `Lazy` initializer reachable from more than one task MUST use no capability except `alloc` (E714). Otherwise it needs `uses nondet`, and its value is a capability result that replay records. An initializer never runs under the handler bindings of the task that touched it. A `Mutex` or `RwLock` guard live across a blocking operation is E323 on every platform. Every lock wait is therefore bounded by a critical section with no suspension in it, so `lock()` stays infallible and is not a cancellation point.

**Mutex guard (R1022, R1023, R1024).** `Mutex[T](v)` holds `v`. `m.lock()` returns a `MutexGuard[T]` that holds the lock until the end of the block that bound it, on every path out of that block (return, `fail`, `break`, `continue`); drop elaboration (5.3) unlocks it there. The guard reads and writes the protected value only through `get()`, which copies it out and needs `T` to be `Copy` (a non-`Copy` `T` is E425; use `replace`), `set(v)` and `replace(v)`, which returns the old value, and through the place `g.value` (R1035). A guard is second-class (4.5): passing it on, returning it, rebinding it or storing it is E414. A task switches only at a suspension point and a guard is never live across one (R395), so a `lock()` that finds the mutex held is a task locking a mutex it already holds. That is a deadlock, and it panics with kind `deadlock` (R396). **Guard place (R1035).** `g.value` is the protected value as a place, a std subscript (4.4) over the lock block's value word. It follows 4.4: valid for one statement, bound `let` when read and `inout` when assigned or passed as `&g.value`, under exclusivity (4.3). `g.value.f = 1` and `f(&g.value)` work for any `T`, and `g.value[a..b]` is an ordinary slice, a `MutView` `from g` (4.5) when `T` is a `List`, `Bytes` or `Vec`. There is no whole-value `MutView[T]`: `inout` is that access. `get`, `set` and `replace` are unchanged. Under `--ub` the access is a plain access to the lock block, so one after the guard is dropped is a race against the unlock. **Early unlock (R1036).** A guard bound by `with` lives to the end of the `with` block and no longer: Yan `with g = m.lock():`, Yan Agent `with m.lock() as g:`. The form is a nested block starting at the binding, so drop elaboration (5.3), the unlock on every path out and the live range of E323 are those of the smaller block; the unlock is never moved earlier than the block end, and a bare `g = m.lock()` still holds to the end of its enclosing block. **Escapes (R1037).** A `MutexGuard[T]` is a view for escape analysis (4.5): second-class, with the locked `Mutex` place as its source. Storing it in a record or container is E414, capturing it in an escaping closure is E406 (4.6), sending it on a channel or to an actor is E415 (not `Send`) and borrowing it from two `par` branches is E417 (not `Sync`). A non-escaping closure may capture it, and a call to that closure is a call in the guard's live range. **Non-blocking and owned access (R1038, R1039).** `m.try_lock()` returns `Option[MutexGuard[T]]` and requires `uses nondet` (E709, 6.5), because whether the lock is free depends on other tasks; replay records the result as a nondeterministic choice (7.6). It returns `None` when another task holds the lock and when the calling task already holds it (where `lock()` panics with `deadlock`), and never panics. The option is second-class (4.5): it is legal only as the scrutinee of a `match` or an `if let` and its Yan equivalents, where the `Some` binding is a guard live to the end of that arm and unlocked there on every path out. Naming the option, passing it, returning it or storing it is E414. `m.get_mut` is an attribute of a `Mutex` that is `inout`: the protected value as a place, a std subscript like `g.value` (R1035); it takes no lock and needs no capability, because `inout` of the `Mutex` excludes every other task. Through an `Arc` the path is `Arc.get_mut` (6.4). `m.into_inner()` takes the `Mutex` by `take`, moves the value out and frees the lock block with no lock and no capability. Under `--ub` the `get_mut` access is a plain access to the lock block, so one concurrent with a `lock` is a reported race.

A guard is live from its binding to the end of its block; in that range a call to a std operation that can wait on another task, a call to a function that may reach one, or a `par` is E323.

### 6.4 Thread-safety traits

The compiler derives three auto traits:

| Trait | Meaning |
| --- | --- |
| `Send` | a value may move to another task |
| `Sync` | a `let` borrow may be held by two tasks at once |
| `Freeze` | no `RawCell` is reachable (Rc and Arc counts excepted) |

`Rc` is neither Send nor Sync. `Arc[T]` is both iff `T: Send + Sync`. `Cell` and `RawCell` are not Sync. `Atomic` is both. `Mutex[T]` is both iff `T: Send`, and a `MutexGuard` is neither (R1037). `Ptr` is neither. Channel and actor message payloads MUST be Send (E415). A value borrowed by two branches of one `par` MUST be Sync (E417). A binding declared outside a `par` is checked by its type in every branch that uses it, even when no other branch names it, because a clone can share its allocation under another name: a branch that borrows it with `let` needs it Sync (E417) and one that moves it or borrows it `inout` needs it Send (E415), so an `Rc` never enters a branch (R1066). A closure's auto traits follow its captures: a by-reference var capture is neither Send nor Sync, and a capture of an exclusive capability is not Sync, so E417 also covers a closure run by two branches (R436). `unsafe impl Send` and `unsafe impl Sync` are allowed only at L6 and are listed by `yan audit`. Reflection reports the auto traits read-only, and no derive can implement one.

**Auto traits are one computation (R1040).** Send and Sync are computed structurally from the checked type by one function that every E415 and E417 site consults (channel and actor payloads, `par` borrows, closure captures, `Arc`): a record, tuple, union or enum is Send or Sync iff every component is, and the primitive facts above are one table in std metadata. `Mutex[T]` is Send and Sync iff `T: Send`; a `Mutex[Rc[int]]` is therefore neither. When `Mutex` is written in std over `RawCell` (the table row), the row becomes `unsafe impl` for Send and Sync (L6, listed by `yan audit`) with no change to the check.

**Channel handles and reference counts (R391, R392).** `Sender` and `Receiver` are Send, not Sync. `send`, `try_send`, `recv`, `try_recv` and `close` take the handle `inout`, so using one handle in two branches is E402, and a second producer or consumer exists only through `share` (E709). `Arc.get_mut`, `make_mut`, `try_unwrap`, `into_inner`, `strong_count`, `weak_count` and `Weak.upgrade` read a count another task may change, so they require `uses nondet` (E709). The `Rc` forms need nothing, because an `Rc` never leaves its task.

### 6.5 Guarantees and synchronization

**Guarantees.** Safe code is data-race free. DRF-SC holds: a race-free execution whose atomics are all SeqCst is sequentially consistent. **Sharing a `Mutex` (R1041).** A `Mutex` holds a `RawCell`, so it is not Freeze, and the sharing site is the E709 check: borrowing or moving it into two `par` branches, a `par` item or a spawned task, or `share` of an `Arc` holding one, requires `uses nondet`. `lock`, `get`, `set`, `replace`, `get_mut` and `into_inner` need no capability, and there is no exemption for a commutative critical section (compare R454). Determinism (section 7.7) is enforced: sharing a non-Freeze value between tasks that may run at once, `share` of a `Sender` or a `Receiver`, `try_send`, `try_recv` and `select` require `uses nondet` (E709). Channels with one sender and one receiver and blocking `send` and `recv`, `par for` and `par_fold` need nothing (R342).

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

**Native pool for plain `par` (R1072, R1073).** In native code a plain `par` whose arms are pool-safe runs each arm on a worker and joins all of them before the code after it, and a result never depends on the worker count. An arm is pool-safe when it has no nested `par`, leaves only by reaching its end, by a panic, or by `?`, `fail` or `return` of word values (R1089), calls only pool-safe functions, and uses no handler other than `rand` and no I/O other than `print`, channel, actor, atomic, indirect call or pointer exposure; any other `par` runs its arms inline in source order, as do `par` blocks under a `pct` scheduler and inside a worker. The worker count is the process setting `YAN_THREADS`, one per core when unset (1 runs inline). If several arms panic, the program ends with the panic of the lowest-index arm once every arm has settled, as it does when the arms run in source order.

**Arms that leave a pool block (R1089, R1090).** Among the arms of a pool `par` that leave, by `?`, `fail`, `return` or a panic, the lowest index decides the block, as in source order: the block ends as that arm does. Arms above it that have not started are skipped, and the exits, panics and output of arms above it that ran are discarded. An arm drops the names it binds on its exit path; a name bound outside it is dropped once, by the frame that returns, on the winning exit (R1090).

**Output and randomness in pool arms (R1088).** What a pool arm prints is held until the join and written in arm order, in full for every arm, or up to and including the lowest panicking arm, so stdout equals the source-order run at every worker count. A `rand` handler in scope is split per arm by task path (R685) before the pool arm draws from it, so draws do not depend on which worker runs the arm.

### 6.6 Aliasing and provenance

**Aliasing is Tree Borrows.** Retags happen only at function entry (`inout`: Unique, protected; `let` by address: Frozen over Freeze bytes, Cell over RawCell bytes, protected) and at view creation (`View`: Frozen, `MutView`: Unique, until last use). There is no retag on moves, projections or `Ptr` operations; a derived `Ptr` inherits its place's tag. Owned heap blocks are not retagged on move. This licenses `noalias` on `inout` and `MutView`, `readonly` on `let` and `View` of Freeze types, and read-read reordering.

**Code-generation facts (R383).** The LLVM backend MUST emit: `noalias` on `inout`, `MutView`, and Freeze `let` or `View` passed by address; `captures(none)` on every borrow parameter, since views are second-class; `nonnull`, `dereferenceable`, `align` and `noundef` where the type implies them; range metadata from refinements, niches and enum tags; `nounwind` on every function; `memory(...)` derived from `uses` and parameter conventions (a `uses none` function with only `let` parameters is `memory(argmem: read)`); and `willreturn` only where `decreases` is proven. The Cranelift backend is exempt.

**Provenance is strict.** `Ptr[T]` is never null; nullable is `Option[Ptr[T]]`. `addr() -> Addr` drops provenance; `with_addr` and `map_addr` keep it; `without_provenance` and `dangling` never dereference. `expose_provenance` and `with_exposed_provenance` follow PNVI-ae-udi (ISO/IEC TS 6010:2025) and are legal only in `ffi` modules (E419); a pointer constructor that takes no `Ptr` argument (`dangling`, `without_provenance`, `with_exposed_provenance`) takes its pointee type from `[T]` or the expected type, else E205 (R1055); pointers received from C are exposed. Pointer `==` compares addresses. Byte copies preserve provenance; transmuting a pointer to an integer yields the address unexposed, and an integer to a pointer yields no provenance.

**`Ptr.take` (R1094).** `unsafe` `Ptr.take[T](p)` reads the pointee like `read` and leaves it moved-from (6.7); `read` stays a copy and does not move, so a peek at a non-`Copy` value that is then forgotten is defined. The pointee MUST be live and initialised; `write` re-initialises a moved-from pointee. Taking a `Copy` value is not a move. Native code and the interpreter run `take` as `read`: only the checked machine (6.9) tracks the move.

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
8. a double drop or use of a moved-from place (R1092);
9. a forged capability, handler, `Secret` or second-class value;
10. a call through a mismatched signature or ABI;
11. use of an unsupported target feature;
12. inline assembly breaking its operands or options (section 12.9);
13. unwinding or `longjmp` across Yan frames;
14. a foreign callee violating a `safe` extern declaration (12.1);
15. `Atomic.wait` on storage that is not 4-aligned, not inside a live allocation, or not a word the program has accessed with an atomic instruction (RFC 0089 D1);
16. violating an `unsafe fn` precondition.

**Moved-from places (R1092).** A place is moved-from from the move that takes its value until a store or assignment to that same place. A field moved out of a record is moved-from and its siblings stay defined (a drop of the remainder reads only them); a move out of a `Copy` place is not a move. A read, a `View` or `MutView` borrow, a second move, a `Copy` or a drop of a moved-from place is item 8. In safe code the static check (4.2) rejects every program the checker would flag, so item 8 arises only through unsafe code, foreign code or a compiler bug.

Not UB: leaks; deadlock (a global deadlock panics and a partial one may hang until a deadline, R396); overflow and division by zero (they panic), races between atomics, infinite loops, reading padding as `Uninit`, comparing pointers across allocations.

### 6.8 The safety contract

`unsafe` code may assume: values are valid for their type, including refinements; `inout` is disjoint from everything else the callee reaches; `let` Freeze bytes are unchanged during the call; view targets are live (and exclusive for `MutView`) until last use; Send and Sync are honored; no capability is forged; borrowed places do not move.

It may not assume: that drops run (leaks are safe); that owned values keep their address across moves (there is no `Pin`); that safe `Ord`, `Eq` and `Hash` impls are lawful; any schedule, or that plain `par` runs concurrently; any layout without `@layout`; any ordering from `Relaxed`.

`unsafe fn` preconditions are `pre` contracts, listed by `yan audit`. They are checked at `check` level (a `contract` panic) and trusted at `release`, with no checked `pub` entry (8.5). Under `--ub` each call evaluates them before the body, and a false one is reported as item 16, naming the function and the clause as R882 does (R1014).

**Volatile and fences.** `read_volatile` and `write_volatile` are never elided, merged or reordered against each other; they are not atomic and do not synchronize; aligned scalars up to the machine word do not tear. `fence` and `compiler_fence` are as in C++. A non-terminating loop is not UB; there is no `mustprogress`.

### 6.9 Checkers

- **UB checker.** `yan test --ub` runs tests in the checked interpreter: abstract bytes, allocations, Tree Borrows with tag GC, vector-clock race detection, store-buffer weak memory, `pre` contracts of every `unsafe fn` call (R1014), capability forgery and a leak report (R1098 to R1100). Reports name the 6.7 item and print a `yan replay` trace. Item 8 is reported at the first access after a move and names the move site (R1093); item 15 is reported at the `wait`, where the address is checked (RFC 0089 D1); moves reach the checker as codeless MIR markers that no backend turns into code. **Leak report.** `yan run --ub` prints one `warning[W862]` per allocation still live when `main` returns, naming its size and the function that allocated it, and exits as the program does (R1098); `--ub=deny-leaks` also exits with the UB status when one is printed (R1099). String literals, blocks the runtime owns and allocations reachable from a `static` are not leaks (R1100). A leak is not UB (6.7); only the checked machine reports it. Every unsafe block in std needs tests passing under `--ub`. The Rust implementation adds `--sanitize=address,thread` in M2; a native Tree Borrows sanitizer is an M4 candidate.
- **Concurrency checker.** The `explore(bound)` sched handler does exhaustive stateless model checking with optimal DPOR (TruSt) over RC11 in the reference interpreter, intercepting atomics, `Mutex`, channels, `Arc` and `Once`, with loops bounded by `bound`. `yan test --explore[=N]` runs `@explore` tests; `--estimate` reports the execution count; failures print a shrunk schedule as a `yan replay` trace. `seeded(seed)` stays the randomized tester.
