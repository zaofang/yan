# Module, file and package structure: what to keep and what to avoid

Study for RFC 0084. Written 2026-11-17 against main ee38394. It is a survey from the authors' working knowledge of each ecosystem plus a check of recent changes (PEP 810, Zig's package manager, Swift SE-0386, Deno's HTTP-import retreat, Gleam's internal modules); no claim here was benchmarked. Where a claim could not be rechecked it is phrased as a design lesson, not a fact about a version.

## What Yan already fixes

These are constraints, not options (spec 3.6, 13, 15, 17, 19; RFC 0020, 0044, 0053, 0079):

1. Two surfaces, `.yag` and `.yn`, mixed in one package, elaborating to one Yan Core. The file extension picks the surface.
2. Items are content-addressed (3.6). Names are per-item trivia in a name table; local names never reach a hash.
3. There are no import statements in either surface. Names are qualified (`schema.Grocery()`); `use` only aliases a module; the lenient reader rewrites `import m` and `from m import a` to qualified names (N01). Method lookup goes through the receiver's declaring module (17), so no trait or method import exists.
4. `pub` marks the exported API. The registry computes the capability set per `pub` item (19.2), and a dependency is public when its types appear in a `pub` signature (inferred, 19.1).
5. Manifest `package.yag`, literal-only Yan data. Names `@org/name`, identity (name, major), minimal version selection, TOML `yan.lock`, dependencies bound to local names, `dev_dependencies`, `tools`, `overrides`, `members`. No optional features, no build scripts, no install scripts, hermetic stage 0.
6. The package is the unit of capability ceiling and of foreign-code consent; `ffi/` and `ub/` at the package root hold foreign modules (12, RFC 0077, 0079).
7. Tests are `test_` functions and `ex` clauses inside modules; `dev_dependencies` are visible only to tests, fixtures, benches and doc fences.
8. One binary `yan`; zero-option formatter; a salsa query database; `.yan/` holds config, store and cache.

