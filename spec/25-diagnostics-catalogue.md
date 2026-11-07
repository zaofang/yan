## 25. Diagnostics catalogue

This is the complete registry of diagnostic codes for 1.0. Codes keep their meaning forever and are never reused; retired codes stay listed. E is an error, W a warning or lint, H a hole, V a verification result, P a patch rejection, N a lenient-reader note and I an internal error. Codes marked *new* were numbered at consolidation from rows that asked for a code without giving one.

### 25.1 Families

| Range | Family |
| --- | --- |
| E0xx | packages, registry, supply chain and stability |
| E1xx | lexical rules and syntax |
| E2xx | names, types, generics and stage 0 |
| E3xx | capabilities, handlers and failure |
| E4xx | ownership, views, memory and thread safety |
| E5xx | exhaustiveness, bounds, shapes, units and checked literals |
| E6xx | contracts |
| H7xx, E7xx | holes and concurrency |
| V4xx, W420 | verification |
| W8xx | lints |
| E9xx | unsafe, FFI, layout, linking and inline assembly |
| P0xx | patches |
| N01 to N43 | lenient-reader notes (14.4) |
| I001 to I999 | internal errors |

### 25.2 Packages, registry and stability

| Code | Meaning | Section |
| --- | --- | --- |
| E001 | preview item used without `preview = ["name"]`, or experimental Yan Meta form used without `@lab(name)` (R778) | 20.6, 16.6 |
| E002 | published package at 1.0 or later enables a preview or contains `@lab` *new* | 20.6, 16.6 |
| E003 | package name does not exist; nearest names suggested *new* | 19.4 |
| E004 | agent add of a package neither locked nor established needs an approval *new* | 19.4 |
| E005 | update gains a capability or changes publisher identity; needs an approval *new* | 19.4 |
| E006 | declared capability ceiling does not contain the computed capability set *new* | 19.2 |
| E007 | version is marked malicious *new* | 19.3 |
| E008 | version bump is below the minimum `yan semver` computed *new* | 20.6 |
| E009 | approval signature invalid, or signer not in `approvers` *new* | 19.4 |
| E010 | build differs between two roots under `--verify` *new* | 19.5 |
| E011 | transparency-log inclusion, consistency or witness quorum check failed *new* | 19.3 |
| E012 | std `pub` item without a current `@since` *new* | 20.3 |
| E013 | confirmed breaking change (with witness) for the declared bump *new* | 20.6 |
| E014 | approved test changed since tests.lock was signed new | 18.4 |
| E015 | approved test in tests.lock is missing new | 18.4 |
| E016 | `yan publish` refuses the manifest: `overrides`, a dev dependency reached from non-test code, or a pre-release dependency at 1.0 or later *new* | 19.4 |
| E018 | `package.yag` has a field not in the manifest table, or lacks `name`, `version`, `edition` or `yan` where required *new* | 19.1 |
| E019 | `yan.lock` has a `format` newer than this toolchain reads *new* | 19.1 |
| E020 | under `--locked`, `yan.lock` is missing or does not match the manifest, or an offline archive is absent *new* | 19.1 |
| E021 | a namespace has no index mapping in `registries` *new* | 19.3 |
| E017 | construct this toolchain does not support yet, reported at the construct; checking continues (R649) new | 18.2 |
| W014 | possible breaking change without a witness *new* | 20.6 |

### 25.3 Lexical rules and syntax

