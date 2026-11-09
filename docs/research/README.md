# Research index

Every study, design doc and proposal behind the spec, newest first. Studies stay as Claude Docs; this
table is where they are found. Add a line the day a study is written, and fill in its outcome when the
decision lands. Accepted rows are registered in [`spec/rows.toml`](../../spec/rows.toml).

| Date | Study | Outcome |
| --- | --- | --- |
| 2026-11-08 | [Solving Yan's engineering and modelling roadblocks](https://claude.ai/artifact/728U7arS1GVNBzdWy5T3jq) | Study only; prior art and experiments for interpreter/native drift, shared ownership analysis, R567 gap, RFC 0051 identity and spans, inferred `decreases`, the `@access` tile model, blocked issues and the optimizer; finds Cranelift `opt_level` is `none`; decided by cards 2026-11-08: R567 reports both raw and after-one-checker-round first-reply pass@1, gate stays on raw; RFC 0033 profile checker and E144 ship before M1, `std.data` after M1 with `derive(Wire)` |
| 2026-11-08 | [Yan development laws and phased plans](https://claude.ai/artifact/RaRsJ6bHuLRe1MQyNet4qq) | Accepted by the lead 2026-11-08 ("Apply"); laws DL1 to DL16 in [docs/development-laws.md](../development-laws.md), practices in CLAUDE.md, gate issues under the parent tracking issue; no rows |
| 2026-11-07 | [R567 failure-mix study (T18)](https://claude.ai/artifact/1tUqnYo9Mds7wDV2JBruKs) | Study only; failures of rerun #289 grouped (Sonnet 13, Haiku 32 first-reply losses; 10 Yan-only wrong results with no diagnostic, 7 repaired rejections, 2 unrepaired); proposals D1 to D5: type empty `ex` lists from the parameter, accept positional arguments in the lenient reader, close residual wrapped rejections, three-seed repeat with a paired sign test, one primer line; no syntax row supported; D1 to D5 open; no rows |
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
| 2026-11-04 | [Language forums study: ideas for Yan](https://claude.ai/artifact/E8xVSw3fhTwuxFu38DqrtT) | Study only; Rust, Swift, Go, Kotlin, C#, Zig and TC39 mined; 15 ranked items F1 to F15 (top: numeric-only panic payloads, in-memory `net` test handler, `@expect` already covers Python I4); no rows. Lead decided 2026-11-04: F1, F3, F7, F13, F15 adopted (F1, F3 `@deny`, F7 in [RFC 0031](../../rfcs/0031-forum-batch.md); F13 in docs/architecture.md; F15 in CONTRIBUTING); Python I4 covered by `@expect(code, reason)`; F14 a non-goal (RFC 0031 D4); post-M1 queue F2 (`mem_net`) first, then studies F5, F4, F6, F8; backlog F9 to F12 |
| 2026-11-04 | [Python Ideas and PEPs: ideas for Yan](https://claude.ai/artifact/R1t3UGwQ9VKSaNbGjHPfVQ) | Study only; 15 ranked items I1 to I15 (top: foreign-name fix table, operand values in panic payloads, `Parse` trait), 15 Python debates already covered; no rows. I4 is covered by the existing `@expect(code, reason)` (forums study F3) |
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
| 2026-11-03 | [Yan Notation: names and real features](https://claude.ai/artifact/XwShntWVWkEzcMhkxbswiT) | RFC 0029: R776 to R786 |
| 2026-11-03 | [Yan naming proposal](https://claude.ai/artifact/BHBRo7tdX5b3H4uZbQGs6o) | R774, R775 |
| 2026-11-03 | [Jian (Xin) model probe: Sonnet 5.5 on 50 development tasks](https://claude.ai/artifact/8bZgdYmkeds1yWouV8Ryxs) | R566 probe report, one model family; proposals only, no rows |
| 2026-11-01 | [Formal core study: what a mechanized proof would take](https://claude.ai/artifact/6PKpzbDykX73NCVzr8nU8y) | RFC 0022 (R680): Lean model under `formal/`, phase 0 now; normative definition open |
| 2026-11-01 | [Deep frontier survey: speed and capability](https://claude.ai/artifact/U5Rg6qH5x2KMHX5dAfkh1H) | RFCs 0015 to 0018 (R622 to R634) |
| 2026-11-01 | [Formal core study: what a mechanized proof would take](https://claude.ai/artifact/6PKpzbDykX73NCVzr8nU8y) | RFC 0022 (R680): Lean model under `formal/`, phase 0 now; normative definition open |
| 2026-11-01 | [Deep frontier survey: speed and capability](https://claude.ai/artifact/U5Rg6qH5x2KMHX5dAfkh1H) | RFCs 0015 to 0018 (R622 to R634) |
| 2026-11-01 | [Xin and Wen learnability](https://claude.ai/artifact/SWCzhtopJtJjpSFa2MBY1x) | RFC 0024 (R655 to R664); D1, the learnability gate, declined |
| 2026-11-01 | [Error messages audit](https://claude.ai/artifact/6catGcMdztyszRmvJLoiRu) | R648 to R651 (RFC 0021) |
| 2026-11-01 | [Adoption path and 1.0 launch](https://claude.ai/artifact/YFzwDZ97bftr7Vq2KmLUGD) | Decided: single 1.0 launch, no previews (card, 2026-11-01) |
| 2026-11-01 | [Yong packaging and ecosystem](https://claude.ai/artifact/NHdZaQ9Q6X5tFxz1xh9BQC) | RFC 0020 (R635 to R647) |
| 2026-11-01 | [What Yong is and how it fits together](https://claude.ai/artifact/HtZ81sCkbF8LnYJRNrGeVU) | RFC 0014 (R616 to R621): the reference agent loop, built first; RFC 0023 (R665 to R671): `yong review` |
| 2026-11-01 | [Theory and compiler frontier survey](https://claude.ai/artifact/YNj73yYvBWphMVzAdNLABE) | RFCs 0010 to 0013 (R605 to R615) |
| 2026-11-01 | [Naming study: Yong and its surfaces](https://claude.ai/artifact/8zD7T9QnYtNUsbF96cLHNN) | R586 |
| 2026-11-01 | [What makes Xin and Wen special](https://claude.ai/artifact/4piYJXunC1Vp4f14HAtYHh) | R566 to R568; Xin-to-Wen view stays in M3 |
| 2026-11-01 | [Yong architecture plan: one core, many surfaces](https://claude.ai/artifact/NpTvDsHWcBnPtCmWvLtqFC) | RFC 0004 (R558 to R565) |
| 2026-11-01 | [Surface shortlist, revision 2](https://claude.ai/artifact/3cSMv6uiyc9DgCXBDGsmhD) | RFC 0002 (R536 to R550), RFC 0003 (R551 to R557) |
| 2026-11-01 | [Research track: eight extensions](https://claude.ai/artifact/CgK4yYLoCXJ55YaPrKffm8) | R522 to R529 (research, post-1.0) |
| 2026-11-01 | [Tracking research, proposals and spec revisions](https://claude.ai/artifact/1Nc3D55kos87dCFhrWgwyP) | Accepted: spec moved to `spec/`, row registry, `rfcs/`, this index |
| 2026-11-01 | [std.iter proposal](https://claude.ai/artifact/5bV4s3C4spRNVJnzVDqAGr) | R508 to R515 |
| 2026-11-01 | [std.math proposal](https://claude.ai/artifact/DyYksx46pyAe555dLaBB2H) | R500 to R507 |
| 2026-11-01 | [std slices and bisect proposal](https://claude.ai/artifact/GfKVALRNrp1oa6PFatYqZq) | R494 to R499 |
| 2026-11-01 | [std.collections proposal](https://claude.ai/artifact/V79buG5mN2UWG7yV1aiaSX) | R488 to R493 |
| 2026-11-01 | [std.regex proposal](https://claude.ai/artifact/VfVzs8W5JiZJ8rJmUiaUDN) | R482 to R486 |
| 2026-11-01 | [Positioning and market gaps](https://claude.ai/artifact/5HW3yvtnC4CcdwREm5Byz3) | Background for RFC 0002 |
| 2026-11-01 | [Niche surfaces](https://claude.ai/artifact/KeTJFKjzLUGkq1DXDCTSdk) | Superseded by the surface shortlist (RFC 0002) |
| 2026-11-01 | [Surface mock-ups with names](https://claude.ai/artifact/PH4PixndFSBKFfsRKzZrbn) | Superseded by the surface shortlist (RFC 0002) |
| 2026-11-01 | [Novelty audit](https://claude.ai/artifact/UGAdAaZBTP4kFekXPszC1M) | Background for RFC 0002 |
| 2026-10-31 | [Performance frontier survey](https://claude.ai/artifact/4kaWSExgSYFAbkzN9xG1hp) | R480, R481, R487 |
| 2026-10-31 | [Std build plan](https://claude.ai/artifact/FKZWX8ou7oHRZ3TcKkoKjQ) | R474 to R476 |
| 2026-10-31 | [Xin, Wen and Yong comparison](https://claude.ai/artifact/VpqnGVVrQ46MrR5p5ZgJaP) | R477, R478 |
| 2026-10-31 | [Xin and Wen grammar check](https://claude.ai/artifact/BgkEJ6284UCgx2rJmMtGfa) | R460 to R466 |
| 2026-10-31 | [Xin token rerun](https://claude.ai/artifact/Nwge2MBPHS5a4f8tzdRp52) | R216 to R218 |
| 2026-10-31 | [Xin token-evaluation dry run](https://claude.ai/artifact/CsjhbkQw6G3cqYx6sGAJGc) | R280 to R289 |
| 2026-10-31 | [1.0 read-through](https://claude.ai/artifact/14pQJGqZScMaogeuPgyXdA) | Mechanical fixes; four decisions |
| 2026-10-31 | [Futures survey](https://claude.ai/artifact/8ExCjwk7hc3EFTqzrWREJb) | R450 to R459 (R455 deferred past 1.0) |
| 2026-10-31 | [Behavior-oriented concurrency study](https://claude.ai/artifact/VzrZufE1XbSHjKNVNNVsQo) | Deferred past 1.0 as @yong/boc (R455) |
| 2026-10-31 | [Simplicity pass](https://claude.ai/artifact/SsawbF2TuvUSk1fAB7mXU8) | R440 to R449 |
| 2026-10-31 | [Wen ergonomics: real programs](https://claude.ai/artifact/5oH4fkjxnE3JRAGt6Z9JsW) | R430 to R439 |
| 2026-10-31 | [Xin ambiguity pass](https://claude.ai/artifact/THrtooNgDYL181V7Cm7s6A) | R420 to R429 |
| 2026-10-31 | [Distributed compute](https://claude.ai/artifact/1bHS11teg1PxvwVqXzCfqP) | R400 to R419 |
| 2026-10-31 | [Concurrency pass](https://claude.ai/artifact/93gMH3spCrFDR4UwQcfyDQ) | R390 to R399 |
| 2026-10-31 | [Performance pass](https://claude.ai/artifact/CLY5YV4tb62sJbXkmPL1jM) | R380 to R389 |
| 2026-10-31 | [Consistency audit](https://claude.ai/artifact/6m2hzP9zq9KsgcMXv8VqEp) | R370 to R379 |
| 2026-10-31 | [Licensing and governance](https://claude.ai/artifact/LzDEiynMMwg4yJeri7oUFq) | R350 to R366 |
| 2026-10-31 | [Red-team review](https://claude.ai/artifact/EWd27Yz5aQN5LD54Co95wT) | R340 to R349 |
| 2026-10-31 | [Internationalization](https://claude.ai/artifact/FaTXsnXRwSKtK8fgPH9SDM) | R330 to R339 |
| 2026-10-31 | [Embedded and GPU profiles](https://claude.ai/artifact/4vauJ4fyd3eUSTWJqXhiTx) | R320 to R329 |
| 2026-10-31 | [Docs and learning](https://claude.ai/artifact/P85rp8yccZ2Kggomit9aKe) | R310 to R319 |
| 2026-10-31 | [Testing and debugging](https://claude.ai/artifact/DY1uJTqzajm9v4PareFfu4) | R300 to R309 |
| 2026-10-31 | [Paradigm keepers](https://claude.ai/artifact/UeApdTe98kdRA3d5K1w5hp) | R260 to R268 |
| 2026-10-31 | [Diagnostics](https://claude.ai/artifact/GLRhfwSr8xcyE1gZqSLhg3) | R290 to R299 |
| 2026-10-31 | [Formatter](https://claude.ai/artifact/L5u1PmipnjWYvtvCrRhNNj) | R240 to R255 |
| 2026-10-31 | [Supply-chain security](https://claude.ai/artifact/XcMuaSWJuuHdxNPs3R6668) | R190 to R209 |
| 2026-10-31 | [Stability and evolution](https://claude.ai/artifact/5v5DW2ucybYCy5m37XjDhV) | R170 to R189 |
| 2026-10-31 | [Networking and async I/O](https://claude.ai/artifact/Winwv4tfsL2AN99gDY3cCj) | R150 to R167 |
| 2026-10-31 | [Compile-time evaluation](https://claude.ai/artifact/6i7MEkLiSmKE4wMTVjr4j5) | R130 to R144 |
| 2026-10-31 | [Memory and concurrency model](https://claude.ai/artifact/QR2nLrit4cgzXUG23ucJNA) | R110 to R124 |
| 2026-10-31 | [FFI and ABI](https://claude.ai/artifact/EwKVm8TrNYdTdiboqr5HYJ) | R89 to R105 |
| 2026-10-31 | [Wen ergonomics review](https://claude.ai/artifact/7asDC3QNPuWSdXZJMCZePR) | R61 to R84 |
| 2026-10-31 | [Date and time](https://claude.ai/artifact/MoSWSGCoXQ4zqGm8RtKvVd) | R49 to R60 |
| 2026-10-31 | [Math package](https://claude.ai/artifact/FgrLroqqU7a3A4kwK69ZBr) | R28 to R37 |
| 2026-10-31 | [Standard library roadmap](https://claude.ai/artifact/HL2DZtHxuCaLofedMN4tT5) | R23 to R27 |
| 2026-10-31 | [Toolchain design](https://claude.ai/artifact/1DBzFtmytWANhwwKurTg3i) | R15 to R22 |
| 2026-10-31 | [Data layer](https://claude.ai/artifact/ThdSKeptdsaDLCf3Sgu2xk) | R38 to R48 |
| 2026-10-31 | [Technical implementation spec](https://claude.ai/artifact/JqBVcrhH5heYx7F4BjkmYF) | SQ2 to SQ33 picks; Python-reference parts superseded by R215 |
| 2026-10-31 | [Naming workshop](https://claude.ai/artifact/FTwhvf6qauq7jhfyLJmSv1) | Yong, Xin, Wen chosen |
| 2026-10-31 | [Mo, model-first surface](https://claude.ai/artifact/U6kJcvaXvVTviNcqyvq2JN) | R210 to R214 (Xin surface rules) |
| 2026-10-31 | [Yue, token-minimal surface](https://claude.ai/artifact/7CuJcNUgbYvhnnKbtucKZN) | Study only (cited in 26.1) |
| 2026-10-31 | [Chang, dense ASCII surface](https://claude.ai/artifact/DubBokGE1zKEpoVzwYKfAz) | Chang 1; superseded by Chang 2 and RFC 0001 (R516 to R521) |
| 2026-11-01 | [Chang 2, kitchen sink](https://claude.ai/artifact/Urz3zEhuESTobLRf8aqk2X) | RFC 0001: R516 to R521, R530 to R535 |
| 2026-11-01 | [Chang K2 study (positional local names)](https://claude.ai/artifact/V6Do54hGXcokurJ45NHF6f) | R519 |
| 2026-11-01 | [Security model: capabilities, FFI and agents](https://claude.ai/artifact/Hotw9YyR8LEvWE4GbB9gLi) | RFC 0026: R672 to R677 |
| 2026-10-30 | [1.0 Development Roadmap](https://claude.ai/artifact/3ADZraWojw9z33QdGY8GsS) | Living plan |
| 2026-11-09 | [Self-hosted Yan compiler and formal-proof plan](https://claude.ai/artifact/3TzWFi1LTaqs7ggzokFz3T) | #851; RFC 0057 (#858): R966 to R970 replace R215; LLVM C API for 1.0, `.yn` surface, S2 after formal phases 1 and 2 |
