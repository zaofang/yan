# Research index

Every study, design doc and proposal behind the spec, newest first. Studies stay as Claude Docs; this
table is where they are found. Add a line the day a study is written, and fill in its outcome when the
decision lands. Accepted rows are registered in [`spec/rows.toml`](../../spec/rows.toml).

| Date | Study | Outcome |
| --- | --- | --- |
| 2026-11-14 | [Uncased spec row audit](2026-11-14-uncased-row-audit.md) | Audit only; at `2d72926` 634 of 879 rows uncased; 47 exempt (unobservable), 30 merge, 54 deferred (post-1.0) candidates; exempt + merge alone do not reach the 60 percent gate (#1025, #1136); awaiting lead card; no rows |
| 2026-11-13 | [What language communities ask for, ranked for Yan](2026-11-13-forum-wishlist-ranking.md) | Study only; ~35 languages swept, 70 themes; most top mainstream asks already in the spec; ranked gaps: hash-keyed incremental compilation, notations and lenses, live programming on hashes, size arithmetic, rank polymorphism, one-shot effects, tactics; declines specialization, HKT, dependent types, procedural macros; lead cards 2026-11-13: gaps filed as #1119, incremental not pulled ahead, R780/R781 via Yan Meta lab, hot reload gets semantics, all four declines confirmed; no rows |
| 2026-11-13 | [Yan Meta: syntax proposal after forum review](2026-11-13-yan-meta-syntax-proposal.md) | Proposal only; forums show pushback on procedural macros (build time, IDE), demand for checked DSLs; recommends no procedural macros, declared bidirectional notations, LL(1) DSL islands, reader lenses; adopted by lead cards 2026-11-13 (D1 to D6 and all detail cards as recommended), RFC after the freeze; no rows |
| 2026-11-13 | [Yan Meta: user-defined syntax and the macro model](2026-11-13-yan-meta-syntax-extension.md) | Study only; recommends three tiers over a fixed shape reader: `notation` (rule plus template, binding specs, derived printer), delimited LL(1) `grammar` islands, `elab` after R525; conflicts with the 26.2 no-macros non-goal and the 16.6 parse-before-imports rule need a lead decision; no rows |
| 2026-11-11 | [Public-release audit](2026-11-11-public-release-audit.md) | Audit only; no secrets in history, no rewrite needed; blockers: self-hosted runners on fork PRs (#423, #461, #462), hosted runners (#973), lawyer, trademark policy; should: LGPL malachite in the binary, third-party notices, private artifact links; no rows |
| 2026-11-10 | [Infrastructure gaps: what Yan still needs](2026-11-10-infrastructure-gaps.md) | Study only; hosted runners do not start (release, merge log, infra rerun blocked); 18 ranked items before 1.0 (top: GitHub plan, branch ruleset, cargo-deny and pinned Actions, backups, fuzzing and sanitizers, CI box); D1 to D3 open; no rows |
| 2026-11-08 | Solving Yan's engineering and modelling roadblocks | Study only; prior art and experiments for interpreter/native drift, shared ownership analysis, R567 gap, RFC 0051 identity and spans, inferred `decreases`, the `@access` tile model, blocked issues and the optimizer; finds Cranelift `opt_level` is `none`; decided by cards 2026-11-08: R567 reports both raw and after-one-checker-round first-reply pass@1, gate stays on raw; RFC 0033 profile checker and E144 ship before M1, `std.data` after M1 with `derive(Wire)` |
| 2026-11-08 | Yan development laws and phased plans | Accepted by the lead 2026-11-08 ("Apply"); laws DL1 to DL16 in [docs/development-laws.md](../development-laws.md), practices in CLAUDE.md, gate issues under the parent tracking issue; no rows |
| 2026-11-07 | R567 failure-mix study (T18) | Study only; failures of rerun #289 grouped (Sonnet 13, Haiku 32 first-reply losses; 10 Yan-only wrong results with no diagnostic, 7 repaired rejections, 2 unrepaired); proposals D1 to D5: type empty `ex` lists from the parameter, accept positional arguments in the lenient reader, close residual wrapped rejections, three-seed repeat with a paired sign test, one primer line; no syntax row supported; D1 to D5 open; no rows |
| 2026-11-07 | [R567 three-seed repeat (T18 D4, #698)](../reports/2026-11-07-r567-three-seed-repeat.md) | Seeds 2 to 4, Sonnet and Haiku; paired first-reply sign test: Yan Agent loses 2 to 32 (Sonnet) and 10 to 38 (Haiku) discordant pairs, p < 0.001 pooled; gap 10 points vs Python spread 3 to 6; not noise; no rows |
| 2026-11-06 | [Package manager crate (`yan-pkg`) design](../studies/package-manager-crate.md) | Design for #481; open choices A to D; no rows yet |
| 2026-11-05 | [Scientific computing: what ships with Yan and what does not](../studies/scientific-computing.md) | Study only; maps numpy, pandas and polars, jax and numba, scikit-learn, statsmodels and plotting libraries onto what the spec already has (arrays, tables, `grad`, kernels, tiers) and recommends a split: compiler transforms (fusion, `ein`, `grad`, new `vmap`), std vocabulary (adds `DynArray`, `Table[Dyn]`, `std.stats`, `rand.at`, DLPack, a `display` capability), first-party `@yan/linalg`, `fft`, `stats`, `learn`, `optim`, `ode`, `plot`, community above; no run-time JIT; handler-pinned tensor-core determinism; charts as `Wire` records; D1 to D13 open; no rows |
| 2026-11-04 | [Saving and restoring a seeded generator](../studies/rand-state.md) | Study only; forums item F12; recommends `rand.state() -> Option[RandState]` (a defaulted `Rand` operation, `None` for `os`) where `RandState` is the record `{algorithm, seed, position}` counting `next_u64` draws, restore by `seeded(seed, algorithm, at=position)` with O(1) ChaCha8 and jump-ahead PCG seeks; not needed by `@durable` (R416 journals draws); flags the R685 vs 7.7 split-rule wording (D6); D1 to D11 open |
| 2026-11-04 | [Bootstrapping roadmap: a self-hosted Yan compiler](../studies/bootstrapping.md) | Study only; self-hosting conflicts with R215, so recommends keeping Rust for 1.0 and self-hosting after RC in stages over the Yan Core text seam: A dogfood (`.yir` reader/printer, `.yag` printer) before 1.0, then Yan Agent front end, elaborator and checker emitting `.yir` byte-identical to Rust, back end via FFI to LLVM, stage2 = stage3 fixed point and diverse double compiling with the Rust tree as seed. Lead decided 2026-11-04 (cards): D1 to D3, D8, D10, D11 as recommended; D6 LLVM C API plus a Cranelift shim (Cranelift kept); D4 all surfaces at stage B; D5 Yan query engine at stage C; D7 both seeds (Rust and `.yir` snapshot); D9 retire Rust except the Cranelift shim once a `.yir` seed exists; follow-up open on DDC after retirement; no rows |
| 2026-11-04 | [Named TLS profiles](../studies/tls-profiles.md) | Study only; forums item F11; recommends a closed `tls.Profile` enum of `compatible` (the 17.6 default, used when none is named) and `modern` (TLS 1.3 only), a narrowing-only `.only(versions, groups, suites, signatures, pins)` attenuator, no raw or `unsafe_config` (rustls cannot go below `compatible`; legacy peers use FFI), profile facts in `yan audit` and `--diff`, weakening a protected target, contents tighten under the name per 20.2, FIPS post-1.0; D1 to D11 open; no rows |
| 2026-11-04 | [Block-scoped relaxed floats](../studies/relaxed-floats.md) | Study only; forums item F10; recommends lexical `with fast_math:` blocks under `uses nondet` (callees and inlined bodies keep their own semantics, item `@fast_math` = whole-body block, no operators or expression form) and, first, a defined safe relaxed set for item and block alike: contract, reassociate, ignore sign of zero, reciprocal; no no-NaN or no-infinity assumptions, no flush-to-zero on CPUs, no approximate libm; D1 to D11 open; no rows |
| 2026-11-04 | [Package-declared lint sets](../studies/lint-sets.md) | Study only; forums item F9; recommends a literal `lints=Lints(strict, deny, forbid)` field in `package.yag` that only raises severity (no package `allow`, `@expect` stays the only suppression), applied by check, test, build and agent tools, inherited by workspace members, never applied to dependents, no groups or wildcards, weakening is a protected patch target; user analyzers out of 1.0; D1 to D11 open; no rows |
| 2026-11-04 | [Extractors: user-defined match patterns](../studies/extractors.md) | Study only; forums item F8; recommends `@pattern` functions of one `let` parameter, `Option` result partial (covers nothing), other results total, sum results checked exhaustive as a group, no capability but `alloc`, at most one call per scrutinee place per match, lowers to call plus match with no new Core form; D1 to D11 open; no rows |
| 2026-11-04 | [Call-once function values](../studies/call-once.md) | Study only; forums item F6; recommends a `once` marker on function types (call consumes the value, checked by ownership only, no runtime flag), closure kind inferred from the body, `par` to plain to `once` coercion, closures with `@linear` captures are exactly-once, capability rules unchanged; D1 to D11 open; no rows |
| 2026-11-04 | [One-way actor send](../studies/actor-send.md) | Study only; forums item F4; recommends std view `actor.send(h).f(args)` for unit, infallible handlers, returns at admission, shares the per-pair FIFO with `call`, detached after admission, needs no capability; finds lossy overflow policies need `uses nondet` (D8); D1 to D11 open; no rows |
| 2026-11-04 | [Shielded sections for cancel scopes](../studies/shield.md) | Study only; forums item F5; recommends bounded `with shield(d):` (fresh deadline, held cancel delivered after the block, clamped under drain, no capability), D1 to D11 open; no rows |
| 2026-11-04 | Language forums study: ideas for Yan | Study only; Rust, Swift, Go, Kotlin, C#, Zig and TC39 mined; 15 ranked items F1 to F15 (top: numeric-only panic payloads, in-memory `net` test handler, `@expect` already covers Python I4); no rows. Lead decided 2026-11-04: F1, F3, F7, F13, F15 adopted (F1, F3 `@deny`, F7 in [RFC 0031](../../rfcs/0031-forum-batch.md); F13 in docs/architecture.md; F15 in CONTRIBUTING); Python I4 covered by `@expect(code, reason)`; F14 a non-goal (RFC 0031 D4); post-M1 queue F2 (`mem_net`) first, then studies F5, F4, F6, F8; backlog F9 to F12 |
| 2026-11-04 | Python Ideas and PEPs: ideas for Yan | Study only; 15 ranked items I1 to I15 (top: foreign-name fix table, operand values in panic payloads, `Parse` trait), 15 Python debates already covered; no rows. I4 is covered by the existing `@expect(code, reason)` (forums study F3) |
| 2026-11-04 | [Yan build system study](../studies/build-system.md) | Decided (cards): D1 to D10 and Q2, Q3 as recommended except D7 (`yan.lock` is TOML) and Q1 (remote cache holds front-end snapshots too); RFC after M1, no rows. One engine over the query database, no build files, content cache with HTTP remote, sandbox only for C via pinned clang; D1 to D10, first milestone B1 in M1b; polyglot builder is a separate product |
| 2026-11-04 | [R567 benchmark, Sonnet and Haiku](../reports/2026-11-04-r567-sonnet-haiku.md) | Measured; M1 gate not met on either model; lowering candidates and gaps listed |
| 2026-11-04 | [Yan syntax consolidation](../studies/syntax/consolidation.md) | Study and proposal only; family grammar, glyph ownership rule, conflicts C1 to C17, decisions D1 to D19, questions Q1 to Q8, no rows |
| 2026-11-04 | [Yan Query Language syntax study](../studies/syntax/yan-query-language.md) | Study only; recommends nest, keyed joins, group blocks, two-valued optionals, `rule` and `view` items for RFC 0002 D4 |
| 2026-11-04 | [Yan Meta syntax frameworks](../studies/syntax/yan-meta.md) | Study only; recommends a closed notation table, a glyph budget and word twins |
| 2026-11-04 | [Yan Probabilistic syntax and semantics study](../studies/syntax/yan-probabilistic.md) | Study only; recommends typed call-site `given`, index-notation plates, three readings per model |
| 2026-11-04 | [Yan Protocol (`.ypc`) syntax study](../studies/syntax/yan-protocol.md) | Study only; recommends three layers (protocol, `plays` functions, `play` choreographies), mostly lowering to R633 and R634 |
| 2026-11-04 | [Plain Yan (`.yn`) syntax study](../studies/syntax/plain-yan.md) | Study only; five proposals and a mix: one-line guards, `for`/`with` as values, trailing block lambdas, one implicit-subject rule, example tables, typed holes, keywords as method names; no rows |
| 2026-11-04 | [Yan Workflow syntax study](../studies/syntax/workflow.md) | Study only; recommends implicit steps, `first`, `undo`/`commit`, hash-pinned runs |
| 2026-11-04 | [Yan Shell syntax and semantics study](../studies/syntax/yan-shell.md) | Study only; five proposals and a consolidated mix, D1 to D12 open |
| 2026-11-04 | [Yan Law syntax study](../studies/syntax/yan-law.md) | Study only; consolidated proposal and items D1 to D9, no rows |
| 2026-11-04 | [Yan Delegate syntax study](../studies/syntax/yan-delegate.md) | Study only; recommends `want` items, example rows, `like`/`law`/`held`, and ambiguity questions before pinning |
| 2026-11-04 | [Yan Docs (`.ydoc`) syntax study](../studies/syntax/yan-docs.md) | Study only; proposes D1 to D10 for a later RFC |
| 2026-11-04 | [Yan Core (`.yir`) syntax study](../studies/syntax/yan-core.md) | Decided (cards): adopt the mix, RFC after M1; conventions as words `borrow`/`inout`/`sink` (no sigils), dotted intrinsics, `par` header outputs, annotated printing as grammar trivia |
| 2026-11-04 | [Yan Dataflow syntax study](../studies/syntax/yan-dataflow.md) | Study only; five proposals and a mix, no rows |
| 2026-11-04 | [Yan Agent (`.yag`) syntax study](../studies/syntax/yan-agent.md) | Study only; proposes D1 to D9, no rows |
| 2026-11-04 | [Yan Infra syntax study](../studies/syntax/yan-infra.md) | Study only; five axis proposals and a mix, no rows |
| 2026-11-04 | [Yan Schema syntax study](../studies/syntax/yan-schema.md) | Study only; brainstorm of five syntax directions and a consolidated mock, no rows |
| 2026-11-04 | [Should surfaces change semantics?](2026-11-04-surface-semantics.md) | Study only; recommends keeping law 1 for Yan Agent, Yan and Yan Meta |
| 2026-11-04 | [Yan Binary Format syntax study](../studies/syntax/yan-binary-format.md) | Study only; five proposals and a consolidated syntax, no rows |
| 2026-11-04 | [Yan Prompt (.ypm) syntax study](../studies/syntax/yan-prompt.md) | Study only; recommends trust-typed lanes, header-as-contract, strategy clauses, pinned learned slots |
| 2026-11-04 | [Yan Spec (`.ysp`) syntax study](../studies/syntax/yan-spec.md) | Study only; five proposals and a mix (state, steps, laws, worlds, links); no rows |
| 2026-11-03 | Yan Notation: names and real features | RFC 0029: R776 to R786 |
| 2026-11-03 | Yan naming proposal | R774, R775 |
| 2026-11-03 | Jian (Xin) model probe: Sonnet 5.5 on 50 development tasks | R566 probe report, one model family; proposals only, no rows |
| 2026-11-01 | Formal core study: what a mechanized proof would take | RFC 0022 (R680): Lean model under `formal/`, phase 0 now; normative definition open |
| 2026-11-01 | Deep frontier survey: speed and capability | RFCs 0015 to 0018 (R622 to R634) |
| 2026-11-01 | Formal core study: what a mechanized proof would take | RFC 0022 (R680): Lean model under `formal/`, phase 0 now; normative definition open |
| 2026-11-01 | Deep frontier survey: speed and capability | RFCs 0015 to 0018 (R622 to R634) |
| 2026-11-01 | Xin and Wen learnability | RFC 0024 (R655 to R664); D1, the learnability gate, declined |
| 2026-11-01 | Error messages audit | R648 to R651 (RFC 0021) |
| 2026-11-01 | Adoption path and 1.0 launch | Decided: single 1.0 launch, no previews (card, 2026-11-01) |
| 2026-11-01 | Yong packaging and ecosystem | RFC 0020 (R635 to R647) |
| 2026-11-01 | What Yong is and how it fits together | RFC 0014 (R616 to R621): the reference agent loop, built first; RFC 0023 (R665 to R671): `yong review` |
| 2026-11-01 | Theory and compiler frontier survey | RFCs 0010 to 0013 (R605 to R615) |
| 2026-11-01 | Naming study: Yong and its surfaces | R586 |
| 2026-11-01 | What makes Xin and Wen special | R566 to R568; Xin-to-Wen view stays in M3 |
| 2026-11-01 | Yong architecture plan: one core, many surfaces | RFC 0004 (R558 to R565) |
| 2026-11-01 | Surface shortlist, revision 2 | RFC 0002 (R536 to R550), RFC 0003 (R551 to R557) |
| 2026-11-01 | Research track: eight extensions | R522 to R529 (research, post-1.0) |
| 2026-11-01 | Tracking research, proposals and spec revisions | Accepted: spec moved to `spec/`, row registry, `rfcs/`, this index |
| 2026-11-01 | std.iter proposal | R508 to R515 |
| 2026-11-01 | std.math proposal | R500 to R507 |
| 2026-11-01 | std slices and bisect proposal | R494 to R499 |
| 2026-11-01 | std.collections proposal | R488 to R493 |
| 2026-11-01 | std.regex proposal | R482 to R486 |
| 2026-11-01 | Positioning and market gaps | Background for RFC 0002 |
| 2026-11-01 | Niche surfaces | Superseded by the surface shortlist (RFC 0002) |
| 2026-11-01 | Surface mock-ups with names | Superseded by the surface shortlist (RFC 0002) |
| 2026-11-01 | Novelty audit | Background for RFC 0002 |
| 2026-10-31 | Performance frontier survey | R480, R481, R487 |
| 2026-10-31 | Std build plan | R474 to R476 |
| 2026-10-31 | Xin, Wen and Yong comparison | R477, R478 |
| 2026-10-31 | Xin and Wen grammar check | R460 to R466 |
| 2026-10-31 | Xin token rerun | R216 to R218 |
| 2026-10-31 | Xin token-evaluation dry run | R280 to R289 |
| 2026-10-31 | 1.0 read-through | Mechanical fixes; four decisions |
| 2026-10-31 | Futures survey | R450 to R459 (R455 deferred past 1.0) |
| 2026-10-31 | Behavior-oriented concurrency study | Deferred past 1.0 as @yong/boc (R455) |
| 2026-10-31 | Simplicity pass | R440 to R449 |
| 2026-10-31 | Wen ergonomics: real programs | R430 to R439 |
| 2026-10-31 | Xin ambiguity pass | R420 to R429 |
| 2026-10-31 | Distributed compute | R400 to R419 |
| 2026-10-31 | Concurrency pass | R390 to R399 |
| 2026-10-31 | Performance pass | R380 to R389 |
| 2026-10-31 | Consistency audit | R370 to R379 |
| 2026-10-31 | Licensing and governance | R350 to R366 |
| 2026-10-31 | Red-team review | R340 to R349 |
| 2026-10-31 | Internationalization | R330 to R339 |
| 2026-10-31 | Embedded and GPU profiles | R320 to R329 |
| 2026-10-31 | Docs and learning | R310 to R319 |
| 2026-10-31 | Testing and debugging | R300 to R309 |
| 2026-10-31 | Paradigm keepers | R260 to R268 |
| 2026-10-31 | Diagnostics | R290 to R299 |
| 2026-10-31 | Formatter | R240 to R255 |
| 2026-10-31 | Supply-chain security | R190 to R209 |
| 2026-10-31 | Stability and evolution | R170 to R189 |
| 2026-10-31 | Networking and async I/O | R150 to R167 |
| 2026-10-31 | Compile-time evaluation | R130 to R144 |
| 2026-10-31 | Memory and concurrency model | R110 to R124 |
| 2026-10-31 | FFI and ABI | R89 to R105 |
| 2026-10-31 | Wen ergonomics review | R61 to R84 |
| 2026-10-31 | Date and time | R49 to R60 |
| 2026-10-31 | Math package | R28 to R37 |
| 2026-10-31 | Standard library roadmap | R23 to R27 |
| 2026-10-31 | Toolchain design | R15 to R22 |
| 2026-10-31 | Data layer | R38 to R48 |
| 2026-10-31 | Technical implementation spec | SQ2 to SQ33 picks; Python-reference parts superseded by R215 |
| 2026-10-31 | Naming workshop | Yong, Xin, Wen chosen |
| 2026-10-31 | Mo, model-first surface | R210 to R214 (Xin surface rules) |
| 2026-10-31 | Yue, token-minimal surface | Study only (cited in 26.1) |
| 2026-10-31 | Chang, dense ASCII surface | Chang 1; superseded by Chang 2 and RFC 0001 (R516 to R521) |
| 2026-11-01 | Chang 2, kitchen sink | RFC 0001: R516 to R521, R530 to R535 |
| 2026-11-01 | Chang K2 study (positional local names) | R519 |
| 2026-11-01 | Security model: capabilities, FFI and agents | RFC 0026: R672 to R677 |
| 2026-10-30 | 1.0 Development Roadmap | Living plan |
| 2026-11-09 | Self-hosted Yan compiler and formal-proof plan | #851; RFC 0057 (#858): R966 to R970 replace R215; LLVM C API for 1.0, `.yn` surface, S2 after formal phases 1 and 2 |
| 2026-11-09 | Yan pre-1.0 polish plan | #862; RFC 0064: R990, R991 |
| 2026-11-10 | Yan surfaces: technology research | #947; RFC 0065: R994 to R1001 (#948 to #957); rest in #958 |
| 2026-11-11 | Tooling study: borrow and champion | Study only; record, replay, debug, profile, semver, doc, why and diff have no CLI; 10 borrows B1 to B10, 10 champions C1 to C10; D1 (build order: replay with trace diff, no-op review, `yan rewrite`) and D3 (RFCs for all ten) decided 2026-11-11, D2 deferred to #992; tracker #1004; RFC 0066 (C1, draft), RFC 0075 (C10, draft) |
| 2026-11-13 | Spec difficulty audit | #1129; `tools/spec_audit.py`; per-row CSV; no rows |
| 2026-11-14 | Work hierarchy: open issues by queue position | 180 open issues in 10 groups; duplicates, orphans and stale M1 items flagged; no rows |
