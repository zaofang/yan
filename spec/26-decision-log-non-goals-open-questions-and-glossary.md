## 26. Decision log, non-goals, open questions and glossary

### 26.1 Decision log

| Decision | Chosen | Rejected | Reason |
| --- | --- | --- | --- |
| Structure | one core, two surfaces | two separate languages | one checker and one ecosystem; ecosystem size drives model performance |
| Names (R586) | every name but Yong and Wen from 兰亭集序 (opens with 永, ends with 文): Xin 信, Chang 畅, niche names from the same text | names picked one by one; 信达雅; 文心雕龙; plain dialect words | one famous source, no sound clashes; https://claude.ai/artifact/8zD7T9QnYtNUsbF96cLHNN |
| Xin surface | typed Python 3.12 shape | a novel token-minimal syntax | general models score near zero on unfamiliar languages; one repair round outweighs a 12% token saving |
| Precedence | Python's, with parentheses for confusable mixes | no precedence | rejecting `a + b * c` guarantees repair loops for no safety gain |
| Runtime | none: scheduler, I/O waiting and allocation are capability handlers | M:N green threads; built-in event loop | systems use and Rust-parity speed |
| Memory | unique ownership, explicit `copy`, `Share` for `Rc`/`Arc`, second-class views | tracing GC; Perceus RC; borrow checker with lifetimes | deterministic and fast, with no lifetimes for people or models to get wrong |
| Memory model | C++20 atomics, Tree Borrows, strict provenance, closed UB list, executable checked interpreter | unspecified unsafe semantics | FFI and std internals need a definition that tools can check |
| Effects | second-class capabilities, no effect variables | row-polymorphic effects | sound without effect variables; static handlers compile to direct calls |
| Async | one code shape under blocking and evented handlers, `par concurrent` | `async`/`await` | no function colouring |
| Failure | typed results (`fail`, `?`, `else`); panics abort | exceptions; unwinding | every failure path is in a type; unwinding weakens checking |
| Overflow | checked in every build | wrap in release | builds never change behavior |
| Determinism | default; nondeterminism only through capabilities and `nondet` | best effort | exact replay, caching, reproducible tests and simulation |
| Indexing | proven in Xin, or checked with lint W820, denied in release; checked at run time in Wen (R372) | runtime checks everywhere | off-by-one becomes a compile error for models; people keep ergonomics |
| Verification | Dafny-style auto-active, optional, cached, deterministic limits | mandatory proofs; interactive provers; wall-clock limits | models verify best in auto-active style; proofs stay reproducible |
| Metaprogramming | specialization on stage-0 values, reflection, `derived` bodies | macros, quotes, token streams | one language at both stages; hygiene by construction |
| Edits | whole-item patches with hash bases | line diffs | no drift, no ambiguous anchors |
| Item order (Xin) | author order, additions appended | sorted | least surprising for people; patches stay simple |
| Hashing | keyed foldhash-class hashing with a SipHash-1-3 fallback, behind insertion-ordered maps (R387) | unkeyed hashing | defeats hash flooding at no cost to determinism |
| Traits | associated types, no GATs | GATs | lending iterators come from `from` |
| Actors | std packages; non-reentrant, bounded mailboxes, deadlines, supervision | in the language | no runtime; OTP defaults |
| Channels | bounded only | unbounded | unbounded queues turn overload into memory exhaustion |
| Contracts | pure predicates; no ignoring unproven `pub` preconditions; one violation path | observe mode in production; global handler | C++26 contracts failed on vague semantics |
| Strings | UTF-8 without integer indexing | integer indexing | indexing UTF-8 by integer is wrong or O(n) |
| Slices (Xin) | clamp, as Python; W807 and `slice_exact` | panic | matches Python |
| Regex | linear time only | backtracking in std | ReDoS; `@yong/fancy_regex` exists |
| Serialization | schema-directed `Wire`, one derive | native object serialization | a third or more of Java's vulnerabilities |
| Processes | `proc.spawn` with argv | shell strings | injection |
| ABI | C profile `yong_abi_v1`, no Yong-to-Yong ABI | a stable native ABI | Swift's resilience needs runtime metadata |
| Supply chain | witnessed log, registry-built archives, SLSA L3, OIDC-only publishing, computed capabilities, signed approvals | API tokens; uploaded tarballs | xz backdoor; npm worms; slopsquatting |
| Stability | bit-identical output across 1.x; surface-only editions; enforced semver; 12-week minors | LTS branches; nightly channel | Rust and Go promises, Elm enforcement |
| Implementation language | Rust only (R215) | Python reference, then a Rust port | one codebase with no port; the chosen toolchain stack is Rust crates; the conformance suite is the oracle |
| Third surface | Chang, dense ASCII with spelling kept as trivia, admitting any form that lowers to the core (R516 to R521) | one printing per tree (Chang 1); no third surface | a design playground that stays one language: same items, same hashes, same checker |
| Xin 1.0 rules | model-first rules on the Python shape: full headers as the contract, undecided checks as runtime checks with W820, ... holes, header changes report broken callers, a wide lenient reader (R210 to R214, R280 to R289) | Yue, a token-minimal surface (research only); implicit string copies (R283) | in the dry run output was about 5% of loop tokens and repair rounds dominated, so rounds and pass@1 matter more than terseness; copies stay explicit and the autofix inserts them |

