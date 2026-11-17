# 0083: `size_of`, `align_of` and the layout and ABI rules

- Status: accepted (lead, 2026-11-17, D1 to D10 as recommended; rows and cases land in the writing PRs, #1519)
- Research: this document (design and code survey, read against main `7c57127`, 2026-11-17); audits
  #1500 (native ownership) and #1502 (capability soundness); RFC 0082 (draft, self-hosted compiler).
  Tracker #449.
- Rows: filled in when accepted

## Summary

The spec uses `size_of[Ptr[T]]` twice (section 3 `Addr` row, 6 CHERI readiness) and never defines it.
Section 5.2 says default layout is unspecified and lists six `@layout` attributes; 5.1 promises niches;
12.3, 12.4 and 12.7 fix what crosses the C boundary. The code has three layouts for one type and they
disagree. This RFC defines `size_of`, `align_of` and `offset_of`, says which layouts a program may
observe, makes layout one query that every backend and tool reads, and stages the work. Each decision
gives the long-term recommendation first and interim steps separately.

## What the code does today (`7c57127`)

| Consumer | Layout it uses | Source |
| --- | --- | --- |
| Stage-0 reflection, `info(T).layout` | byte layout: fields reordered by descending alignment, 1, 2 or 4 byte tag, `Option` null niche for `Str`, `Bytes`, `ByteArray`, `BigInt`, `Path`, `WinPath`, `List` only | `yan-mir/src/layout.rs`, called only from `yan-interp/src/meta.rs:375` |
| Native (Cranelift) and `yan-machine` | slot layout: one 8-byte word per scalar (`bool` and `u8` included), declaration order, a sum is an `i64` tag followed by the fields of every variant side by side, no niche | `yan-mir/src/lower/shape.rs:92` (`Shape::slots`), R922 |
| Value interpreter | none; values are tagged trees; a `@layout(bits(uN))` record is one `uN` | `yan-elab/src/lower/bits.rs` |
| `Map` and `Set` keys | a third description, a schema string parsed at run time | `yan-rt/src/map.rs` (`Layout::parse`), shared by native and machine |

Divergences and gaps:

1. `info(T).layout` reports numbers no backend uses. `class P: a: bool; b: u8` reports size 2, align 1;
   native stores 16 bytes. A two-variant sum reports the larger variant; native stores the sum of both.
   Two representations of one fact (practice 12).
2. No backend applies a niche. `layout.rs` describes one for `Option` over seven shapes; `Map`, `Set`,
   boxed variants, `Ptr`, `bool`, `char` and refined integers, which 5.1 names, have none.
3. `char` is 8 bytes in both layouts. `Str` is one pointer to a runtime object; 5.1 says strings up to
   23 bytes are inline. `Mutex`, `Atomic`, `Arena`, channels and actors are 8-byte handles into
   `yan-rt`; a guard is one pointer. A function value is always two words, where R622 says a closed
   lambda set is a record of its captures. `Ptr` is two words.
4. `@layout`: elaboration accepts `c` and `bits(uN)` and reports every other form as unsupported
   (`decorators.rs:235`). `@layout(c)` only marks the record for the E904 check; nothing computes a C
   layout. Extern calls take integers and `f64` only (`yan-mir/src/ffi.rs`); no record, pointer to
   record or callback crosses.
5. `c_long`, `c_ulong`, `c_size` are 64-bit on every target (`yan-elab/src/ffi.rs:32`), wrong for
   Windows and `wasm32`.
6. The machine checks alignment as "8 bytes at a multiple of 8" (`yan-machine/src/lib.rs:193`), so it
   cannot find a misaligned or out-of-field access inside a record.
7. There is no WASM backend and no target description; every layout assumes 64-bit little-endian.

## Decisions

### D1: three layout classes, and which one a program may observe

Every type instance has a layout: size and alignment in bytes, field offsets, and its valid bit
patterns. Size is a multiple of alignment, alignment is a power of two, and there is no separate
stride. Layouts fall in three classes:

| Class | Members | Layout is a function of |
| --- | --- | --- |
| portable | fixed-width integers and floats, `bool`, `char`, unit; newtypes, arrays, and `@layout(ordered)`, `packed`, `align`, `bits`, `transparent` types whose fields are all portable | the type alone; identical on every target and release |
| target | `@layout(c)` types, `core.ffi` C types, `Ptr`, `Addr`, C function pointers, `Owned`, and the `Option` forms of 12.4; fixed-layout types with a target-class field | the type and the target's C ABI |
| default | everything else: plain records, sums, tuples, closures, std containers, handles | unspecified to programs (D2) |

Recommended, long term: safe code can observe portable and target layouts only. A default layout is
visible only in `ffi` modules and std (L6), where allocators and containers need it. Then no safe
program's output depends on a layout the compiler chose, which is what lets the compiler change
default layouts between releases and keeps program output equal across targets and backends.

Options weighed: (a) observable everywhere, as Rust, simplest and the common expectation, but program
output then depends on target and compiler release; (b) class rule (recommended); (c) no `size_of` in
the language, only on `@layout` types through reflection, which leaves allocators in Yan with nothing.

The class is a structural fact of a type, folded over its fields like `Send`, `Sync` and `Freeze`.
#1502 item 4 found that fold missing for auto traits. One query computes all structural facts from the
declarations (auto traits, trivially copyable, needs drop, layout class, niche), so they cannot
disagree. A generic function that needs a fixed layout states it as a bound; an instantiation is never
the first place the rule is checked (the failure of #1502 item 7).

- Decision: accepted as recommended (lead, 2026-11-17)
- Row:

### D2: default layout stays unspecified to programs and becomes deterministic for implementations

Recommended, long term: keep 5.2's rule and add what it lacks. A default layout MUST be a function of
the monomorphic instance, the target and a layout algorithm version, and of nothing else: not the
host, the optimization level, the backend, `--ub`, thread count or map iteration order. The version is
part of every item hash and of the build record. The algorithm is documented in the implementation,
not the spec, and may change in any release.

This is what RFC 0082 needs: its S3 gate compares printed MIR, which carries layouts, between the Rust
and the Yan compiler, and its S4 fixpoint and cross-host checks need the same layout from every host.
Debug and release sharing one layout means a bug `--ub` finds is the bug native has.

What a program may still rely on for default layout, in L6 code: the scalar table of D4, newtypes
equal to their underlying type, arrays as `n` elements with no gaps, the `Option` forms of 12.4, zero
size for unit and empty records, and that a move is a bitwise copy of `size_of` bytes (6: no `Pin`).

Options weighed: (a) specify the default (declaration order or a fixed sort), predictable but freezes
an ABI and forbids niche and reordering improvements; (b) unspecified and deterministic (recommended);
(c) unspecified and randomized under a flag to catch reliance, useful later as a `--ub` mode, not a
substitute for (b).

- Decision: accepted as recommended (lead, 2026-11-17)
- Row:

### D3: `size_of`, `align_of`, `offset_of`

Recommended, long term: three stage-0 functions in `core.mem`, spelled like `info(T)`:

- `size_of(T) -> int`, `align_of(T) -> int`, `offset_of(T, .field) -> int`.
- The value is the target's, on every backend. It is folded before any backend runs, so the value
  interpreter, `yan-machine`, native and a WASM build for the same target give the same number. The
  interpreter never reports the size of its own value representation.
- Legal anywhere for a portable type; legal anywhere for a target-class type, where the value may
  differ by target (as `Addr` already does); for a default-layout type legal only in `ffi` modules
  and std, and a diagnostic elsewhere. Never legal for a capability type, an unsized `extern type`, or
  `dyn Trait` (use the vtable's size and align, R1004, which MUST equal these functions on the
  concrete type).
- `info(T).layout` is `Option[Layout]` under the same rule, with field offsets; it and the three
  functions are one query.
- The two prose uses of `size_of[Ptr[T]]` become `size_of(Ptr[T])`.
- Yan Agent has the same three names. Python's `sys.getsizeof` gets an N-code alias pointing at
  `size_of`, per the Python-fidelity policy; it is not implemented as a deep size.

Options weighed for spelling: `size_of[T]` as a type-indexed constant (what the prose says today; a
new form with no other user), `size_of[T]()` (a generic call with no value argument), `size_of(T)`
over `Type` (recommended; `info(T)` already works this way).

The names are new and need rows in the names and projection tables (section 16) for each surface.

- Decision: accepted as recommended (lead, 2026-11-17)
- Row:

### D4: scalars and the target description

Portable scalars: `iN`/`uN` occupy the width rounded up to a power-of-two byte count, aligned to
their size, capped at 16; `f32` 4, `f64` 8; `bool` 1 byte, valid values 0 and 1; `char` 4 bytes, valid
values the Unicode scalar values; unit and empty records size 0, align 1. `int` is `i64`.

Recommended, long term: one target description, a data file read by the compiler (and by both
compilers during the bootstrap, RFC 0082 D5): pointer size and alignment, `Addr` width, the C type
table (`c_int`, `c_long`, `c_size` and the rest), the C aggregate rules, and endianness. Every layout
query takes it. `yan-machine` and the interpreter take `--target` and model that target; the default
is the host. `wasm32` is then an entry (4-byte pointers, 8-byte maximum scalar alignment), not a
backend-specific layout.

Yan 0.1.0 targets are little-endian. A big-endian target is a diagnostic at target selection until an
RFC says what byte views of multi-byte scalars mean there.

- Decision: accepted as recommended (lead, 2026-11-17)
- Row:

### D5: the `@layout` attributes

The six attributes of 5.2 stay. This item fills in what 5.2 leaves open; anything not listed is a
diagnostic (practice 13), as today.

- `ordered`: declaration order, each field at its alignment. On a sum: a tag of the smallest unsigned
  integer that holds the variant count, or the one named in `ordered(tag: uN)`, then the variants
  overlaid at one offset; no niche. Portable when every field is.
- `c`: the target C ABI's layout for the equivalent `struct`, `union` or `enum`. On a closed sum, the
  `{uint32 tag, union}` of 12.7.
- `packed(n)` and `align(n)` modify `ordered` or `c` and nothing else. A field of a packed type whose
  alignment exceeds `n` cannot be borrowed, taken by `inout` or matched by reference (a diagnostic);
  it is read and written by value.
- `transparent`: exactly one field of non-zero size. A newtype is always transparent; this is the
  layout half of newtype identity and is guaranteed.
- `bits(uN)`: unchanged (the value is one `uN`); #1189 covers size.
- Tuples have default layout. A fixed tuple layout is a record.
- A capability, a closure, a view or a default-layout type as a field of an `ordered`, `c`, `packed`
  or `bits` type is a diagnostic: a fixed layout cannot contain an unspecified one.

- Decision: accepted as recommended (lead, 2026-11-17)
- Row:

### D6: sums and niches

Recommended, long term: for default layout the compiler overlays variant payloads, picks the tag
width, and fills niches for any sum, not only `Option`. Only the forms of 12.4 are guaranteed. The
valid bit patterns of each scalar (D4, plus non-null for `Ptr`, `Owned`, references and std handles,
and the declared range of a refined integer) are one table. Niche selection reads it, and so does
the `--ub` validity check (an invalid value is already on the UB list), so the tag encoding the
compiler uses and the values the machine rejects cannot drift apart.

Two rules tie this to #1500:

- Layout never carries ownership state. Whether a value, or one field of it, has been moved is the
  static owned fact, with drop flags as ordinary locals (5.3). No bit pattern means "moved from", and
  drop glue never tests for one. Otherwise a niche that uses the null pattern for `None` is
  indistinguishable from a moved-out pointer, and a double drop becomes a silent no-op. The partial
  move fix for finding 3 therefore lives in `yan-opt`'s owned fact per field path and adds no field
  to any layout.
- Under `--ub`, the bytes of a moved-from place are poisoned, so the reads of finding 1 are reported
  by the machine instead of panicking it (finding 2).

Interim: the slot layout of R922 stays until the byte layout lands (D10 stage L2), with sums laid
side by side. It is correct, only large.

- Decision: accepted as recommended (lead, 2026-11-17)
- Row:

### D7: std types, closures, actors, generics

All of these are default layout; the table says what is guaranteed and where the code differs.

| Type | Guaranteed | Recommended representation, long term | Today |
| --- | --- | --- | --- |
| `Str`, `Bytes`, `List[T]` | nothing in memory; `{ptr, len, cap}` at the C profile boundary (12.7) | three words inline, equal to the profile form so an export does not copy; `Str` with the 23-byte inline form of 5.1 | one pointer to a `yan-rt` object |
| `Map`, `Set`, `Deque`, `Heap` | nothing | one pointer to a table block | same |
| `Rc[T]`, `Arc[T]` | pointer size; `Option` of one has no extra space | pointer to count and value | not surveyed |
| `Atomic[T]` | size of `T`, alignment equal to its size (6: lock-free only) | inline | 8-byte handle |
| `Mutex[T]`, `RwLock[T]` | nothing | lock word and `T` inline; legal because a lock is only addressed while borrowed and a borrowed value cannot move | 8-byte handle |
| guards | pointer size, second-class | pointer to the lock | same |
| closures | `{fn, ctx}` or `{fn, ctx, drop}` at the boundary only | R622: a closed lambda set is a tagged record of captures (zero size with no captures and one member); open sets are a boxed pair | always two words |
| `dyn Trait` | two pointers (R1004) | same | not lowered (#1244) |
| `Actor[A]`, channel ends | pointer size | pointer to the mailbox and state block; state is `A`'s default layout | handle |

Recommended, long term: these stop being compiler shapes. When the runtime is ported to Yan (RFC 0082
S3) each is an ordinary std record over `Ptr`, `RawCell` and atomics, and its layout follows from D2
with no special case in the layout query. Until then the handle forms stay.

No in-memory layout crosses a process boundary. Actor messages to a remote node (`uses cluster`,
R400), channels over a transport, record and replay, and caches use the wire schema. A byte view of a
value (`View[u8]`, `.ybf` of R543) is legal only for a portable type with no padding and no pointer.

Generics: layout is per monomorphic instance, computed after monomorphization. There are no
layout-polymorphic functions and no runtime size witnesses except the `dyn` vtable. A type whose size
is unbounded without boxing (5.1) or exceeds the target's address space is a diagnostic.

- Decision: accepted as recommended (lead, 2026-11-17)
- Row:

### D8: the C boundary

`@layout(c)` promises the target C compiler's layout, and 12.6 says the compiler never parses C. Two
things are needed that do not exist.

Recommended, long term:

- **Data layout** comes from the target description (D4): scalar sizes, struct and union rules, enum
  backing type. Bitfields stay behind generated accessors (12.3).
- **Calling convention for aggregates by value** is implemented per supported native target above MIR (RFC 0082
  D7 puts every ABI decision there; LLVM does not classify aggregates for a front end). Printed MIR
  shows how each parameter is passed.
- **Verification**: `yan ffi import` also writes a C file of static assertions (size, alignment and
  each offset of every imported type), compiled by the pinned clang when the package is built. A
  mismatch between the compiler's C layout and the C compiler's is a build error, not a run-time
  corruption. Hand-written extern blocks get the same file from `yan audit`.
- **Callbacks**: a `scoped fn` or `escaping fn` parameter is the `{fn, ctx}` or `{fn, ctx, drop}` of
  12.7, with a generated trampoline in the C convention per signature; never the closure's default
  layout.

Interim: aggregates cross by pointer only; a by-value `@layout(c)` parameter or result is a
diagnostic naming this limit. On a target without that implementation it stays that way, with a generated C shim
for by-value signatures.

Capability rules (#1502, RFC 0079):

- A capability value has no observable layout: no `size_of`, no field of a fixed-layout type, no byte
  view, no crossing except inside a `yan_world` (12.7). This closes layout as a second route for the
  laundering of #1502 item 1.
- Reinterpreting bytes as a value is safe only for the plain-data types of D7. Anything else is an
  `unsafe` operation in an `ffi` module and counts toward `allow_ffi` consent like any other.
- Layout attributes grant nothing and are not ceiling entries. The `yan_abi_v1` surface of a package
  (its `@export` items and their types) is listed by `yan audit`, and a change to a fixed layout of an
  exported type is a breaking change for `yan publish`.

- Decision: accepted as recommended (lead, 2026-11-17)
- Row:

### D9: determinism guarantees, stated once

1. Safe code cannot observe a default layout or an address (D1, D3).
2. A portable layout is the same on every target and in every release.
3. A target-class layout is the target C ABI's, so it agrees with any conforming C compiler.
4. A default layout is a function of instance, target and layout algorithm version (D2).
5. Interpreter, machine and every native backend agree on every `size_of`, `align_of` and
   `offset_of`; the differential law covers them.

- Decision: accepted as recommended (lead, 2026-11-17)
- Row:

### D10: one layout query, and the order of work

Recommended, long term: `yan-mir`'s layout module is the only code that computes sizes, offsets, tags
and niches. `Shape::slots`, reflection, drop glue, the map key schema, DWARF, the machine and the
future LLVM generator read its result. It lands before the canonical MIR text of RFC 0082 D7 is
frozen as an equality gate, since that text prints layouts.

| Stage | Work | Claims | After |
| --- | --- | --- | --- |
| L0 | reflection stops reporting the unused byte layout: `info(T).layout` is absent for default layout; target description file with the host entry | working | lead decision on D1, D3 |
| L1 | `size_of`, `align_of`, `offset_of`; structural type facts query shared with auto traits (#1502 item 4); `ordered`, `transparent`, `packed`, `align` in elaboration and the interpreter; rows and cases | working | L0 |
| L2 | native and machine move from slots to the byte layout (R922 retired for layout); machine is byte-addressed with the validity table; poisoned moved-from bytes (#1500 findings 1, 2) | right | L1, #1500 fixes |
| L3 | overlaid sums and general niches; callgrind figures | fast | L2 |
| L4 | C boundary: `@layout(c)` data layout, aggregates by pointer, callbacks, static assertion file | working | L2, target description |
| L5 | aggregates by value for the supported native targets; `wasm32` entry and `--target` for machine and interpreter | right | L4 |
| L6 | inline `Str`, `List`, `Mutex`, `Atomic`, lambda-set closures as std records | fast | RFC 0082 S3 |

- Decision: accepted as recommended (lead, 2026-11-17)
- Row:

## Spec changes

- 3: `Addr` row wording (`size_of(Ptr[T])`); scalar sizes and valid values (D4).
- 5.1: niche wording made general; the valid-pattern table (D6); `Str` inline form marked as
  representation, not guarantee (D7).
- 5.2: layout classes (D1), determinism of default layout (D2), attribute details (D5), the layout
  functions (D3).
- 6: CHERI readiness wording; poisoned moved-from bytes under `--ub` (D6).
- 10: `info(T).layout` is optional and carries offsets (D3); structural type facts (D1).
- 12.3, 12.5, 12.7: target description, aggregate passing, static assertion file, callbacks,
  capability rules (D8).
- 16: names and projection rows for the three functions.
- 17: `core.mem`.
- 22.2: R922's interim layout retired at L2; target description in the crate table.

Under CASE WITH ROW each row lands with its case: run cases printing `size_of`, `align_of` and
`offset_of` for portable types on all three backends, fail cases for a default-layout `size_of` in
safe code, a capability field in a fixed layout and a borrow of a packed field, and an `ffi` case
with the static assertion file. D2 and D9 item 4 have no program-observable behaviour and need a
lead-approved entry in `spec/row-exemptions.toml` or a toolchain case on printed MIR.