| Code | Meaning | Section |
| --- | --- | --- |
| E100 | unexpected token; the expected set is listed* new* | 13.2, 15.2 |
| E101 | operator mix that requires parentheses | 13.2, 15.2 |
| E102 | unclosed bracket, string or block* new* | 13.1, 15.1 |
| E103 | backslash continuation or `;` (Yan Agent) | 13.1 |
| E104 | chained comparison | 13.2 |
| E105 | forbidden invisible, bidirectional or tag character | 13.1 |
| E106 | contract line (`pre`, `post`, `ex`, `decreases`, `invariant`) after the first other statement of a body; fix moves it (R422) new | 13.1 |
| E107 | comprehension with more than one `for` clause | 13.2 |
| E110 | malformed value name | 13.1 |
| W111 | value name longer than 32 characters | 13.1 |
| E112 | consecutive capitals in a type name | 13.1 |
| E120 | `try`, `except`, `finally` or `raise` | 13.1 |
| E121 | unsupported Python keyword (`global`, `nonlocal`, `del`, `yield`, `async`, `await`) | 13.1 |
| E130, E131 | retired in v0.2 (hyphenated Yan names) |  |
| E132 | `--` not followed by a space (Yan) | 15.1 |
| E133 | tab in Yan indentation | 15.1 |
| E134 | `a -b` inside a grid literal | 15.1 |
| E135 | nested implicit-subject closures | 15.3 |
| E140 | interior NUL in a `c"..."` literal *new* | 12.3 |
| E141 | `yield` outside a generator body *new* | 10.5 |
| E143 | become inside a constructor not in last-evaluated argument position (R456) new | 3.5 |
| E144 | data file outside the data profile: an item other than a literal constant, or an operator, call or name in a value (R827) *new* | 17.3, 19.1 |

### 25.4 Names, types, generics and stage 0

| Code | Meaning | Section |
| --- | --- | --- |
| E200 | unresolved name, including std items newer than the package's `yan` version *new* | 20.3 |
| E201 | type mismatch, with the differing parts of the two types marked; arithmetic on an `int`, `float` or `str` optional adds the `choice` fix `(x else 0)`, `(x else 0.0)` or `(x else "")` (R929)* new* | 10.1, 14.1 |
| E202 | wrong number of arguments, or unknown keyword or label argument* new* | 14.1, 15.3 |
| E203 | no such field or method; nearest names suggested* new* | 10.2, 17.2 |
| E210 | keyword or label arguments required (two parameters of a callee with three or more share a type) | 14.1, 15.3 |
| E220 | overlapping impls | 10.2 |
| E221 | type does not implement a required trait; the trait's @diagnostic hint is shown when present* new* | 10.2 |
| E230 | rebinding a name (no shadowing) | 14.1, 15.3 |
| E231 | retired (R214): unused bindings and parameters are lint W812 | 14.1 |
| E240 | missing parameter or return annotation | 14.1 |
| E241 | annotation needed: inference would need search | 10.1 |
| E250 | two functions of one name at one method-resolution step | 17.2 |
| E251 | capability set includes itself, directly or through another set (R431) new | 7.1 |
| E260 | stage-0 panic (with stage-0 backtrace and use site) *new* | 10.4 |
| E261 | stage-0 step, heap or depth budget exceeded *new* | 10.4 |
| E262 | non-materializable value crosses into run time *new* | 10.4 |
| E263 | compile-time function parameter not specialized away *new* | 10.4 |
| E264 | reflection access to an item or field not visible at the call site *new* | 10.4 |
| E265 | reified name collides with an existing name *new* | 10.4 |
| E266 | attribute function output not allowed (rewrite, removal, extern, export or built-in attribute) *new* | 10.4 |
| E267 | `embed` path outside the package root *new* | 10.4 |
| E268 | construct not allowed at stage 0 (`unsafe`, `Ptr`, `extern`, `asm`, atomics, `Vec`) *new* | 10.4 |
| E269 | derive of a trait without a `derived` body, or of an auto trait; a `derive(Cli)` field the mapping cannot parse, or an `@help` key that names nothing or appears without `derive(Cli)` (R694) *new* | 10.4, 17.3 |
| E270 | `compile_error` raised by stage-0 code *new* | 10.4 |
| E271 | `become` callee signature differs from the caller's *new* | 3.5 |
| E232 | a `type` that mixes inline variants with other alternatives, or lists `None` among variants (retargeted by R423) | 14.1 |
| E272 | error in a stage-0-dependent body, found at an instantiation; carries the instantiation site and stage-0 backtrace new | 10.4 |
| E273 | `become` outside tail position, inside par, with or a generator, or calling a closure or dyn method *new* | 3.5 |
| E274 | access outside the region an `@access` claim declares (R627) new | 11.2 |
| E275 | instantiation depth exceeded: polymorphic recursion past 64 nested instances, reported once per cycle (R772) *new* | 3.5, 10.1 |
| E276 | a union lists `None` twice; write `Option[T \| None]` for a nested optional (R793) *new* | 14.1 |
| E277 | `@expect` of a code that an enclosing `@deny` item denies (R797) *new* | 18.2 |
| E278 | `@deny` names a code that is not a W8xx lint (R797) *new* | 18.2 |
| E279 | `yan view --strict`: a spelling mark that raise did not use, or dropped as invalid (RFC 0034, R841) *new* | 16.5 |
| E280 | one parameter documented both in an `Args:` section and above the parameter (R846) *new* | 18.9 |
| E281 | a type written in a parameter doc disagrees with the signature (R849) *new* | 14.4 |
| E282 | a derived trait (`Eq`, `Ord`, `Hash`, `Show`, `Wire`) has a field whose type does not implement it; one diagnostic per field, at the field's type annotation (RFC 0040, R872, R873) *new* | 10.4, 17.4 |
| E233 | str or f-string where a Template is expected (R452) new | 13.1 |

