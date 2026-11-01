## 1. System overview

Each surface has its own reader. Everything from the elaborator down is shared:

```
.xin --> Xin reader (+ lenient reader) --\
                                            >-- elaborator --> checked Yong --> optimizer --+--> interpreter (reference, stage 0, checked UB mode)
.wen  --> Wen reader ----------------------+                                               +--> Cranelift (debug builds)
.chang  --> Chang reader ----------------------/                                               +--> LLVM (release builds)
checked Yong --> Wen printer (any item);  checked Yong --> Xin printer (items inside the Xin subset)
checked Yong --> Chang printer (any item, with its spelling notes)
```

The printers are projections: any Yong item prints as Wen or Chang, and any Yong item inside the Xin subset prints as Xin (section 16).

**One pivot (R558).** Every surface, the niche surfaces of 16.7 included, lowers to the shared high-level IR (`yong-hir`) and is printed from it; no surface depends on anything below it. A form belongs in the HIR only when two or more surfaces spell it; other sugar stays in the surface and is recovered from spelling notes (16.5). Elaboration from HIR to Yong and raising from Yong back to HIR are written once, for all surfaces (16.4, R559). An item that runs on a niche surface engine (16.7, R568) enters the HIR as its boundary signature only, as an extern item; the engine is runtime code, not part of the surface crate (R576).

| Name | Role | Files |
| --- | --- | --- |
| Yong (永, "eternal") | core semantics, IR, compiler, the `yong` binary | `.yong` (IR text), `package.xin`, `yong.lock`, `.yong/` (caches, checkpoints, provenance) |
| Xin (信, "trust"; 兰亭集序 信可乐也) | surface written by language models; canonical, explicit, Python-shaped | `.xin` |
| Wen (文, "text, writing") | surface written and read by people; inferred, expressive | `.wen` |
| Chang (畅, "unhindered"; 兰亭集序 畅叙幽情) | dense ASCII surface and language-design playground; every form lowers to the shared core (16.6, R516) | `.chang` |

**No language runtime.** No scheduler, collector or implicit reference counting runs unless the program binds a library that provides it. Concurrency, I/O waiting and allocation are capability handlers chosen by the program, and actors are a std package over them (sections 7 and 9).

**Layers.** Depth is additive: each layer adds checked capabilities without changing the meaning of code above it.

| Layer | Adds | In Xin |
| --- | --- | --- |
| L0 | values, functions, collections, control flow, records, sums | yes |
| L1 | generics, traits, refinements, arrays, tables, units, views | yes |
| L2 | capabilities and handlers, typed failure | yes |
| L3 | `par`, `par for`, `par concurrent`, `Rc`, `Arc`, std atomics and locks | yes |
| Contracts | `pre`, `post`, `ex`, `invariant`, `decreases` | yes |
| L4 | arenas and `Ref`, layout control, proof attributes, portable SIMD Vec (R386) | no |
| L5 | stage-0 code (`#for`, `#if`, `#expr`), reflection, `derived` bodies, attribute functions, generators | no (Xin may apply derives and attributes, and declare compile-time parameters) |
| L6 | `unsafe`, `Ptr`, `RawCell`, extern blocks, target SIMD intrinsics over `Vec`, inline assembly | no |

**Library tiers.** `core` needs no allocation and no OS; `alloc` adds collections over the `alloc` capability; `std` adds the OS handlers. Platform `freestanding` links `core` and optionally `alloc` (section 17.1).

**Targets.** M1 MUST support Linux on x86-64 and AArch64 and macOS on AArch64. Windows and WebAssembly follow the LLVM backend in M2. CHERI targets are M4 (section 6.6). Release targets require hardware FMA: x86-64-v3 or ARMv8 (section 11.1).
