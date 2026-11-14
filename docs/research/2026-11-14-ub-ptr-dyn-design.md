# Design: `run --ub`, `Ptr`, `dyn`, `Arena`/`Ref` and the `every_production` lowerings

Source: issue #1182 (remaining M3 failures after #1226, #1229), read against main 2aefcb1 on 2026-11-14. Design only; nothing here is implemented. No new rows or codes are assigned (spec freeze, #1025). Items are labelled D1 to D9; rows a design needs are named as gaps, not numbered.

Cases covered:

| Bucket | Cases | Root cause on main |
|---|---|---|
| `run --ub` | `ub/data_race`, `ub/dead_allocation`, `ub/invalid_bool`, `ub/offset_outside_allocation`, `ub/one_past_end_is_defined` | no `--ub` flag; `Ptr` has only address ops (`yan-interp/src/ptr.rs`, `yan-std/lib/core/ptr.yag`); no `Ptr.of`, `offset`, `cast`, `read`, `write`, `as_ptr`; `unsafe` outside `ffi/` is E901 |
| E017 | `yn/fail/caps/e307_dynamic_rand` | `yan-hir/src/raise.rs` rejects `dyn` types; E307 has no check |
| E017 | `yn/fail/ownership/e414_ref_escapes_arena` | no `Arena` or `Ref[T]` anywhere in the crates |
| E017 | `yn/pass/syntax/every_production` | four yn constructs not lowered: `use ... as`, positional-only `/` in record fields, `..` in constructor patterns, `is` as an expression outside `if` |

## Contradictions found

These block the `ub/` cases and are spec-surface, so they are the lead's to resolve (the freeze allows contradiction fixes).

1. **`unsafe` outside `ffi/`.** Spec 12 makes `unsafe` legal only in `ffi` modules and std (E901, enforced in `yan-elab/src/lower/stmt_form.rs`). Every `conformance/ub/` case uses `unsafe` and none lives under an `ffi/` directory, so all five get E901 before they run. By 6.7 only unsafe code can cause UB, so every `ub/` case is unsafe code by construction.
2. **`Ptr` outside `unsafe`.** Spec 3 ("Rules on the grammar") says `Ptr` is legal only inside `unsafe`. Conformance A11 and 6.6 treat building a pointer (`as_ptr`, `Ptr.of`, `offset`, `cast`, `addr`, `with_addr`) as safe and only `read`/`write` as unsafe; the cases build pointers and return `Ptr[int]` outside `unsafe`.
3. **`run --ub` against `test --ub`.** 6.9 names only `yan test --ub`; A13 and `tools/conform.py` use `yan run --ub`. Not a contradiction (A13 is an assumption), but the spec should name both once the freeze lifts.
4. **Offset UB in safe code.** 6.7 item 3 makes an out-of-allocation offset UB, A11 makes `offset` safe, and 6.7 says only unsafe code causes UB. D4 reconciles them without a spec change.

Recommendation (the decision card): keep E901 exactly as it is; extend A9 so a case under `conformance/ub/` is checked as an `ffi` module; fix the spec 3 rule to the A11 reading ("dereferencing a `Ptr`, reading `Uninit`, `asm` and `RawCell` access are legal only inside `unsafe`; a `Ptr` may be built and passed wherever `unsafe` is permitted"). This keeps the audit boundary (`ffi/` plus `allow_ffi` consent) intact and changes no row's meaning. Rejected: relaxing E901 for the root package (widens where unsafe code can hide; every package's supply-chain audit gets weaker); permitting `unsafe` only under `--ub` (static semantics would depend on run mode).

## D1. The checked machine executes MIR over abstract bytes

**Decision.** A new crate `yan-machine` executes `yan-mir` bodies on the 6.1 abstract machine: allocations with base, size, alignment, liveness; bytes `Uninit | Init(u8, provenance fragment)`; pointers `(addr, Option<(alloc id, tag)>)`. Layouts come from `yan-mir/src/layout.rs`, the same ones Cranelift uses (law 12: layout is computed once). Drops come from MIR drop elaboration (RFC 0028), so liveness is the same fact native code uses.