### 25.5 Capabilities, handlers and failure

| Code | Meaning | Section |
| --- | --- | --- |
| E301 | missing capability (fix inserts `uses`) | 7.2 |
| E306 | capability stored, returned or captured by an escaping closure | 7.2 |
| E307 | dynamic handler for a capability that must be static | 7.3 |
| E310 | missing `From` conversion | 8.1 |
| E311 | `try` result never inspected | 8.1 |
| E312 | non-unit result discarded | 8.1 |
| E313 | both injection and `From` apply | 8.1 |
| E315 | fallible call without `?` or `else` where no `Result` is expected; fix inserts `?` (R424) new | 14.1 |
| E316 | replay(trace, live=\[...\]) names a capability whose operations return values the program reads (R305, R379) new | 7.6 |
| E314 | capability outside the package ceiling, directly or through a dependency *new* | 7.4, 19.2 |
| E317 | release build binds a `fault` handler other than `never` *new* | 7.3 |
| E318 | release build binds the `toolchain` capability *new* | 21.4 |
| E319 | differentiating an impure function *new* | 11.5 |
| E320 | profile restriction violated (names the profile and the rule) new | 17.1 |
| E321 | stack bound not provable: call-graph cycle other than become, or extern without @stack new | 17.1 |
| E322 | task stack bound exceeds the manifest's stack reserve new | 17.1 |
| E323 | lock (Resource, CsMutex, Mutex or RwLock guard) held across a suspension point, on every platform (R395) new | 17.1 |
| E325 | Untrusted value passed to a @trusted parameter (R451) new | 17.4 (R451) |
| E326 | host handler built or bound outside the platform, a `yan_world` or an attenuation of a held handle (R672) new | 7.3 (R672) |
| E327 | closure literal whose body consumes a capture, not marked `once` (R816) *new* | 4.6 |
| E328 | `once` function value where a plain one is expected (R815), no automatic fix *new* | 4.6 |
| E329 | extractor or view function that uses a capability other than `alloc`, or can fail (R817) *new* | 15.3 |
| E330 | a `Wire` field type a codec cannot represent: a nested optional (`T \| None \| None`), a variant field named like its sum's json tag, or a csv row field or cell that is not a scalar, a field-less sum or a newtype over them (R879, RFC 0041) *new* | 17.4 |
| E331 | a malformed item-level `@wire`: a `names` key that names no field or variant, a name renamed twice, a key that is both a variant and a field, two fields or variants with one wire name, or an unknown `@wire` key or value (R886, R887) *new* | 17.4 |
| E332 | a manifest `edition` that is neither `"next"` nor a four-digit year of 2026 or later (R888) *new* | 19.1, 20.4 |

### 25.6 Ownership, views, memory and thread safety

