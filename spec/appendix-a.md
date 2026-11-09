## Appendix A. Traceability

### A.1 Rows to sections

Every v0.3 change row and resolved SPEC\_QUESTION now lives in the body. `archive/v0.3.md` keeps the original row text.

<!-- rows: generated from spec/rows.toml by tools/rows.py render; do not edit -->

| Rows | Topic | Sections |
| --- | --- | --- |
| R961 | `yan-flow` is the one move, last-use and drop analysis: a generic control-flow graph of basic blocks with edges for branches, loop back edges, `break`, `continue`, `match` arms, and exits for `return`, `fail` and `?`; the checker, the elaborator and drop elaboration each lower their own IR to it (RFC 0054 D1) | 4.2, 5.3 |
| R962 | Facts are keyed by binding identity assigned at definition: a shadowing `let` or a loop re-bind is a new binding and a name is only a lookup into the current scope (RFC 0054 D2) | 4.2, 5.3 |
| R963 | Three facts solved to a fixed point: maybe-moved (a read of a maybe-moved binding is a use after move), live (a last-use move is a read at which the binding is not live, and the witness for the used-again label is the first read found), and maybe-owned with a must-owned companion (a drop flag only where the type has custom `Drop` or is `@linear` and ownership depends on the path); a field taken out of a record leaves it owned with a hole; migrated one client at a time (RFC 0054 D3) | 4.2, 5.3, 5.4, 10.6 |
| R964 | Loops are decided by the graph: a read in a loop body is followed by every read reachable around the back edge; there is no separate loop rule, and a program whose E413 result changes is migrated by the lead (RFC 0054 D4) | 4.2 |
| R965 | `yan-flow` migrates in order: crate, elaborator, checker, `yan-opt` drops, then conformance cases on a `conform/*` PR; each stage deletes the mechanism it replaces (RFC 0054 D5) | 4.2, 5.3 |

<!-- /rows -->

### A.2 Consolidation choices

Where the rows left wording to the consolidation, this draft chose as follows. Each is open to the owner's override.

- **Numbering.** 55 new diagnostic codes were assigned (marked *new* in section 25). Packages and stability take the new E0xx family; stage 0 takes E260 to E270; systems-layer errors take the new E9xx family. Virtual time under `pool` is E710, beside E708, as the time design suggested.
- **Kind `Size` renamed `Nat`**, matching the surfaces' `N: Nat` bounds (R36, R62).
- **`uint` removed** from Yan Agent's base types and the prelude, as R80 listed it among the replaced forms; Yan Agent uses the sized `uint8` to `uint128`.
- **`ffi` modules** are the files under a package's `ffi/` directory, where `yan ffi import` writes them; v0.3 never said how a module is marked.
- **`@forbid(rc)`** (spelled `@no_rc` before R447) now means no `Rc` or `Arc` count operation is reachable, since Yan has no `dup` after R1.
- **`yan` field.** SQ33's `toolchain` field is the `yan = "1.x"` field of R177.
- **API evolution table.** R183 listed the major changes; the minor and patch rows of 20.6, and the "trait method without default" and "parameter added without default" rows, follow Cargo's SemVer chapter.
- **Examples.** The portfolio and VWAP examples now call `http.get_text` from `std.http` and insert the `copy` calls that R1 requires.
- **Section numbers** changed: the memory model (6), FFI (12) and stability (20) became sections, and every cross-reference follows the new numbering.
- **Paradigm keepers (R260 to R268)**, accepted on 2026-10-31 from the paradigm keepers design (https://claude.ai/artifact/UeApdTe98kdRA3d5K1w5hp). Yan Core spells the tail call `become Call`, beside `ret`, and Yan allows `become postfix`. Yan's clause form uses Yan's own match arms (`pattern -> ...`, R370), not the `|` arms of the design sketch, and a function with several parameters matches on their tuple. The codes are E273, E271 and E420 (E142 renumbered by R379). The class and fold views were placed in 18.6, and `view --steps`, `audit --stats` and `audit --policy` in 18.1, where R449 folded the first two. R264 is wording only.
- **Diagnostics (R290 to R299)**, accepted on 2026-10-31 from the diagnostics design (https://claude.ai/artifact/GLRhfwSr8xcyE1gZqSLhg3). The owner chose recovery by marking (R291), the line rendering as MCP content (R295) and the neutral voice (R296); the other rows follow the design. The new codes are E100, E102, E201, E202, E203 and E221.