Nothing resolves a name across modules or packages yet (#1452, examples/tally gap 1), so the module system is a greenfield on top of these constraints.

## Per-ecosystem notes

Format: keep, avoid, each with the reason.

### Rust and Cargo
- Keep: a file maps to a module and a directory to its children (2018 `a.rs` beside `a/`); one lock and one build cache for a workspace; `pub(crate)` as a concept (a level between private and exported); inline unit tests with private access.
- Avoid: `mod x;` declarations (a file nobody declared is silently ignored; every new file is two edits); `mod.rs`; `use crate::`, `super::`, `self::` prefixes; `pub` inside a private module meaning "crate-visible" (visibility depends on reachability, and `unreachable_pub` exists to lint it); `path` plus `version` double bookkeeping for workspace siblings; features (unification across the graph, combinatorial test matrix, additive-only rule that is routinely broken); `build.rs`; `target/` inside the tree; configurable `[lib] path`/`[[bin]]` tables; highest-compatible version selection with a lockfile as the only reproducibility.

### Go
- Keep: minimal version selection; the import path as identity; `internal/` as proof that location can carry a visibility rule; one tool for fmt, test, doc, vet; checksum database with a transparency log (Yan's 19.3 is the same shape); tests beside code; `gofmt` as the zero-option precedent.
- Avoid: directory = package (file names carry no meaning, any file can silently depend on any other file's package-level names, init order is a hidden algorithm, packages grow to hundreds of files); `init()` functions; import paths tied to VCS hosts; `/v2` suffix in every import statement of every user; `replace` directives that leak; capitalization as visibility (invisible in a non-Latin script, a rename on every visibility change).

### Python
- Keep: one file one module with dotted paths; `import m` plus `m.name` as the dominant style; PEP 751 `pylock.toml` and uv as the lock precedent; PEP 723 inline script metadata (Yan's single-file scripts match); PEP 810 explicit lazy imports confirm that import-time work is the thing people pay for.
- Avoid: `__init__.py` and namespace-package rules; `sys.path` and the working directory deciding what a name means (a local `random.py` shadows the standard library); import-time execution, which is what makes cycles hurt; `from x import *`; `..` relative imports; distribution name differing from import name (`pillow`, `PIL`); packaging fragmentation across setuptools, poetry, pdm, conda and pip.

### Zig
- Keep: file is a struct is a module; no hidden global state; `build.zig.zon` content hash as identity and a fingerprint so a renamed or forked package cannot claim an old identity; lazy dependencies fetched on first need; inline `test` blocks; `zig fmt`.
- Avoid: `build.zig` as an arbitrary program (Yan forbids build scripts); relative file-path strings as import identity (`@import("../x.zig")`); no registry, so discovery and trust are left to the user.

### Swift
- Keep: SE-0386's `package` level exists because `internal` (module) was too narrow and `public` too wide once packages held several targets. Yan's package is that unit, so the level is justified, and it is cheap when written as one word.
- Avoid: six levels plus `open` and `@testable` back doors; a `Package.swift` that is a program; one directory per target.

### Deno, JSR and JavaScript
- Keep: static ESM `import` (analysable without running code); import maps as local names (Yan's `dependencies` local names are the same idea, bound in the manifest); JSR's scoped `@scope/name`, source publishing and the `deno.json` surface (Yan's manifest is already smaller).
- Avoid: URL imports as the primary mechanism (Deno's own retrospective retreats from them); `node_modules` upward directory walking; phantom dependencies; `package.json` `exports` condition matrices and CJS/ESM duality; extensionless and `index.js` magic resolution.

### Gleam and Elixir
- Keep (Gleam): fixed `src/`, `test/`, `dev/` layout with no configuration; file is module, path is name; qualified imports with unqualified opt-in; no module cycles; an `internal_modules` list that hides modules from dependents; Hex immutability.
- Avoid (Gleam): the package root module is named after the package, so renaming a package renames a file.
- Elixir: avoid module names unrelated to file paths (any file can define any module), compile-time versus runtime dependency subtleties that need a tool to explain, and umbrella apps (the ecosystem has since moved away from them). Keep `mix` as one tool for everything.

### OCaml and dune
- Keep: file is module and `lib/foo.ml` becomes `Lib.Foo` through wrapping; no cycles; the opam lock idea.
- Avoid: a `dune` build file in every directory; `.mli` as a second file that must be kept in step with the first (Yan derives the interface from `pub` and shows it through `yan doc`); cycles forbidden without escape.

### Haskell
- Keep: module name must equal file path; PVP bounds as a vocabulary for compatibility.
- Avoid: `exposed-modules` and `other-modules` lists that must name every module; `.hs-boot` files to break cycles; orphan instances; a global `Data.*` namespace.

### Nim
- Keep: one terse marker for export; file is module.
- Avoid: `include` (textual splice); style-insensitive identifiers.

### Roc
- Keep: separating application from platform, which Yan already has as `platform` capability bundles; an explicit exports list in the module header as an alternative to per-item markers (weighed in RFC 0084 D3).
- Avoid: URL packages with no registry as the only story.

### Unison
- Keep: content-addressed items; names as metadata so renames and moves never break a dependent; hashes as the cache key. Yan already has all three (3.6).
- Avoid: no source files at all. The codebase is a database that only its own tools can read, which makes review, diff, grep and agent editing depend on one tool. Yan stays text and uses hashes for identity and caching only.

### Julia
- Keep: `Project.toml` plus a separate manifest; the root module named by the package (as Gleam); `test/` by convention.
- Avoid: `include("file.jl")` (textual, order-dependent); `using` that brings names into scope and creates conflicts when a dependency adds an export; package extensions and weak dependencies.

### Java JPMS
- Keep: strong encapsulation by default; `requires transitive` is the same fact as Yan's inferred public dependency.
- Avoid: a second declaration file (`module-info.java`); split-package rules; automatic modules; reflection back doors (`--add-opens`) that every real project ends up using.

### Bazel
- Keep: hermetic actions, content-addressed remote caching, target-level visibility, and bzlmod's move to minimal version selection.
- Avoid: a `BUILD` file in every directory with hand-written dependency lists; a second programming language for the build; label syntax in everyday source.

## Cross-cutting findings

1. Every system that keeps a declaration list of files (`mod x;`, `exposed-modules`, `include`, `BUILD`) pays an edit-per-file tax and gets orphan-file bugs. File presence should be the declaration.
2. Three visibility levels (module, package, exported) are the stable answer: Rust has `pub(crate)`, Swift added `package`, Go has `internal/`, Gleam has `internal_modules`. Every system with fewer levels grew a back door.
3. Name resolution that depends on the working directory, a search path or the order of files (Python, Node, Go package-level names) is the largest source of "works on my machine". A name should resolve from the manifest and the file's own path.
4. Cycles hurt where import has effects (Python) or where compilation is ordered by module (Haskell, OCaml). Neither holds for Yan: modules have no import-time effects, and the compiler orders items, with recursive groups hashed together (3.6).
5. Configurable layout (`[lib] path`, `[[bin]]`, `exposed-modules`) buys nothing and costs every tool and every agent a configuration read. Convention without a knob is cheaper.
6. Dual bookkeeping (Cargo `path` plus `version`, `go.mod` `replace`, workspace dependency tables) exists because version selection is not canonical. With minimal version selection and one lock it is not needed.
7. Build scripts and install scripts are the largest supply-chain surface in every ecosystem that has them. Yan has none.