| Code | Meaning | Section |
| --- | --- | --- |
| E401 | use after move | 4.1 |
| E402 | exclusivity violation | 4.3 |
| E403 | assignment to an immutable binding | 4.1 |
| E404 | missing `&` on an inout argument | 14.1, 15.3 |
| E405 | `&` where no inout parameter accepts it | 14.1, 15.3 |
| E406 | escaping closure captures an inout parameter, `var` or view; a captured capability is E306, not E406 (R467) | 4.6 |
| E407 | `retired by R441; an arena Ref that escapes is E414` | 4.7 |
| E408 | retired by R441; arena data that escapes is E414 | 5.5 |
| E412 | `@linear` value not consumed | 10.6 |
| E413 | implicit copy of a heap-backed value (fix inserts `copy` or `share`) | 4.2 |
| E414 | view or second-class value escapes or outlives its source | 4.5 |
| E415 | non-Send value crosses a task boundary | 6.4 |
| E416 | `copy` of a `Share` value (fix: `share`) | 4.2 |
| E417 | non-Sync value borrowed by two `par` branches | 6.4 |
| E418 | `Atomic[T]` where the target has no lock-free form | 6.2 |
| E419 | exposed provenance outside an `ffi` module | 6.6 |
| E420 | `become` argument borrows a caller local *new* | 3.5 |
| E421 | drop body that can fail, blocks or uses a capability other than alloc, or Drop on a @linear type new | 5.4 |
| E422 | subscript body does not yield exactly once on every path (R457) new | 4.4 |
| E423 | subscript body fails under the let or inout mode it is used in (R457) new | 4.4 |
| E424 | branch, index, division or early exit depends on a secret-derived value in a `@constant_time` function (R611) new | 17.4 |

### 25.7 Exhaustiveness, bounds, shapes, units and literals

| Code | Meaning | Section |
| --- | --- | --- |
| E501 | index, key or other goal refuted, with the counterexample (R211) | 14.1 |
| E502 | wildcard on a sum (Yan Agent) | 14.1 |
| E503 | non-exhaustive match, including open C enums and `@non_exhaustive` types without `_` | 14.1, 12.3 |
| E504 | integer indexing or `len` on `str` | 14.1 |
| E520 | positional axis broadcast against a non-scalar | 11.2 |
| E521 | tile shape mismatch in a kernel operation new | 17.1 |
| E530 | unit, dimension, kind or affine-point mismatch | 11.4 |
| E540 | invalid or out-of-range date or time literal *new* | 14.2 |
| E541 | zoned literal in a gap or an unresolved fold *new* | 14.2 |
| E542 | literal format pattern does not match the value's type *new* | 14.2 |
| E543 | literal or destructuring of a `@non_exhaustive` type from another package without `..` *new* | 20.6 |
| E544 | `is_null` on a non-optional column *new* | 11.8 |
| E545 | literal regular expression rejected at stage 0: syntax, a backreference or lookaround, or an automaton over the size limit (R484, R486; registered by R648) | 17.2 |
| E550 | malformed locale literal (R332) new | 17.5 |
| E551 | malformed MessageFormat 2 message or unknown function (R335) new | 17.5 |
| E552 | translation key not in the source messages record (R335) new | 17.5 |
| E553 | message variable undeclared in the source message or of the wrong type (R335) new | 17.5 |
| E554 | plural match misses a CLDR category of the locale and has no \* arm (R335) new | 17.5 |
| E555 | unbalanced message markup or a tag the source message lacks (R335) new | 17.5 |

### 25.8 Contracts and verification

| Code | Meaning | Section |
| --- | --- | --- |
| E601 | `pub` function with no `ex` and no `test_` calling it (Yan Agent; publish and release only, R284) | 8.4 |
| E602 | recursion without `decreases` | 8.4 |
| E603 | contract not proven in a `@prove` module *new* | 8.5 |
| E617 | retired by R447; a test that binds a real capability is an I/O test | 18.4 |
| V401 | proof failed, with counterexample | 8.7 |
| W420 | proof fell back to a runtime check | 8.5 |
| E604 | non-ghost code reads a ghost value (R458) new | 8.7 |
| E605 | lemma uses a capability (R458) new | 8.7 |
| E618 | `cost` bound refuted, with a witness input (R610) new | 8.4 |
| E619 | the arguments of an `ex` line break the function's `pre` (R621) new | 21.1 |
| V402 | proof certificate rejected by the toolchain's checker (R608) new | 8.7 |

### 25.9 Holes and concurrency