**Why not the value interpreter.** `yan-interp` holds `Value` trees (`Vec<Value>` lists, `Rc` sharing, no drops). Giving it pointer-reachable memory needs address-taken promotion of locals and of list buffers, a second encoding of layouts and a second source of drop points; each is a second representation of a fact the MIR already has, which law 12 rejects. Running MIR also means `--ub` checks exactly the program native code runs, and the machine becomes a backend-independent oracle for MIR lowering.

**How it fits 6.1.** "The reference interpreter's checked mode" is the machine, reached through the interpreter's CLI paths. Executors after D1:

| Executor | Input | Memory | Used by |
|---|---|---|---|
| `yan-interp` | checked Yan | `Value` trees | `yan run` on safe programs, stage 0 (until stage 0 needs layout) |
| `yan-machine` | MIR | abstract bytes | `yan run` on programs that touch raw memory (checks off), `yan run --ub`, `yan test --ub` (checks on) |
| Cranelift | MIR | native | `--backend cranelift` |

`yan run` picks the machine when the program's MIR contains a raw-memory operation (`Ptr` read/write, `Uninit`, `RawCell`, atomics on raw memory); that choice is made from the MIR, not by name. A construct MIR does not lower gets the existing unsupported diagnostic (E017, R649), never a fallback (law 13). Differential law: on every safe `run` case the machine and `yan-interp` agree (added to `tools.difftest`).

