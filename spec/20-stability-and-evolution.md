## 20. Stability and evolution

The design behind this section is the Yan stability and evolution design (https://claude.ai/artifact/5v5DW2ucybYCy5m37XjDhV).

### 20.1 The compatibility promise

A package that builds and passes its tests on toolchain 1.a MUST build, pass and produce bit-identical output for deterministic code (law 7) on every later 1.b, given the same edition, `yan` API version and `yan.lock` pins. Only the permitted breakage of 20.2 breaks it.

### 20.2 What is frozen

**Frozen at 1.0, additive after:** core semantics (never changed by an edition); the Yan Agent and Yan grammars, elaboration and canonical printing per edition; the lenient reader's acceptance set (it only grows); the std and prelude API except `@preview` items; E and W codes, the JSON diagnostic schema, the patch protocol, MCP tool schemas, `sig` structure, the `package.yag` and `yan.lock` formats; `yan_abi_v1`.

**Not frozen:** Yan IR text and item hashes (salted by toolchain version); performance; message wording; the set of warnings; cache and replay-trace formats (a trace replays on the minor that wrote it); the security defaults of tls, http and crypto.

**Permitted breakage** is a closed list: soundness holes, capability-safety holes, determinism bugs (results now match the checked interpreter), divergence from a normative rule (the spec wins), and the security defaults of tls, http and crypto. Each ships with an ecosystem run (20.7), a warning in an earlier minor where detectable, and a `yan fix` rule where mechanical.

**Diagnostics never break builds.** A minor adds warnings, never errors, except under the permitted-breakage list. A lint becomes an error only at an edition, with its `yan fix` rule.

**Machine contract.** Codes keep their meaning and are never reused; retired codes stay in section 25. JSON diagnostics, the patch protocol, MCP schemas, `sig` output, `package.yag` and `yan.lock` change only by optional fields with a `schema` version.

### 20.3 Toolchain and std API version

`package.yag` requires `yan = "1.x"`. The build needs the maximum over the package and its dependencies and fetches it through `yan toolchain` if the installed one is older. std and prelude items carry `@since`, and items newer than the package's version are invisible to it (E200). std CI rejects a `pub` item without a current `@since` (E012).

### 20.4 Editions

- **Surface-only.** An edition may change surface grammar, keywords, elaboration, canonical printing, lint-to-error promotions, prelude visibility and Unicode tables, never core semantics. Every edition elaborates into one core, so editions link with no glue and item hashes ignore editions. Every edition stays supported.
- **One edition, both surfaces.** `edition` covers every `.yag` and `.yn` file of a package and fixes, as one unit, both grammars, elaboration rules, both printers, the projection table and the Unicode version. The round-trip laws hold within each edition.
- **Naming and process.** Editions are named by the year of the stabilizing toolchain; the 1.0 edition is named at RC. There is at most one edition per three years, and only with a change that passed law 11. Each edition-gated change ships with a machine-applicable `yan fix` rule, old and new conformance tests and its projection entry. `yan fix --edition` applies, rebuilds, tests and stops at the first failure. Before RC the field accepts only `next`.
- **Stale-model tolerance.** Every Yan Agent form an edition retires stays accepted by the lenient reader forever and normalizes to the current form. The law-11 gate for a Yan Agent change measures total tokens per solved task with models that have not seen the new form.

### 20.5 Deprecation

`@deprecated(since, note)` warns at uses (W815). A deprecated function whose body is one expression, or a forwarding type or const alias, is inlined at call sites by `yan fix` and the language server. std removes nothing in 1.x; an edition may hide deprecated std items from code in that edition. Incompatible std redesigns ship as `.v2` modules. Packages remove items only at a major.

### 20.6 API evolution and enforced versions

**Previews.** `@preview("name")` marks unstable items, modules, handlers and next-edition syntax; using one requires `preview = ["name"]` in `package.yag` (E001). There is one release channel. Previews are outside the compatibility promise. A published package at 1.0 or later MUST NOT enable one (E002). A preview stabilizes, is withdrawn or is re-justified within four minors. The `std.evented` handlers are the first. Experimental Yan Meta forms are previews too, enabled per module by `@lab` (16.6, R778).

**API evolution table (normative).** Each row has a conformance witness in `conformance/semver/`.

| Change to a `pub` item | Bump |
| --- | --- |
| remove an item, module or re-export; rename it | major |
| change a parameter's type, convention or position; add a parameter without default | major |
| change the return type, or the sources in its `from` clause | major |
| add a capability to `uses` | major |
| grow a `fail` sum or union | major |
| strengthen `pre` or weaken `post` | major |
| change an `ex` result | major |
| become `nondet` or `@unordered` | major |
| lose Send, Sync or Freeze | major |
| add a sum variant or a record field (without `@non_exhaustive`) | major |
| add a field without default to a `Wire` type | major |
| change an exported C layout or export signature | major |
| bump a public dependency's major | major |
| add a method to a trait without a default | major |
| add an item or module | minor |
| add a sum variant or record field to a `@non_exhaustive` type | minor |
| add a parameter with default; add a trait method with a default | minor |
| remove a capability from `uses`; shrink a `fail` union | minor |
| weaken `pre` or strengthen `post`; add an `ex` | minor |
| gain Send, Sync or Freeze | minor |
| deprecate an item | minor |
| change a body with the same interface hash, contracts and `ex` results | patch |

`@non_exhaustive` on a `pub` sum or record makes variant and field additions minor: outside the package, matches need a wildcard (W811 exempt) and literals and destructuring need `..` (E543).

**`yan semver`.** `yan semver [--base v]` builds each version's API digest from the query database (signatures, `uses`, `fail`, contracts, `ex`, auto traits, layout, exports, public dependencies), skips items whose interface hash is unchanged, and reports findings as diagnostics (E013 for a confirmed break, W014 for a possible one). `yan check` runs it against the last published version.

**Witnesses.** Each type-level break carries a generated Yan Agent program that checks against the old version and fails against the new. A finding without a witness is reported as possible (W014) and does not block publishing.

**Behavioral check.** The base version's `ex` clauses and property tests of `pub` items run against the new implementation in the checked interpreter. A failure requires a major, or `publish --behavior-change item`, which is recorded in release notes and `audit --diff`.

**Enforced versions.** `yan publish` computes the minimum bump from these checks and refuses less (E008); `yan version` writes it.

**Binary evolution.** `yan_abi_v1` is frozen forever; later profiles get new names and coexist. `yan semver` diffs the export set (symbols, signatures, size, alignment, offsets). There is no Yan-to-Yan binary evolution.

### 20.7 Release train

A toolchain minor ships every 12 weeks. Patches go to the latest minor, and security fixes also to the previous minor for 12 weeks; there is no LTS. Each release is gated by conformance on the interpreter and every code generator; an ecosystem run (every registry package's latest version per major plus the golden programs, old toolchain against new); `yan semver` on std against every earlier 1.x; and byte-identical golden outputs.