| Code | Meaning | Section |
| --- | --- | --- |
| H701 | typed hole, with expected type | 21.2 |
| E702 | hole in a release build | 21.2 |
| E705 | `par` branch reads a binding another branch makes, or a place it writes; reported as E705 alone, not also E402 (R471) | 9.2 |
| E708 | `par concurrent` under a handler without concurrent forward progress | 9.1 |
| E709 | shared mutable state or nondeterministic channel operation without `nondet` | 6.5 |
| E710 | `virtual` time handler under the `pool` scheduler *new* | 7.3 |
| E711 | `select` arm is not `recv()`, `send(v)`, `after(d)` or `deadline(t)` *new* | 15.3 |
| E712 | body of a streaming (unit-bodied) par for can fail new | 9.3 |
| E713 | plain par branch or par for item waits on a sibling through a channel (R390) new | 9.2, 9.3 |
| E714 | Once or Lazy initializer reachable from several tasks uses a capability other than alloc without uses nondet (R393) new | 6.3 |
| E715 | @reentrant handler keeps a value read from actor state live across a suspension (R397) new | 9.6 |
| E716 | non-par function parameter called from more than one branch of a par or from a par for body (R436) new | 4.6 |
| E720 | actor shared as Remote has a handler whose parameters, result or failure are not Wire (R401) new | 9.6.1 |
| E721 | call\_remote or an off-node spawn names something other than a @remote item, such as a closure (R407) new | 9.6.1 |
| E722 | @remote item's uses set exceeds the ceiling of every node role that exports it (R407) new | 9.6.1 |
| E723 | actor state schema changed with no @upgrade function and no automatic converter, or an @upgrade that is not uses none (R415) new | 9.6.1 |
| E724 | breaking change to a pinned Wire schema (R406) new | 9.6.1, 20 |
| E725 | @durable function uses a non-journaling capability, unjournaled nondeterminism, or a view across operations (R416) new | 9.6.1 |
| E726 | entity key not Wire + Hash, or entity state not Wire (R413) new | 9.6.1 |
| E727 | session operation the endpoint's current state does not allow: a send of another label, a `recv` or a `close` (R634, R926) new | 9.5 |
| E728 | session protocol cannot be projected onto a role (R633) new | 9.5 |
| E717 | Lub get() inside a par block that writes the cell (R454) new | 9.3 (R454) |
| E718 | reserved (R455, deferred past 1.0) | 9.3 |

### 25.10 Lints

All warn by default and deny under `--strict` and `--level release`, except where noted.