### 26.2 Non-goals

- No null, exceptions, inheritance, implicit conversions, or operator overloading outside the listed traits.
- No syntactic macros in any surface.
- No `async`/`await`, no unwinding, no language runtime, no tracing garbage collector and no implicit reference counting.
- No formatting options.
- No Unicode identifiers, and no localized keywords.
- No hiding of remoteness: distribution is visible in capabilities and failure types.
- No optional features or conditional dependencies in 1.0.
- No generic associated types.
- No stable Yong-to-Yong ABI, no direct C++ interop, no dylib plugins passing Yong types in 1.0.
- No `Condvar` in 1.0.

These are non-goals for Yong. Chang (16.6) may spell some of them as sugar over the core, such as `null` for `None`, lossless widening written out in Yong, `extends` as delegation and `async`/`await` as branch and join; the Yong it elaborates to keeps every non-goal above.

### 26.3 Open questions

1. **Query compilation trigger**: decided by the owner on 2026-10-31, automatic in release builds for stage-0-constant plans (11.9).
2. **R1 to R14 confirmation**: confirmed as normative by the owner on 2026-10-31.
3. GPU code generation from `par for` bodies through MLIR (M4).
4. Whether Xin's remaining non-Python constructs (`uses`, `pre`, `post`, `ex`, `var`, `take`, `fail`, `&`, `from`) need the 21.6 fluency plan to reach parity with Python on pass@1.
5. Every token-cost claim must be re-measured against the target models' own tokenizers.
6. Behaviour-oriented concurrency (R455) was deferred past 1.0 on 2026-10-31, as a possible first-party package `@yong/boc`. E718 stays reserved for it. The study (https://claude.ai/artifact/VzrZufE1XbSHjKNVNNVsQo) found that its deterministic subset rarely beats a loop or `par for` followed by an ordered commit, and that live use needs `uses nondet`, as Mutex does. Before any adoption, seven defects need rows: no Xin or Wen spelling, an undefined waiting scope, no result path to the scheduler, one cell named twice, E718 across calls, an unmeasured scheduling cost, and how a package registers waits under R396. Revisit when an M2a benchmark program needs atomic updates across several objects.

### 26.4 Glossary

| Term | Meaning |
| --- | --- |
| Yong (永) | the core semantics, its IR, and the toolchain |
| Xin (信) | the surface written by language models |
| Wen (文) | the surface written and read by people |
| capability | a second-class value granting an effect, such as `io` or `net` |
| handler | a value implementing a capability's trait, bound with `with` |
| canonical text | the one printing of a program that the formatter produces |
| item | a top-level declaration: function, generator, subscript, lemma, type, trait, impl, constant or test |
| item hash | BLAKE3 hash of an item's canonical Yong text (3.6) |
| lenient reader | the Xin front end mode that accepts and normalizes listed deviations |
| L0 to L6 | the layers of depth, from the surface (L0) to the machine (L6) |
| place | an access path: a variable followed by fields and indices |
| view | a second-class borrow of a contiguous part of a value: `View`, `MutView`, `StrView` |
| second-class | may not be stored in escaping data, returned without `from`, or captured by an escaping closure |
| projection | rendering an item in the other surface, or the borrowed place `a[i]` (by context) |
| stage 0 | compile-time evaluation on the checked interpreter |
| materializable | a stage-0 value that can be emitted as read-only data |
| spec gate | the two-phase protocol: contracts checked before a body is written |
| edition | a named set of surface rules covering both surfaces of a package |
| C profile | `yong_abi_v1`, the frozen C ABI mapping of Yong types |
| computed capability set | the capabilities a version actually uses, computed by the registry at publish |

### 26.5 Licensing, governance and trademark (R350 to R366)

The owner accepted these rows on 2026-10-31 from the licensing and governance design (https://claude.ai/artifact/LzDEiynMMwg4yJeri7oUFq). They are project policy, not legal advice. Employment IP and trademark clearance MUST be settled before the repository goes public, and the repository goes public at the M0 gate.

| Row | Rule |
| --- | --- |
| R350 | Code is licensed Apache-2.0 WITH LLVM-exception. This covers the compiler, toolchain, std, runtime support and conformance suite. |
| R351 | Spec text is CC-BY-4.0. Grammars, normative tables and spec code samples are also available under R350. |
| R352 | Docs, the tour and the book are CC-BY-4.0. Doc examples, `yong new` templates and published eval task statements are 0BSD. |
| R353 | Compiler output carries no Yong license obligation. Std and runtime code embedded in it falls under the LLVM exception. |
| R354 | Contributions come in under DCO 1.1 (`Signed-off-by:`), inbound = outbound, with no CLA. |
| R355 | Every agent-written or AI-assisted commit carries an `Assisted-by:` trailer. Notices read "Copyright the Yong authors". |
| R356 | `yong publish` refuses a manifest without a valid SPDX `license` expression. `LicenseRef-*` is allowed only with a bundled license file. The registry terms grant the registry and its mirrors a perpetual, irrevocable right to redistribute published versions. License data goes into the SBOM (R207). A manifest `license_policy` allow or deny list is enforced over the dependency graph by `yong check`. |
| R357 | The held-out eval repository and brand source files are all rights reserved and private. |
| R358 | The project has one lead, the owner. Agents propose, draft, implement and review. They never accept, approve, merge or publish. |
| R359 | RFC pipeline: draft, adversarial review by an independent reviewer whose findings carry witnesses (R185), decision by the lead, spec rows, `@preview` (R180), stable (R172). Once public, RFCs live in `rfcs/NNNN-slug.md`, rejected ones included. |
| R360 | An RFC is required for anything in the freeze table (R171), editions, the std public API, the grammar of any surface, registry policy and these rows. |
| R361 | A core team forms when 3 outside humans each have 10 or more merged PRs within 6 months. It decides by consensus, the lead keeps a veto, and a 14-day comment period starts. The marks, domains and registry move to an entity (a fiscal sponsor or an LLC) before the 1.0 release. |
| R362 | A sealed succession instruction names who receives the marks, domains, GitHub org, publishing identities (R194), namespace-admin passkeys and witness keys (R191). |
| R363 | The repository carries CODE\_OF\_CONDUCT.md (Contributor Covenant 3.0), SECURITY.md (private reporting, 90-day disclosure, OSV advisories per R202) and CONTRIBUTING.md (DCO, AI disclosure, RFC scope). |
| R364 | YONG (word) and the 永 logo are registered in classes 9 and 42: US first, then Madrid designating CN and EU. Xin and Wen are unregistered sub-names. The logo calligraphy MUST be original or assigned in writing. |
| R365 | Trademark policy: nominative use is free, including in package names. Forks rename, except distro packages with build or security patches. "Yong-compatible" requires passing the published conformance suite for the claimed version and surface. "Official" and "std" mean project artifacts only. |
| R366 | Registry namespaces `yong`, `std`, `core`, `xin` and `wen` are reserved to the project (R195), along with the project domain and handles. |

### 26.6 Research track (R522 to R535, R554 to R557)

Accepted for research on 2026-11-01, not normative for 1.0. Each row names the 1.0 text it would amend if adopted; adoption takes its own RFC. The study is https://claude.ai/artifact/CgK4yYLoCXJ55YaPrKffm8; the Chang forms waiting on each row are in RFC 0001. Rows R554 to R557 come from RFC 0003 (study https://claude.ai/artifact/3cSMv6uiyc9DgCXBDGsmhD).

| Row | Research item | 1.0 text it would amend |
| --- | --- | --- |
| R522 | Lexical effect handlers with one-shot resume, the handler passed as a capability parameter (Effekt). | 10.5 (no continuations), for one-shot lexical segments only |
| R523 | Information-flow labels `T@L` on a Lub lattice, with `declassify` behind a `Declassify[L]` capability (Jif, FlowCaml). | law 4 (a sixth fact) |
| R524 | Graded types, the 0/1/many semiring first; grade 0 makes erasure of ghost code and type arguments checked (Granule, Idris 2). | none |
| R525 | Typed staging: `Code[T]` values at stage 0, quote and splice, splice output hashed as ordinary text; no run-time code generation (MetaOCaml, Scala 3). | 10.4 (no quotes or AST values), for typed code only |
| R526 | Per-reader spelling: `yong view --as <surface>` for review and diff, over item hashes (Hazel, MPS). | none |
| R527 | Bidirectional grammars: one definition per surface yields its parser, printer and round-trip check; later `std.syntax` for user formats (Boomerang). | none |
| R528 | Incremental computation: compiler queries keyed by item hash; later a `Cell[T]` library with pure derivations (Adapton, Salsa). | none |
| R529 | Synthesis from examples: `yong fill` proposes code for a hole from its type, capabilities, contracts and `ex` clauses; tool-only, the result is committed as source (Myth, Hazel). | none |
| R530 | Positional type parameters, for anonymous generics (`some Trait`). | 3.6 |
| R531 | `std.sym.Sym`, an interned text type compared by content. | 17 |
| R532 | `std.any.Any` with type ids from the identity hash and checked downcast. | 10.2 |
| R533 | Panic recovery at a task boundary behind a `Recover` capability; unwinding only inside the recovered task. | 8.3, 26.2 |
| R534 | Software transactional memory over `TVar` cells behind an `Stm` capability, under `uses nondet`. | 9 |
| R535 | `eval` behind an `Eval` capability: parse, check and run on the reference interpreter with only the capabilities passed in. | 11.9 |
| R554 | Dry-run handler class: handlers that perform no effect and return `Deferred[T]` placeholders; a later real run re-reads state and fails with `Drift` on a changed read (RFC 0003). | 7.3 |
| R555 | Streams over `Remote`: a call returning a typed receiver with credit-based backpressure on the R404 transport (RFC 0003). | 9.6.1 |
| R556 | Ceiling-level `fs.only(read=..., write=...)`, so package ceilings can name paths (RFC 0003). | 17.6, 19.2 |
| R557 | Semantic diff: `yong diff --semantic` decides equivalence of two item versions in the decidable fragment and gives a distinguishing input otherwise (RFC 0003). | none |

