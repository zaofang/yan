# 0079: Foreign code in dependency ceilings, and the package graph that checks it

- Status: accepted
- Research: [capability security review, 2026-11-16](../docs/research/2026-11-16-capability-security-review.md) (finding F2); RFC 0077 D3; #1427 items 1 to 3
- Rows: R1026 to R1029

## Summary

19.2 says every extern library is a capability its package's ceiling must list and that "the ceilings
of its dependencies are enforced transitively" (R1020), but does not say what transitive means for a
library that a dependency reaches through `uses none` items, and nothing checks it: E314 runs per module
against the nearest `package.yag`, and no compile reads dependency source at all. Consent (R1019)
already covers the whole resolved graph, but it reads the `ffi` list from `yan.lock` or `.info` and
trusts it. This RFC fixes what a dependency's foreign code costs each package above it, where the one
computed fact comes from, and the package graph the compiler and the resolver share to check it. It
adds no diagnostic code: E314 already reads "directly or through a dependency", and E023 stays consent.

## Where things stand (main ed133de, 2026-11-16)

- Ceiling: `desugar::ceiling` (`yan-elab`) checks the module's own extern blocks and written `uses`
  against `capabilities` of the nearest `package.yag` above the file (`yan-db` `package_manifest`).
- Consent, compile side: `yan-db` `consented` walks up to the outermost `package.yag` and reads its
  `allow_ffi`. Consent, resolve side: `yan-cli` `lock::consent` over every chosen package (dep, dev and
  tool), from `Info.ffi`; a locked build reads the `ffi` array of `yan.lock`.