| Code | Meaning | Section |
| --- | --- | --- |
| W801 | confusable identifier | 13.1 |
| W802 | retired by R443; an unclosed `@linear` value is E412 | 5.4 |
| W803 | exposed `Secret` flows into `log` | 17.4 |
| W804 | retired by R448; `FastHasher` moved to `@yan/fasthash` | 7.7 |
| W805 | error union over 8 members | 8.1 |
| W806 | code special-cases literal `ex` inputs | 21.1 |
| W807 | constant slice proven out of range | 14.1 |
| W808 | static call cycle between actors | 9.6 |
| W809 | retired by R448; std has no `shell` | 7.1 |
| W810 | `@expect` suppressed nothing | 18.2 |
| W811 | wildcard on a sum (Yan) | 15.3 |
| W812 | unused binding or parameter (Yan Agent and Yan, R214) | 15.3 |
| W813 | `pub` item without `ex` (Yan) | 15.3 |
| W814 | spin loop on an atomic shared by plain `par` branches | 6.5 |
| W815 | use of a deprecated item *new* | 20.5 |
| W816 | `compile_warn` raised by stage-0 code *new* | 10.4 |
| W817 | `day` or `week` `Duration` added to a `ZonedDateTime` *new* | 14.2 |
| W818 | else that discards a Cancelled failure (R341) new | 9.4 |
| W819 | method found at an earlier resolution step hides one of the same name at a later step (R348) new | 17.2 |
| W820 | check neither proven nor refuted, compiled as a runtime check; carries the unknown goal (Yan Agent, R211) new | 8.6, 14.1 |
| W823 | local annotation that inference already gives (Yan Agent); safe fix removes it (R429) new | 14.3 |
| W824 | a `None` or failure guard right after a binding that a fallback `else` replaces; safe fix rewrites it (R429) new | 14.3 |
| W821 | @multiversion function whose versions compile to the same code (R386) new | 11.3 |
| W822 | + in a loop on a Str or List that is not at its last use (R388) new | 3.3 |
| W830 | pub item without a doc comment (R317) new | 18.9 |
| W831 | doc item link resolves to nothing (R317) new | 18.9 |
| W832 | Fails: or Uses: doc section disagrees with the signature (R317) new | 18.9 |
| W833 | fence tagged text parses as code (R317) new | 18.9 |
| W834 | pub item of a library package fails with std Error (R432) new | 8.1 |
| W840 | string literal content is not NFC; fix normalizes it (R338) new | 13.1 |
| W841 | message missing from a shipped locale; the source text is used (R335) new | 17.5 |
| W843 | permanent supervisor child placed on(node), which cannot fail over (R411) new | 9.6.1 |
| W850 | durable operation on a shared capability whose handler declares no idempotency support (R453) new | 9.6.1 (R416) |
| W851 | `cost` bound neither proven nor refuted, compiled as a counter in `checked-all` (R610) new | 8.4 |
| W852 | float expression less accurate than a rewrite found for it (R613) new | 11.1 |
| W853 | match arm that can match no value left by earlier arms (R606) new | 8.6 |
| W854 | Yan string with `${...}`, which Yan prints literally; fix rewrites it to `\(...)` (R663) new | 15.1 |
| W855 | `shield` block with no blocking operation (R812) *new* | 9.4 |
| W856 | `shield` duration not below the default cancel grace (R812) *new* | 9.4 |
| W857 | send to a `drop_*` actor inside a `shield` (R814) *new* | 9.6 |
| W858 | relaxed float block with no float operator (R822) *new* | 7.7 |
| W859 | `with fast_math:` nested in a relaxed scope (R822) *new* | 7.7 |
| W860 | an `Args:`, `Attributes:` or `Variants:` entry names no parameter, field or variant (R848) *new* | 18.9 |
| W861 | off by default, on in `yan.toml`: a public parameter, field or variant without a doc (R848) *new* | 18.9 |

### 25.11 Unsafe, FFI, layout and assembly

| Code | Meaning | Section |
| --- | --- | --- |
| E900 | proof attribute (`@forbid(...)`, R447) violated, per line *new* | 5.7 |
| E901 | `unsafe` outside std or an `ffi` module *new* | 12 |
| E902 | extern block outside an `ffi` module *new* | 12 |
| E903 | unsafe operation outside an `unsafe` block *new* | 12.1 |
| E904 | type crosses the FFI boundary without `@layout(c)` or a C-profile type *new* | 12.3 |
| E905 | C variadic argument violates the default promotions *new* | 12.3 |
| E906 | item with returns-twice semantics declared *new* | 12.5 |
| E907 | `@export` signature uses a type outside the C profile *new* | 12.7 |
| E908 | `@layout(bits(uN))` field widths do not sum to N *new* | 5.2 |
| E909 | duplicate export name at link time *new* | 12.8 |
| E910 | `asm` operand type not supported *new* | 12.9 |
| E911 | `asm` register or register class invalid for the target *new* | 12.9 |

### 25.12 Patches, notes, internal errors and panics

| Code | Meaning |
| --- | --- |
| P001 | base hash mismatch (current text returned) |
| P002 | edit search text matched zero or many times |
| P003 | patch introduces errors (under `--check`) |
| P004 | replaced or deleted item missing |
| P005 | added item already exists |
| P006 | patch touches a protected target without a human confirmation token |
| N01 to N43 | lenient-reader normalizations (14.4) |
| I001 to I999 | internal errors, including Yan validator failures |

Panic kinds, reported as `panic(kind): message` with exit status 101: `overflow`, `div_zero`, `contract`, `exclusivity`, `bounds`, `shape`, `unreachable`, `assert`, `deadlock`. Undefined-behavior reports from `--ub` name the item number of 6.7.
