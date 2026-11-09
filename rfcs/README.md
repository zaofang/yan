# RFCs

An RFC is one batch of spec decisions: what needs one, and the pipeline, are in
[CONTRIBUTING](../CONTRIBUTING.md#what-needs-an-rfc). Research behind an RFC is listed in
[docs/research](../docs/research/README.md). Decisions made before 2026-11-01 have no RFC file; the
research index links their studies and rows.

## Writing one

1. Copy `0000-template.md` to `NNNN-slug.md`, with the next free number from the index below, and open
   a draft PR. If another PR takes the number first, renumber on rebase.
2. Label each proposed decision D1, D2, and so on. Never write an R number or a diagnostic code into a
   draft.
3. The lead decides each item. Record each answer and its date in the file.
4. In the same PR, take row numbers from `uv run python tools/rows.py next N`, add the accepted items
   to `spec/rows.toml`, edit the spec sections, and run `tools/rows.py render` and `check`. New codes
   take the next free code in their section 25 family.
5. Set the status (accepted, rejected, deferred) and merge. Rejected and deferred RFCs are merged too.

## Index

| RFC | Title | Status | Rows |
| --- | --- | --- | --- |
| [0001](0001-yan-meta.md) | Yan Meta, the third surface | accepted | R516 to R535 |
| [0002](0002-niche-surfaces.md) | Niche surfaces | accepted | R536 to R550 |
| [0003](0003-core-gaps-from-surfaces.md) | Yan gaps found by the surface study | accepted | R551 to R557 |
| [0004](0004-architecture.md) | Architecture for one core and many surfaces | accepted | R558 to R565 |
| [0005](0005-std-json.md) | std.json | accepted | R569 to R575 |
| [0006](0006-engine-layering.md) | Where niche surface engines sit | accepted | R576 |
| [0007](0007-std-encoding.md) | std.encoding and `bytes` | accepted | R579 to R585 |
| [0008](0008-std-path.md) | std.path | accepted | R587 to R592 |
| [0009](0009-std-time.md) | std.time | accepted | R593 to R604 |
| [0010](0010-exact-solver.md) | Exact integer solver and guard-aware matches | accepted | R605, R606 |
| [0011](0011-proof-and-test-power.md) | Proof and test power | accepted | R607 to R609 |
| [0012](0012-header-facts.md) | New facts in the header | accepted | R610 to R612 |
| [0013](0013-numerics.md) | Numerics: accuracy lint, reverse mode, sparse index equations | accepted | R613 to R615 |
| [0014](0014-agent-loop.md) | The reference agent loop | accepted | R616 to R621 |
| [0015](0015-zero-cost-guarantees.md) | Guaranteed zero-cost abstractions | accepted | R622 to R624 |
| [0016](0016-data-engines.md) | Data engines | accepted | R625 to R628 |
| [0017](0017-std-algorithms.md) | std algorithms | accepted | R629 to R632 |
| [0018](0018-session-protocols.md) | Session protocols | accepted | R633, R634 |
| [0019](0019-std-system-modules.md) | std.bits, std.rand, std.env, std.log, std.proc and std.cli | accepted | R681 to R695 |
| [0020](0020-packaging-and-ecosystem.md) | Packaging and ecosystem | accepted | R635 to R647 |
| [0022](0022-formal-core.md) | A formal model of the core in Lean 4 | accepted (D1, D3, D4); D2 open | R680, R764, R765 |
| [0026](0026-security-model.md) | Security model: unforgeable handlers, unconfined capabilities, agent confinement | accepted | R672 to R677 |
| [0024](0024-learnability.md) | Learnability for Python and TypeScript developers | accepted | R655 to R664 |
| [0027](0027-loop-invariants.md) | Inferred loop invariants for the bounds solver | accepted | R760 to R763 |
| [0021](0021-error-messages.md) | Error messages: E545, E017 for unsupported constructs, line format, near names | accepted | R648 to R651 |
| [0023](0023-yan-review.md) | `yan review`, a report for agent PRs | accepted | R665 to R671 |
| [0028](0028-drops-and-yan-opt.md) | Ownership facts, drop elaboration and the `yan-opt` contract | accepted | R769 to R773 |
| [0029](0029-yan-meta.md) | Yan Meta, the lab and canary surface | accepted | R776 to R786 |
| [0030](0030-nested-optionals.md) | Nested optionals keep two levels | accepted | R793 to R795 |
| [0031](0031-forum-batch.md) | Numeric panic operands, item-level deny, 7-day cool-off, target-typed variants as a non-goal | accepted | R796 to R799 |
| [0032](0032-std-net-and-mem-net.md) | A first `std.net` slice and the `mem_net` test handler | accepted | R789 to R792 |
| [0033](0033-data-profile.md) | The data profile, a typed literal file in place of TOML | accepted | R826 to R829 |
| [0034](0034-spelling-marks.md) | Spelling marks from lowering to raise, so law 1 keeps written choices | accepted | R837 to R843 |
| [0035](0035-sub-item-docs.md) | Docs on parameters, fields, variants, dict entries and tuple positions | accepted | R844 to R849 |
| [0036](0036-more-spelling-marks.md) | More spelling marks so the remaining law 1 cases keep written choices | accepted | R850 to R855 |
| [0037](0037-remaining-spelling-marks.md) | Spelling marks for the last eight law 1 cases: `+` on strings, N36, `range(0, n)`, tuple assignment, `divmod` and `pow`, `**` | accepted | R857 to R861 |
| [0038](0038-t09-spelling-marks.md) | Spelling marks for the four T09 law 1 cases: hoisted declaration, `View`, cli schema, comprehension, `ex`, tuple parentheses | accepted | R863 to R868 |
| [0039](0039-drops-on-the-failing-edge.md) | Drops on the failing edge of `?`: a drop list on the `?` form in optimized Core | accepted | R871 |
| [0040](0040-non-wire-field-diagnostic.md) | Code, span and text of the diagnostic for a `derive(Wire)` field that is not `Wire` | accepted | R872, R873 |
| [0041](0041-typed-json-csv-decode.md) | Typed json and csv over Wire: tags, shapes, errors, csv rules | accepted | R874 to R881 |
| [0042](0042-panic-operand-fields.md) | Panic operand fields the spec names but does not define | accepted | R882 to R885 |
| [0043](0043-native-par-concurrent.md) | Native `par concurrent`: branches and blocking callees as resumable tasks | accepted | none |
| [0044](0044-manifest-and-lock.md) | Package manifest and yan.lock | accepted | R890 to R896 |
| [0045](0045-language-readings.md) | Language readings assumed by the implementation | accepted | R898 to R907 |
| [0046](0046-std-readings.md) | Std readings assumed by the implementation | accepted | R908 to R914 |
| [0047](0047-toolchain-readings.md) | Toolchain readings assumed by the implementation | accepted | R915 to R924 |
| [0048](0048-residual-wrapped-rejections.md) | Residual wrapped rejections from the R567 failure-mix study | accepted | R927 to R929 |
| [0049](0049-positional-args-lenient.md) | Positional arguments in the lenient reader | accepted | R932 |
| [0050](0050-empty-literal-argument-typing.md) | Empty literals in arguments of an optional parameter | accepted | R933 |
| [0051](0051-reflection-and-user-type-functions.md) | Reflection and user type functions | accepted | staged |
| [0052](0052-access-binding-syntax.md) | Binding syntax for `@access` claims: named operands, one region each | accepted | R952 |
| [0053](0053-package-manager-gaps.md) | Package manager gaps after the directory-index resolver | accepted | R943 to R951 |
| [0054](0054-shared-move-drop-analysis.md) | One dataflow move and drop analysis | accepted | R961 to R965 |
| [0055](0055-kernel-access-verification.md) | `@kernel` functions and E274 body verification | accepted | R953 to R955 |
| [0056](0056-actor-runtime.md) | Actor runtime: spawn, handler calls, mailboxes, supervision, native and Remote on `std.sim` | accepted | R956 to R960 |
| [0057](0057-self-hosted-compiler.md) | Self-hosted compiler for 1.0; Rust as the frozen stage 0 (replaces R215) | accepted | R966 to R970 |
| [0058](0058-kernel-launch.md) | Launching a `@kernel` function: `gpu.launch`, the grid, whole tiles | accepted | R971 to R973 |
| [0060](0060-actor-supervision-surface.md) | Actor supervision: the surface and what stops an actor | draft | |
