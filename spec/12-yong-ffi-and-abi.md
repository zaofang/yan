## 12. Yong: FFI and ABI

Foreign code is reached through C. A module is an `ffi` module when its file lives under the package's `ffi/` directory; only `ffi` modules (and std) may contain `unsafe` (E901) or extern blocks (E902). The root package's `allow_ffi` list is its consent that a dependency contains extern blocks.

### 12.1 Extern blocks

`unsafe extern "C" lib name link "x"` declares a foreign library (Wen L6). Writing the block is the unsafe act. Each item is `safe` (callable from safe code; the declaration asserts that no well-typed argument causes UB) or `unsafe` (a call needs an `unsafe` block, E903). Extern statics are declared the same way.

### 12.2 Libraries are capabilities

`lib zlib` declares trait `Zlib`, capability `zlib` and a native handler that calls the linked symbols. Callers need `uses zlib`, the package ceiling must list it (E314), and tests may bind a fake. An item marked `uses none` is pure: it needs no capability, is deterministic and is cacheable (18.4); this is a trusted claim, listed by `yong audit`. Its library still appears in the package ceiling and needs the root's `allow_ffi` consent, so a `uses none` claim never moves code outside the ceiling (R673). Calls through a library capability are nondeterministic like I/O and blocking unless the item is declared `nonblocking`. Extern libraries that std binds inside its own handlers (aws-lc, io\_uring syscalls) never appear in user signatures.

### 12.3 Layout and C types

Types that cross MUST be `@layout(c)` or C-profile types (E904); layout attributes are section 5.2. A `@layout(c)` enum has a backing integer (default `c_int`) and is open: every match needs a `_` arm (E503), and W811 does not apply. C bitfields are reached only through generated accessors.

`core.ffi` provides `c_char` to `c_ulonglong`, `c_size`, `c_ssize`, `c_ptrdiff`, `c_float`, `c_double`, `c_void`, `CStr` (a second-class view of a NUL-terminated buffer), `CString` and `c"..."` literals (interior NUL is E140). Calls to C variadics are allowed with the default argument promotions checked (E905); Yong functions are never variadic.

### 12.4 Niches

`Option[T]` for a non-null raw pointer, a C function pointer, `Owned` has C pointer layout and ABI, with None = NULL. No other `Option` layout is guaranteed.

### 12.5 Ownership across the boundary

Types say who owns what:

| Type | Meaning | Lowering |
| --- | --- | --- |
| `View[T]`, `MutView[T]` | borrowed for the call, not retained | ptr and len |
| `-> View[T] from p` | the result borrows `p` | ptr and len |
| `Owned[T, free_fn]` | C-allocated, dropped by calling `free_fn` once | pointer |
| `sink` parameter (take in Xin and Wen) | ownership passes to C | by value |
| `extern type T drop f` | unsized opaque handle, dropped by `f` | pointer |
| `Ptr[T]` | anything else, unsafe | pointer |

Out parameters become return values.

**Callbacks.** `scoped fn(...)` (or C `__noescape`) accepts any closure, including captured views and capabilities, valid only during the call, with no allocation. `escaping fn(...)` accepts closures with owned captures only (no capabilities or views, E406), passed with a drop thunk; registration returns an `Owned` token whose drop unregisters. A callback on a thread Yong did not start runs with only `alloc` and the `sequential` scheduler.

**Unwinding.** Every extern call is nounwind. A foreign unwind reaching a Yong frame aborts through a table-only abort pad at the call site. There is no "C-unwind". Items with returns-twice semantics (`setjmp`, `vfork`) cannot be declared (E906); `longjmp` over Yong frames is UB (6.7).

### 12.6 Importing C

