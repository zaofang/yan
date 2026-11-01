## 1. System overview

Each surface has its own reader. Everything from the elaborator down is shared:

```
.jian --> Jian reader (+ lenient reader) --\
                                            >-- elaborator --> checked Yong --> optimizer --+--> interpreter (reference, stage 0, checked UB mode)
.wen  --> Wen reader ----------------------+                                               +--> Cranelift (debug builds)
.fei  --> Fei reader ----------------------/                                               +--> LLVM (release builds)
checked Yong --> Wen printer (any item);  checked Yong --> Jian printer (items inside the Jian subset)
checked Yong --> Fei printer (any item, with its spelling notes)
```

The printers are projections: any Yong item prints as Wen or Fei, and any Yong item inside the Jian subset prints as Jian (section 16).

| Name | Role | Files |
| --- | --- | --- |
| Yong (永, "eternal") | core semantics, IR, compiler, the `yong` binary | `.yong` (IR text), `package.jian`, `yong.lock`, `.yong/` (caches, checkpoints, provenance) |
| Jian (简, "concise and clear") | surface written by language models; canonical, explicit, Python-shaped | `.jian` |
| Wen (文, "text, writing") | surface written and read by people; inferred, expressive | `.wen` |
| Fei (斐, "brilliantly patterned") | dense ASCII surface and language-design playground; every form lowers to the shared core (16.6, R516) | `.fei` |

**No language runtime.** No scheduler, collector or implicit reference counting runs unless the program binds a library that provides it. Concurrency, I/O waiting and allocation are capability handlers chosen by the program, and actors are a std package over them (sections 7 and 9).

**Layers.** Depth is additive: each layer adds checked capabilities without changing the meaning of code above it.

| Layer | Adds | In Jian |
| --- | --- | --- |
| L0 | values, functions, collections, control flow, records, sums | yes |
| L1 | generics, traits, refinements, arrays, tables, units, views | yes |
| L2 | capabilities and handlers, typed failure | yes |
| L3 | `par`, `par for`, `par concurrent`, `Rc`, `Arc`, std atomics and locks | yes |
| Contracts | `pre`, `post`, `ex`, `invariant`, `decreases` | yes |
| L4 | arenas and `Ref`, layout control, proof attributes, portable SIMD Vec (R386) | no |
| L5 | stage-0 code (`#for`, `#if`, `#expr`), reflection, `derived` bodies, attribute functions, generators | no (Jian may apply derives and attributes, and declare compile-time parameters) |
| L6 | `unsafe`, `Ptr`, `RawCell`, extern blocks, target SIMD intrinsics over `Vec`, inline assembly | no |

**Library tiers.** `core` needs no allocation and no OS; `alloc` adds collections over the `alloc` capability; `std` adds the OS handlers. Platform `freestanding` links `core` and optionally `alloc` (section 17.1).

**Targets.** M1 MUST support Linux on x86-64 and AArch64 and macOS on AArch64. Windows and WebAssembly follow the LLVM backend in M2. CHERI targets are M4 (section 6.6). Release targets require hardware FMA: x86-64-v3 or ARMv8 (section 11.1).
