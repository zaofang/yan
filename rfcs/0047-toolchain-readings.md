# 0047: Toolchain readings assumed by the implementation

- Status: accepted
- Research: none; starts from the toolchain group of `SPEC_QUESTIONS.md` and issue #409
- Rows: R915 to R924

## Summary

Settles the toolchain group of `SPEC_QUESTIONS.md`: the conformance header layout, the patch protocol, MCP, FFI, native
code and `yan test`. The FFI item "an extern call does not require `uses lib`" is T102's and stays in
`SPEC_QUESTIONS.md`.

## Decisions

### D1: conformance header layout

Headers are the file's leading comment lines (`#` in Yan Agent, `--` in Yan and Yan Core). A `stdout:` block is the
following comment lines with one space stripped (a bare marker is an empty line) up to the next header or the first
non-comment line; several blocks concatenate. `patch/` cases are directories whose format the conformance PR defines;
agents do not edit `conformance/`, so the format is left to that PR.

- Decision: as recommended, taken under the lead's standing rule for choices not worth a card; Antonio, 2026-11-07
- Row: R915

### D2: patch addressing and hashes

A JSON patch's `module` names `<module with dots as slashes>.yag` beside the patch. A plain file patch takes `--module
NAME`, or applies when exactly one other `.yag` file is beside it. The item hash in `base` is `b3:` and the BLAKE3 hex of
the item's canonical text. An edit's search text is matched in the canonical text and the result must parse as that one
item, else P002.

- Decision: as recommended; Antonio, 2026-11-07
- Row: R916

### D3: patch protection, check and oplog

P006 covers a change to or deletion of a `test_` function and any change to the set of `pre`, `post`, `ex`, `invariant`
and `decreases` lines of an existing item; adding a test is allowed. `--check` refuses with P003 on an error whose code
and item the original lacked. The oplog line is `{"file", "before", "after"}` with the module's whole text. Option for the
oplog: (a) whole text; (b) a diff. Recommendation: (a), because replay and undo need no diff algorithm.

- Decision: as recommended; Antonio, 2026-11-07
- Row: R917

### D4: MCP

`yan mcp` serves the modules under the working directory; tool name `a.b` is item `b` of module `a` when `a.yag` exists
and `a/b.yag` does not. `max_tokens` and `budget` count four bytes per token, keeping lines or items in order while they
fit. `autofix` returns the canonical reprint of each differing item as a `replace` patch with base hashes, until a
diagnostic carries a machine fix. `sig` gives a function up to its first body statement and other items whole. A plain
text patch through `check_patch`/`apply_patch` takes `module`; patches are always checked (P003) and protected targets
refuse with P006 until a confirmation token exists.

- Decision: as recommended; Antonio, 2026-11-07
- Row: R918

### D5: FFI modules and types

A module is an ffi module when a directory named `ffi` contains it. `core.ffi` names resolve only there, with LP64
sizes (`c_int` `i32`, `c_long` and `c_ssize` `i64`, `c_size` `u64`, `c_double` `f64`, `c_void` `Unit`). A `MutView[T]`
extern parameter takes `&list`; a list passes as `View[T]` by value. A named function used as a value lowers to a closure.

- Decision: as recommended; Antonio, 2026-11-07
- Row: R919

### D6: FFI diagnostics

E905 covers the default argument promotions (`f32` to `f64`; `i8`, `i16`, `u8`, `u16`, `bool` to `c_int`). E906 names
`setjmp`, `_setjmp`, `sigsetjmp`, `savectx`, `vfork`, `getcontext`. E907: an owned `Str` or `List` crosses only as a
`take` parameter or a result, a `let` parameter must be a `View`. E911 checks register classes of the host target
(x86_64 or aarch64). `@forbid(alloc)` (E900) flags list, set and dict literals, comprehensions and f-strings. Owed:
interior NUL in `c"..."` is E140 (changed: unchecked today) and `@forbid(rc, copy, panic)` is checked (it stops
elaboration today); both are tracked as implementation gaps, not readings.

- Decision: as recommended; Antonio, 2026-11-07
- Row: R920

### D7: native code scope and order

The lead chose on 2026-11-01 to start M1b before the M1 gate passes, against 22.6 rule 1; the interpreter stays the oracle
and native code matches it on every case it runs. The first backend is Cranelift in-process; object files and the system
linker follow once std values have a native layout. Anything outside the covered set is a tool failure (`not supported
yet`), never a program result.

- Decision: as recommended; Antonio, 2026-11-07
- Row: R921

### D8: native layout and memory

Until 5.2's layout pass exists a record or tuple is one machine value per scalar field and a sum is a tag then every
variant's fields; list elements are 8-byte words; a closure is a code address and a capture block; `inout` parameters come
back as extra results; a failing `main` exits 1. The layout is not observable and is replaced by 5.2. `Str` and `List`
blocks are not freed until drop elaboration (5.3 pass 2) lands with `yan-opt`.

- Decision: as recommended; Antonio, 2026-11-07
- Row: R922

### D9: unsafe, lazy compilation, sorts

The workspace forbids `unsafe`; `yan-rt` and `yan-cg-cranelift` deny it and allow it only in `yan_rt::abi`, at the call into
generated code, in the lazy-compile callback and in the AVX-512 sort kernel. `--jit lazy` is the default and `--jit eager`
compiles everything before `main`; a Cranelift error at a first call is exit 2. Calls go through one code-address table,
the debug jump table of 18.4. Integer `sort`, `sort_unstable` and `sorted` without a key use `yan_rt::sort` (vectorized
quicksort with AVX-512F, ipnsort otherwise); equal integers are indistinguishable so every kernel gives the same list.

- Decision: as recommended; Antonio, 2026-11-07
- Row: R923

### D10: `yan test`

`yan test` runs in the package root over `.yag` and `.yn` files under `src/` (else the root); a test id is the module path
with dots then the function name. Only `test_` functions with no value parameters run; `ex`, `bench_` and property tests
(R301) are not run yet. Each test gets the stage-0 budgets (50 million calls, depth 1,000). The test hash is `blake3:` then the
BLAKE3 hex of the function's canonical Yan Core text; test-support items and callee interfaces are not hashed in yet. `--gate`
makes a locked test with a differing hash E014, one missing E015, and a failing approved test fails the run; proposed tests
are reported only. `--format json` prints diagnostic records then one `"kind": "test"` record per test; the line format ends
each failure in `| repro: yan test --only <id>`. A wall-clock timeout for I/O tests waits for tests to declare capabilities.

- Decision: as recommended; Antonio, 2026-11-07
- Row: R924

## Spec changes

24.1 (D1), 18.3 (D2, D3), 21.4 (D4), 12 (D5, D6), 22.1 to 22.4 (D7 to D9), 18.4 (D10). Rows carry the rules;
conformance cases are proposed in a separate PR.