- The `ffi` fact has three producers: `yan-db` `is_ffi` (per module), `lock.rs` overrides
  (`dir.join("ffi").is_dir()`, modules only, no libraries), and `.info` (read, never written by
  `yan publish`, #1427 item 3). None of them lists extern libraries by name.
- Dependencies are resolved, fetched and verified into the store, but no command elaborates their
  modules; there is no cross-module or cross-package name resolution yet.

Two holes follow. A package whose dependency binds C behind `uses none` items publishes a confined
ceiling and is shown as confined (R673 never fires for it). And a hand-edited `yan.lock` with an empty
`ffi` array passes the locked-build consent check, since the lock is the only input.

## Decisions

### D1: what a dependency's foreign library costs the packages above it

Options:

- (a) Root only. The root's ceiling must list every library in the resolved graph; intermediate
  ceilings are unchecked. Simple, but a published package's ceiling then says nothing about its
  subtree, and the registry shows a package as confined while it ships C.
- (b) Closure by presence. Every package's ceiling must list every extern library declared by any
  package in its normal-dependency closure, itself included, whether or not its code calls into it.
- (c) Closure by reachability. Only libraries reachable through the call graph from the package's
  code (from the per-`pub`-item sets in `.info`, Capslock-style) count.

Recommendation: (b). Foreign code is unconfined (R673): its authority does not depend on which entry
point is called, library initializers run at load (and #1391 moves loading to startup), and C reaches
itself through callbacks and function pointers that no Yan call graph sees. Reachability (c) is
unsound for exactly this class and churns with every refactor of the dependency; it remains useful as
a report in `yan audit`, never as a grant. (a) leaves the misleading ceiling in place.

Confined capabilities are unchanged: they stay transitive by use. A dependency's `pub` item that
`uses net` makes its callers need `uses net`, and their package ceiling then needs `net` (E314 at the
caller), which follows from `uses` being part of the signature once dependencies are elaborated (D6).
Only extern items carry a trusted `uses` claim, so only foreign libraries need the presence rule.

`dev_dependencies` stay outside the ceiling (R636) but still need consent (R1019, unchanged). A `tools`
package is checked against its own ceiling, as its own root of the presence rule.

- Decision: accepted as recommended (lead, card, 2026-11-17)
- Row: R1026

### D2: how a transitive library is named in a ceiling

`lib zlib` declares trait `Zlib` and capability `zlib` in its own package, so two packages that both
declare `lib zlib` declare two capabilities, possibly linking different objects. A bare `zlib` in a
dependent's ceiling would cover both, and a new dependency could hide behind a library name the root
already grants for its own code.

Options: (a) bare names everywhere; (b) a package's own libraries bare, another package's libraries
qualified by package name, `"@a/b:zlib"`.

Recommendation: (b). Capability identity follows the declaring package as the trait does, so a
ceiling entry names one fact. `:` is free in ceiling entries (`.` is the attenuator separator,
`net.only`, and `/` is inside package names). Package name, not version, as in `allow_ffi` (R1018).
`yan add` and `yan update` print the entries a new library needs; they never write the ceiling.

- Decision: accepted as recommended (lead, card, 2026-11-17)
- Row: R1027

### D3: every package in the graph is checked, and where

Every package in the resolved graph is checked, not only the root: its ceiling against the libraries
of its own closure under the root's resolution. Places:

1. Resolution (`yan lock`, `add`, `update`, a locked build): E314 for the first package whose ceiling
   misses a closure library, naming the library and the dependency path from that package to the
   declaring one. `yan update` does not choose a version that would cause this; it holds it back and
   reports it, as it does for a capability gain (19.4, E005). `yan add` fails when no version fits.
2. Publish: the registry checks the computed set (19.2) including the closure under the lock the
   version was built with (E006). The consumer's resolution stays authoritative, since it may choose
   other versions.
3. Compiling the root or a workspace member: the same check over the package graph (D5), reported once
   per package on its `package.yag`, not per module.

Options weighed: root only (D1 a); resolution only, trusting the lock at build time. Recommendation:
all three, one function, so a lock edited by hand, a member added without `yan lock`, or an override
cannot skip the check.

- Decision: accepted as recommended (lead, card, 2026-11-17)
- Row: R1028

### D4: one computed foreign set, recomputed from source

A package's foreign set is `{ffi modules, extern libraries with their link names}`. It is computed by
one function from the package's source tree (only `<root>/ffi/` and `<root>/ub/` modules, whose extern
blocks are parsed with the front end), and every consumer uses that function: `yan publish` and the
registry for `.info`, overrides and workspace members at resolution, locked builds, and the compiler.
`.info` and `yan.lock` record it as a cache:

```toml
[[package]]
name = "@a/b"
ffi = ["ffi/zlib.yn"]
libs = [{ name = "zlib", link = "z" }]
```

A locked build recomputes the set from the verified archive in the store (the archive holds exactly
the files the compiler reads, 19.2) and fails with E020 when it differs from the lock. `.info` from a
registry is checked against the archive the same way after the digest check. This removes the
overrides copy in `lock.rs` and makes `is_ffi` a use of the same function (law 12).

Options weighed: trust `.info` (registry-computed, signed in the log) and the lock. Recommendation:
recompute, because the lock is editable and the pre-registry static index (R636) is unsigned; the cost
is parsing the `ffi/` modules of packages that have them.

- Decision: accepted as recommended (lead, card, 2026-11-17)
- Row: R1029

### D5: the package graph the compiler reads

A `PackageGraph` salsa input in `yan-db`, built by `yan-cli` from the workspace, `yan.lock`, the store,
`vendor/` and overrides: for each package its name, source root, manifest, foreign set (D4), and
normal, dev and tool edges, with the root and members marked. It replaces the ancestor walks:
`package_manifest` becomes a lookup of the package that owns a path, `consented` reads the root's
`allow_ffi` from the graph, and `is_ffi` uses the owning package's root. A tracked `graph_diagnostics`
query emits E023 (consent) and E314 (D1 to D3) per package. A single file outside any package gets a
graph of one package, so scripts and conformance cases keep working.

Options weighed: keep per-file ancestor walks and add a second walk for dependencies; compute the
closure only in `yan-cli`. Recommendation: the graph input, because the same structure is what
cross-package name resolution needs (D6) and both the resolver and the compiler then read one fact.

- Decision: accepted as recommended (lead, card, 2026-11-17)
- Row: none (implementation)

### D6: dependency modules, when they are compiled

When cross-package name resolution lands, a dependency's modules are elaborated with that package's
own manifest and `ffi` root from the graph, never the root's: E314 for its own items against its own
ceiling, E901 and E902 against its own `ffi/`. The importing package sees each `pub` item's `uses` in
its signature, which makes confined capabilities transitive by use (D1). That work belongs to the
module system and is out of scope here; this RFC fixes only that it reads the graph of D5.

- Decision: accepted as recommended (lead, card, 2026-11-17)
- Row: none (module system, gap #1452)

## Spec changes

7.4 (ceiling paragraph), 12 (`lib` paragraph: closure and qualified names), 19.1 (`capabilities` row:
qualified entries), 19.2 (R1020 sentence: presence rule, every package, three places; computed set
recomputed from source), 19.4 (update holds back a version that breaks a ceiling), lock fields (R945:
`libs`), `.info` fields (R895: `ffi`, `libs`), 25 (E314 and E020 wording; no new code).

## Conformance cases

On a `conform/*` branch with the rows: `pkg/` cases with a static-index registry double whose `.info`
carries `ffi` and `libs` (#1427 item 2): dependency library listed in the root ceiling (passes),
missing from the root ceiling (E314 with path), missing from an intermediate package's ceiling (E314
naming that package), same bare library name in two packages (the qualified entry is required),
`uses none` wrapper in a dependency (still E314), lock with an edited `ffi` or `libs` (E020 under
`--locked`), update that would add a library held back.

## Implementation (after acceptance)

Slices S1 to S6, filed under #1427 as #1446 to #1451. S1 the foreign-set function; S2 `.info` and lock fields and
`yan publish` writing them; S3 the `PackageGraph` input; S4 the closure check at resolution and update
hold-back; S5 the compile-side graph diagnostics and `yan audit` paths; S6 the conformance cases. D6 is
tracked as gap #1452.
