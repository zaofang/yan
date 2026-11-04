## 1. System overview

Each surface has its own reader. Everything from the elaborator down is shared:

```
.yag --> Yan Agent reader (+ lenient reader) --\
                                            >-- elaborator --> checked Yan Core --> optimizer --+--> interpreter (reference, stage 0, checked UB mode)
.yn  --> Yan reader ----------------------+                                               +--> Cranelift (debug builds)
.ymt  --> Yan Meta reader ----------------------/                                               +--> LLVM (release builds)
checked Yan Core --> Yan printer (any item);  checked Yan Core --> Yan Agent printer (items inside the Yan Agent subset)
checked Yan Core --> Yan Meta printer (any item, with its spelling notes)
```

The printers are projections: any Yan Core item prints as Yan or Yan Meta, and any Yan Core item inside the Yan Agent subset prints as Yan Agent (section 16).

**One pivot (R558).** Every surface, the niche surfaces of 16.7 included, lowers to the shared high-level IR (`yan-hir`) and is printed from it; no surface depends on anything below it. A form belongs in the HIR only when two or more surfaces spell it; other sugar stays in the surface and is recovered from spelling notes (16.5). Elaboration from HIR to Yan Core and raising from Yan back to HIR are written once, for all surfaces (16.4, R559). An item that runs on a niche surface engine (16.7, R568) enters the HIR as its boundary signature only, as an extern item; the engine is runtime code, not part of the surface crate (R576).

| Name | Role | Files |
| --- | --- | --- |
| Yan (言, "word, speech") | the project, toolchain and core semantics; the `yan` binary (crates `yan-*`) | `package.yag`, `yan.lock`, `.yan/` (caches, checkpoints, provenance) |
| Yan Core (言核, "core"; internal) | the shared IR and its text form | `.yir` (IR text) |
| Yan Agent (言信; 言而有信, "words that keep faith") | surface written by language models; canonical, explicit, Python-shaped | `.yag` |
| Yan (言文; 言文一致, "speech and writing unified"), plain Yan where it needs telling apart | surface written and read by people; inferred, expressive | `.yn` |
| Yan Meta (言元; 元语言, "metalanguage") | dense ASCII surface and language-design playground; every form lowers to the shared core (16.6, R516) | `.ymt` |

**Naming rule (R774).** Every other surface is "Yan" plus a plain English job word, with a `.y` extension of 2 to 4 letters and a Chinese name of 言 plus one character that forms or echoes no negative phrase in either order. A surface's name and extension are used only once it ships.

**No language runtime.** No scheduler, collector or implicit reference counting runs unless the program binds a library that provides it. Concurrency, I/O waiting and allocation are capability handlers chosen by the program, and actors are a std package over them (sections 7 and 9).

**Layers.** Depth is additive: each layer adds checked capabilities without changing the meaning of code above it.

| Layer | Adds | In Yan Agent |
| --- | --- | --- |
| L0 | values, functions, collections, control flow, records, sums | yes |
| L1 | generics, traits, refinements, arrays, tables, units, views | yes |
| L2 | capabilities and handlers, typed failure | yes |
| L3 | `par`, `par for`, `par concurrent`, `Rc`, `Arc`, std atomics and locks | yes |
| Contracts | `pre`, `post`, `ex`, `invariant`, `decreases` | yes |
| L4 | arenas and `Ref`, layout control, proof attributes, portable SIMD Vec (R386) | no |
| L5 | stage-0 code (`#for`, `#if`, `#expr`), reflection, `derived` bodies, attribute functions, generators | no (Yan Agent may apply derives and attributes, and declare compile-time parameters) |
| L6 | `unsafe`, `Ptr`, `RawCell`, extern blocks, target SIMD intrinsics over `Vec`, inline assembly | no |

**Library tiers.** `core` needs no allocation and no OS; `alloc` adds collections over the `alloc` capability; `std` adds the OS handlers. Platform `freestanding` links `core` and optionally `alloc` (section 17.1).

**Targets.** M1 MUST support Linux on x86-64 and AArch64 and macOS on AArch64. Windows and WebAssembly follow the LLVM backend in M2. CHERI targets are M4 (section 6.6). Release targets require hardware FMA: x86-64-v3 or ARMv8 (section 11.1).