**Reports.** A check failure prints `ub(N): <message>` on stderr (A13), N the 6.7 item, plus the allocation and the two accesses involved, and exits 101 (abort, as a panic). A `yan replay` trace (6.9) follows when replay exists (#1004 C-items); not needed for the cases.

## D2. `Ptr` semantics and std surface

Std (`yan-std/lib/core/ptr.yag`, intrinsics through the registry, no manual rows):

| Operation | Safe | Meaning |
|---|---|---|
| `Ptr.of(&place) -> Ptr[T]` | yes | pointer to a place; its provenance is the place's allocation and tag (6.6) |
| `xs.as_ptr() -> Ptr[T]` | yes | pointer to element 0 of a list's buffer |
| `p.offset(n: int) -> Ptr[T]` | yes | address plus `n * size_of[T]`, provenance kept (D4) |
| `p.cast[U]() -> Ptr[U]` | yes | same address and provenance |
| `p.read() -> T` | unsafe | typed load; validity checked under `--ub` |
| `p.write(v: T)` | unsafe | typed store |

`&place` is a place operand, legal only as the argument of `Ptr.of` (no first-class references in Yan; views stay second-class). Taking `&x` makes `x` address-taken: MIR gives it a stack slot instead of a register (the same rule Cranelift needs). `Ptr[T]` is `Copy`, never null (6.6), and is Yan-only (L6): Yan Agent elaboration rejects it as today.

**Native lowering.** `Ptr[T]` is a machine pointer; `offset` is `iadd` with a scaled index; `cast` is nothing; `read`/`write` are typed `load`/`store` with the layout's alignment; `as_ptr` reads the data pointer of the `yan-rt` list block. No checks natively (sanitizers are M2, 6.9).

**Value interpreter.** Unchanged: it never sees a raw-memory program (D1 dispatch). The address ops already in `ptr.rs` stay for safe code.

## D3. The `--ub` checks needed by the cases

All are machine checks, off unless `--ub`:

- **Liveness (item 2).** Every allocation has a live bit; MIR `Drop` of a list frees its buffer allocation; an access through provenance naming a dead allocation is `ub(2)`. `dead_allocation`: `xs` is dropped when `make` returns, `p.read()` hits the dead buffer. Depends on MIR drop elaboration for `List` (RFC 0028); if MIR still leaks lists for this case, that is the first task.
- **Bounds (item 3).** See D4.
- **Validity (item 6).** A typed read decodes the bytes with the layout; `bool` accepts 0 and 1, `char` the scalar values, enums their tags, `Ptr` non-null, refinements their predicate. `invalid_bool`: a `u8` slot holding 2 read as `bool` is `ub(6)`.
- **Races (item 1).** D5.

Items 4, 5, 7, 8, 9, 15, 16 (alignment, Tree Borrows, immutable memory, moved-from places, forgery, `Atomic.wait`, `pre` of `unsafe fn`) are the rest of 6.9's checker; they are a numbered follow-up (task T12), not needed for the five cases.

## D4. Offsets: safe to compute, checked at the access

`offset` never traps. The machine records, per pointer value, whether its derivation ever left `[base, base + size]` (one past the end is inside). An access through such a pointer reports `ub(3)` even if the address is back in bounds; an access through an in-bounds pointer at the end address reports `ub(2)` as out of provenance. This keeps 6.7's "only unsafe code can cause UB" (the unsafe access is the act; item 3 names its cause), matches A11, and gives:

- `offset_outside_allocation`: `as_ptr().offset(4)` on 3 elements is poisoned; the read reports `ub(3)`.
- `one_past_end_is_defined`: `offset(3)` is one past the end, not poisoned; `offset(-1)` reads element 2; clean.

Native `offset` is a plain add, so the poison bit costs nothing outside the machine. LLVM must then not use `getelementptr inbounds` for `Ptr.offset` (R383 lists no `inbounds` fact; nothing to change).

## D5. Race detection: vector clocks over `par` arms

Each `par` arm runs as a logical thread with its own vector clock, forked from the parent at entry and joined at exit; channel, mutex and atomic release/acquire edges join clocks. Every allocation byte range keeps the last write clock and read clocks (FastTrack). Two accesses to overlapping bytes, at least one a write, neither happening before the other, is `ub(1)`. Detection does not depend on the schedule, so it is deterministic even though the machine runs arms one at a time; `data_race` reports `ub(1)` on the second write. Store-buffer weak memory and DPOR exploration (6.9) build on the same clocks later.

## D6. Capability safety

- `unsafe` stays confined to `ffi/` modules and std (E901 unchanged); `allow_ffi` stays the consent. `Ptr` building is safe but useless without `unsafe`, so safe packages gain no authority.
- The machine runs under the same handlers as the interpreter; `--ub` grants nothing. A capability value stored to raw memory keeps a capability provenance kind; reading one from bytes that were not written as that capability is item 9 (T12).
- Stage 0 keeps rejecting `unsafe` and `Ptr` (10.4). `yan mcp` and `yan agent` may expose `--ub`; it only adds checks.

## D7. `dyn`: the check now, the lowering after rows

The E307 case needs only a static check: yn must lower `dyn Path` types and `with name = dyn e` into checked Yan (the core AST, parser and printer already have `Type::Dyn`), and the checker rejects `dyn` on the five compile-time-resolved capabilities (`alloc`, `rand`, `time`, `sched`, `fault`) with E307 at the `dyn` keyword (7:15 in the case). The check runs in the elaborator before HIR raise, so the case never reaches `raise.rs`'s E017.

The lowering of `dyn Trait` values is underspecified: the spec has no object-safety rules, no representation (by-value `dyn` needs a box or an allocator) and no vtable layout. Recommended design for a later RFC: `dyn Trait` is an owned pair (boxed data through the bound `alloc`, vtable pointer); the vtable holds drop, size, align and the trait's methods in declaration order, generated per (type, trait) at monomorphization; object safety as Rust's (no generic methods, no `Self` by value outside `where Self: Sized`). The value interpreter already dispatches by runtime type (`Callee::Dyn` in `compile.rs`), so it needs only the coercion. **Rows needed:** dyn representation and object safety (spec 10). Blocked by the freeze; this does not unblock M1.

## D8. `Arena` and `Ref[T]`

Rows exist (R441; 4.7, 5.5), so this is implementable now.

- **Static (case `e414_ref_escapes_arena`).** yn lowers `Ref[T]`; std declares `Arena` in `core`/`alloc` with `Arena()`, `alloc(let self, v: T) -> Ref[T] from arena`, `allocator()`. The `from arena` origin is one more origin in the elaborator's existing second-class escape check (`yan-elab/src/lower/calls.rs`, which already emits E406/E414 for views): a returned `Ref` whose origin is a local arena is E414. One escape analysis for views and refs, not a second one (law 12).
- **Runtime.** Value interpreter: `Arena` is a slot vector with a generation, `Ref` is (arena, index); dropping the arena bumps the generation. Native: `Arena` is a `yan-rt` bump allocator over a chunk list; `Ref` is a pointer; dropping the arena frees all chunks. Machine: each chunk is an allocation, so a `Ref` reached through unsafe code after the arena dies is `ub(2)`.

## D9. `every_production`: four yn lowerings

All in `yan-yn/src/lower.rs` (not one of the three frozen lowering files):

1. `use std.text as text`: an import alias, lowered to the core `use` with a local name.
2. Positional-only `/` in record types (`{ x: float, y: float, / }`) and parameters (`pos(a, b, /, c = 1)`): fields and parameters before `/` cannot be named at call or construction sites; lowered as the core positional marker.
3. `Rect(..)` in constructor patterns: rest of fields ignored, lowered to wildcards per field from the constructor's declaration.
4. `r is Ok(_)` as a `bool` expression outside `if`: lowered to `match r { Ok(_) -> true, _ -> false }` with no bindings escaping (bindings in a non-`if` `is` are an error per 15.2).

## Implementation plan

Ordered; each task is one PR, interpreter and native outputs attached where it executes code (practice 1), its case flipping from fail to pass.

| # | Task | Depends on | Closes |
|---|---|---|---|
| T1 | yn `use ... as` lowering | none | part of `every_production` |
| T2 | yn positional-only `/` in records and parameters | none | part |
| T3 | yn `..` in constructor patterns | none | part |
| T4 | yn `is` as an expression outside `if` | none | `every_production` (with T1 to T3) |
| T5 | yn `dyn` types and `with = dyn`; E307 for the five capabilities | none | `e307_dynamic_rand` |
| T6 | `Arena`, `Ref[T]` declarations and the `from arena` origin in the escape check | none | `e414_ref_escapes_arena` |
| T7 | `Ptr` surface: `&place`, `Ptr.of`, `offset`, `cast`, `read`, `write`, `as_ptr` in yn and std | decision card | none (cases still need `--ub`) |
| T8 | MIR and Cranelift: address-taken locals, `Ptr` ops, list drop elaboration for the cases | T7 | native run of the `ub/` programs without checks |
| T9 | `yan-machine`: abstract-byte MIR executor, `yan run` dispatch for raw-memory programs, difftest against `yan-interp` | T8 | `one_past_end_is_defined` once T10 adds the flag |
| T10 | `--ub` on `run` and `test`: liveness, bounds poison, validity, `ub(N)` reports | T9 | `dead_allocation`, `invalid_bool`, `offset_outside_allocation`, `one_past_end_is_defined` |
| T11 | Vector-clock race detection over `par` arms | T10 | `data_race` |
| T12 | Rest of the 6.9 checker: alignment, Tree Borrows, immutability, moved-from, forgery, `Atomic.wait`, `pre` | T10 | gap list |
| T13 | `Arena`/`Ref` runtime on interpreter, native and machine | T6, T9 for the machine part | none (new run cases proposed via `conform/*`) |
| T14 | `dyn` lowering (vtables, object safety) | rows after the freeze lifts | none |

T1 to T6 are independent and can run in parallel on disjoint files (yn lowering vs elaborator checks). Wrong code/position (6) and run failures (3) in #1182 are out of scope here.