`yong ffi import header --lib name` reads headers with libclang once, writes `ffi/name.wen` and pins header hashes in `yong.lock`; the compiler never parses C. `__counted_by`, `__sized_by`, `__noescape`, `__lifetimebound`, nullability, ownership and pure attributes map to `View`, `MutView`, `from`, `Option`, `Owned` and `uses none`. An item is emitted `safe` only when every pointer is fully annotated. A Xin overlay `ffi/name.overlay.xin` adds annotations to headers the user does not own. Hand edits to imported modules are flagged by `yong audit`.

### 12.7 The C profile and exports

Yong has no stable Yong-to-Yong ABI. The stable binary boundary is the C ABI under the Yong C profile `yong_abi_v1`, frozen at 1.0:

| Yong | C profile |
| --- | --- |
| views | `{ptr, len}` |
| owned `Str`, `List` | `{ptr, len, cap}` with a generated free |
| `Option` | by 12.4, else `{bool, T}` |
| closed sums | `{uint32 tag, union}` in declaration order |
| `fail E` | `int32` status plus out-parameters |
| opaque exported types | pointers with a generated `_free` |
| closures | `{fn, ctx}` or `{fn, ctx, drop}` |

`@export` (optionally `@export(name=...)`) on a `pub` item whose signature uses only profile types is legal in Xin and Wen (E907 otherwise). A `uses none` export is a plain C function. An export that uses capabilities takes a leading `yong_world *`, built by the host from explicit grants (directories, network targets, scheduler) and binding std's hosted handlers; it is the only source of host handlers besides the platform (R672).

**Bindings.** `yong bind --lang c`, `rust` or `python` generates from the export set: a C header with `_Static_assert` on every size, alignment and offset; a Rust `-sys` crate plus a safe crate (slices, lifetimes from `from`, Drop, Result, Send and Sync); a CPython extension.

**Python extensions** are built on the CPython Limited API: abi3 wheels (3.12+) and abi3t wheels (3.15+), the PEP 793 entry point, PEP 697 type data, vectorcall and `.pyi` stubs. Modules whose exports are Sync or `uses none` declare `Py_MOD_GIL_NOT_USED`. Scalar views use the buffer protocol, tables and columns the Arrow PyCapsule interface, arrays DLPack. `fail E` raises a generated exception class; a panic aborts.

**Shipping (R640).** `yong build --wheel` writes the abi3 and abi3t wheels with PEP 740 attestations derived from the version's provenance, for PyPI trusted publishing. The `yong` PyPI wheel (18.8) is also a PEP 517 build backend, so `pip install` of a source distribution fetches the pinned toolchain and builds. The published `yong-build` crate lets a Rust `build.rs` run `yong build` and link the generated `-sys` crate.

### 12.8 Linking

The manifest's `native` table sets static or dynamic linking per target and may list C sources, compiled with the toolchain's pinned clang (section 19.5). Windows imports need no import libraries. Native libraries that first-party packages bind (ICU4X for `@yong/i18n`) ship inside the toolchain, built reproducibly with it under the audits of 19.6; packages carry no Rust sources or prebuilt binaries (R639). Internal symbols are hash-mangled with hidden visibility; duplicate export names are E909 at link time. `@weak` gives an `Option[fn ...]`.

### 12.9 Inline assembly

Inside `unsafe`, `asm(template, operands..., options...)` with operands `in(reg) e`, `out(reg) name: T`, `inout(reg) place`, `lateout(reg) name: T`, an explicit register such as `in("eax") e`, `clobber_abi("C")`, and the options `pure`, `nomem`, `readonly` and `nostack`. The compiler checks operand types (scalars and `Vec`, E910) and register classes for the target (E911), and lowers to LLVM inline-asm constraints. Assembly that breaks its operands or options is UB (6.7).

### 12.10 Audit and scope

`yong audit` lists every extern block, `safe` item, `uses none` claim, `unsafe impl` and hand edit to an imported module, and model-authored changes to any of them. Out of scope for 1.0: direct C++ interop (use C shims), dylib plugins that pass Yong types, and Component Model exports (an M4 candidate). An `@yong/sandbox` package (C compiled through wasm2c, RLBox style) may follow 1.0.
