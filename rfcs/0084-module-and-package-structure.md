# 0084: Files, modules, directories and packages

- Status: accepted
- Research: [module, file and package structure, 2026-11-17](../docs/research/2026-11-17-module-package-structure.md)
- Rows: pending (numbered with the spec PR and its conformance cases)

## Summary

Spec 19 fixes what a package is (manifest, lock, registry, ceilings) and spec 15 says `use` only aliases a
module, but nothing says what a module is, how files and directories map to module paths, who may see
what, how a name in one file reaches a name in another, or where tests, examples and outputs live. No
command resolves a name across modules or packages yet (#1452; examples/tally gap 1), so this is designed
before it is built, and RFC 0079 D6 already waits on it.

The proposal in one paragraph: a file is a module and its path under `src/` is its name, with no
declarations, no `mod.yn`, no `__init__`; the layout of a package is fixed by convention and has no
configuration; visibility has three levels, private to the module, `pkg` for the package and `pub` for
the API; names stay always qualified, with no item imports and no globs; module cycles inside a package
are allowed and package cycles are not; workspaces share one lock and need no dependency inheritance
because version selection is minimal; tests live where they already do, inline, plus a black-box `tests/`
directory; generated state lives in one self-ignoring directory. It extends RFC 0020, 0044, 0053 and 0079
and contradicts none of them.

## Principles

Each decision below is checked against these. They come from the project's laws and from the survey.

1. File presence is the declaration. No list of files, modules or targets is ever written down.
2. A name resolves from the manifest and the file's own path, never from the working directory, a search
   path, file order or the surface it is written in.
3. One fact, one representation (practice 12): visibility, module path and package identity are each
   computed once and read by every tool.
4. Convention with no knob. Where a choice has one answer, the toolchain does not read configuration to
   find it.
5. Moves and renames are hash-neutral edits (3.6: names are trivia), so the structure may be refactored
   freely and mechanically.

## Where things stand (main ee38394, 2026-11-17)

- `yan run|check|test` load one file. Module names are file stems, so two files with one stem collide and
  tests are dropped silently (tally gap 11). Names such as `fees.fee` from another file fail with "not
  supported yet" (gap 1).
- `yan new` writes `package.yag` and `src/lib.yag` or `src/main.yag` (`crates/yan-cli/src/new.rs`).
- Foreign modules are the files under `<root>/ffi/` and `<root>/ub/` (`yan-db` `foreign.rs`).
- The package graph exists as a salsa input (`yan-db` `graph.rs`, RFC 0079 D5); a path is owned by the
  package whose root is its longest prefix. Roles are root, member, nested and locked.
- Standard library source is `yan-std/lib/{core,alloc,std}/`; the tier directory is the source root and
  is not part of the module path (17, `std.list`).
- Spec text: `use` is `"use" ["."] NAME {"." NAME} ["as" NAME]`, aliases only (15.2); a dependency is
  public when its types appear in a `pub` signature (19.1); the archive holds exactly the files the
  module graph reads (19.3); the manifest is `package.yag` today (19.1).

## Decisions

### D1: file, directory and module

A file is a module. Its path is its directory path under the package's source root, joined by `.`, with
the extension removed: `src/billing/invoice.yn` is `billing.invoice`. A directory is a namespace, never a
module, so there is no `mod.yn`, no `__init__`, no `index` and no declaration of any kind. A file and a
directory of one stem may sit side by side (`billing.yn` and `billing/`): `billing` is the module and
`billing.invoice` a child; neither needs the other. A module is `.yag` or `.yn` by its extension, and a
package may mix them freely, per file. Two files with the same path and different extensions, a file stem
or directory name that is not a value name (13.1), and a stem that is a reserved word, a Windows device
name or longer than 32 characters are errors at layout time, reported on the file. Names are lowercase
by the lexical rule, so case-insensitive file systems cannot collide.

The package's root module is `src/lib` (`.yag` or `.yn`); its items are the package's top-level names to
a dependent (`http.get`), and `src/billing/invoice` is `http.billing.invoice`. `src/main` is the default
binary (D2). Neither stem is a namespace for the others.

A module's items are the items of its one file. Module attributes (`@@`) and the docstring or `---`
header belong to the file.

Options weighed:
(a) the Go model, a directory is a package of many files sharing one namespace. It makes file names
meaningless, couples files by package-level names, forces a surface per directory (the tally package mixes
`.yn` and `.yag` in one directory), and its one benefit, a compile unit, is unneeded because Yan's cache
unit is the item.
(b) the Rust model, explicit `mod` declarations. Two edits per file, silent orphan files, and a second
source of truth for the module tree that tools must keep in step.
(c) the Python and Zig model, file is module, path is name (recommended). One representation, found by
`ls`, and the surface is chosen per file.
(d) a namespace file per directory (`mod.rs`, `__init__.py`, `index.js`). Rejected for the same reason as
(b), and it makes "what is in this directory" depend on a file with a magic name.

Recommendation: (c) with the directory-and-file sibling form. The module path is not part of an item hash
(3.6), so moving a file never changes a hash; the toolchain must keep that true when this lands.

- Decision: as recommended (lead, 2026-11-17: "the most correct long term option")
- Row: pending, numbered with the spec PR

### D2: the layout of a package is fixed

```
package.yd                 manifest, the marker of a package root
src/
  lib.yn | lib.yag         root module, the package's API facade (optional for a binary-only package)
  main.yn | main.yag       default binary (optional)
  <path>.yn | <path>.yag   modules
bin/<name>.yn|yag          further binaries, one per file, not importable
ffi/  ub/                  foreign and undefined-behaviour modules (12, D9)
native/                    C sources named by the manifest's native field (12.8)
tests/                     black-box tests (D8)
examples/                  runnable examples, checked in CI (D8)
benches/                   benchmarks (D8)
vendor/                    yan vendor output, committed (19.4)
.yan/                      generated state and workspace configuration (D10)
README  LICENSE  CHANGELOG optional, shipped in the archive
```

Nothing in it is configurable: no source path, no list of binaries, no list of targets. Targets are
inferred from the tree: `src/main` and each `bin/*` is a binary (the build study already infers `main` as
a binary), everything else under `src/` is library code. A binary's name is its file stem, `main`
taking the package name's last segment. Source roots are exactly `src/`, `bin/`, `ffi/`, `ub/`, `tests/`,
`examples/` and `benches/`; a `.yn` or `.yag` file elsewhere is not part of the module graph and `yan
check` says so once, rather than ignoring it. The standard library is the one other case: each tier
directory is a source root whose modules are named `std.<path>` (17), consistent with this rule because a
source root is not a namespace.

Options: (a) fixed layout (recommended); (b) Cargo-style defaults plus `[lib] path`, `[[bin]]` tables;
(c) a `modules` list in the manifest. (b) and (c) cost every tool and every agent a configuration read,
reintroduce a list of files (principle 1), and, as in Cargo, are used mostly by projects that later regret
it. Packages that need an unusual tree are two packages in a workspace.

Existing templates (`src/lib.yag`, `src/main.yag`, `package.yd`) already conform.

- Decision: as recommended (lead, 2026-11-17: "the most correct long term option")
- Row: pending, numbered with the spec PR

### D3: visibility

Three levels on items, fields and variants' fields:

- private, the default: visible in the module (the file) only. A child module does not see its parent's
  private items, and a parent does not see its child's; there is no `pub(super)`.
- `pkg`: visible to every module of the package, not to dependents.
- `pub`: the exported API (as now: contracts, capability sets, certificates and the semver check
  (20.6) are computed over `pub` items).

`pkg` is a keyword in the position `pub` is, one word, soft outside that position. There is no
`pub(path)`, no friend or `@testable` access, no per-module visibility attribute, and no `internal/`
directory rule: a `pub` item in any module is API, wherever the file sits. The API of a package is
therefore the set of its `pub` items, which `yan doc` and `yan api` print and which D5 aliases may
rename. A type with a private field cannot be constructed or destructured outside its module, so opaque
types need no further feature. A `pkg` or private item named in a `pub` signature is an error: a public
signature may only name types a dependent can name.

Amendment (D3, impl-item visibility): methods of a trait and of an `impl` take no modifier and inherit
visibility: a method is public iff its trait or its implementing type is `pub`, and the rule above
applies to its signature (decided by the lead, 2026-11-21, #1608 item 3).

Options: (a) private, `pkg`, `pub` (recommended); (b) two levels (private, `pub`) as now, with sharing
inside a package done by making things `pub`; (c) Rust's `pub(crate)`, `pub(super)`, `pub(in path)`;
(d) Go's `internal/` by directory; (e) an export list in the module header (Roc, Haskell). (b) makes
every shared helper part of the API, which the registry then computes capabilities for and the semver
check then freezes; every language with two levels grew a third (Rust `pub(crate)`, Swift `package`,
Gleam `internal_modules`). (c) is the same level at four times the tokens and three of its four forms are
rarely used. (d) makes a file move change the API, a risk Go lives with and Yan need not. (e) puts a
second list in each file (principle 1).

Recommendation: (a). Language rows: 15.2 grammar, 3.5 Yan Core `pkg` modifier (a Core item carries
private, `pkg` or `pub`; the hash includes it, as it includes `pub` today).

- Decision: as recommended (lead, 2026-11-17: "the most correct long term option")
- Row: pending, numbered with the spec PR

### D4: names, `use` and what is in scope

Names stay always qualified. There is no item import, no glob, no `from m import a`, and no import
ordering. The lenient reader keeps rewriting `import m` and `from m import a` to qualified names. `use`
creates an alias for a module, nothing else:

- `use billing.invoice` binds `invoice`; `use billing.invoice as inv` binds `inv`.
- `use .invoice` is the sibling form: the leading dot means "relative to the directory of this file",
  one level, never `..`. From `src/billing/tax.yn`, `use .invoice` is `billing.invoice`. A leading dot
  inside `src/lib` or `src/main` names a child of the root.
- `use` lines sit at the top of a file, sorted by `yan fmt`, one module per line.

A leading name resolves, in one fixed order, from four roots that must not collide:

1. local bindings and the file's `use` aliases;
2. `std` and `core` (the prelude is the one set of unqualified names, as now);
3. the dependency local names of the manifest (`http`, RFC 0020 D1);
4. the package's own top-level modules (`schema`, `fees`, `billing`, and `lib` for the root facade), plus
   the reserved roots `ffi` and `ub`.

A collision between roots 3 and 4, or an own module named like a prelude name, `std`, `core`, `alloc`, `ffi`
or `ub`, is an error at layout time that names both sides. Nothing shadows anything (principle 2): a local
`random.yn` cannot silently replace a dependency, and a dependency added later cannot silently capture a
name already in use, because adding it fails until one side is renamed. The new std modules are always
reached as `std.x`, so a new std module never collides with a user module.

A method call resolves through the receiver's declaring module (17), unchanged. Trait implementations are
global facts of the graph (a type's impls are those of its declaring module, the trait's module and, per
the coherence rules of spec 10, nowhere else), so no import carries an impl.

Options: (a) qualified names, module aliases only (recommended; it is what spec 15 and the lenient reader
already do); (b) add `from m use a, b` item imports (Rust `use`, Python `from`, Gleam unqualified
imports); (c) globs. (b) makes a name's origin invisible at its use site, which is the property agents and
reviewers read code for, costs a tool to keep the import list in step with the body, and makes a refactor
move names between lines instead of editing one qualifier; it also lets two imports collide on a bare
name. (c) is the classic source of capture bugs (a dependency adds an export and a name changes meaning).
The cost of (a), longer lines, is paid with `use ... as` and is bounded by law 11 because a qualifier is
one token.

Recommendation: (a). The spec edit is 15.2's `use` paragraph, the name-resolution order in 3.5 and 17, and
the layout-time collision error.

- Decision: as recommended (lead, 2026-11-17: "the most correct long term option")
- Row: pending, numbered with the spec PR

### D5: re-exports

`pub use path.to.module as name` re-exports a module and `pub use path.to.module.item as name` an item,
under a new name or its own. There are no globs, no `pub use` of more than one name per line, and no
implicit re-export of a dependency. A re-export is an alias in the name table, not a new item: it has the
same hash and the same capability set, `yan doc` shows the item once at its defining path and lists the
aliases, and the semver check treats an alias removal as an API removal. A dependency that a `pub`
signature mentions is public by inference (19.1) with no `pub use` needed; `pub use` exists only to
present a curated facade from `src/lib`, so that a package can keep its files in `src/billing/*` and
expose `billing.total` without exposing the file layout.

Options: (a) explicit aliases of one name each (recommended); (b) no re-export (a dependent then depends
on file layout, so every refactor is a breaking change); (c) glob re-export and preludes (Rust,
Haskell), whose API is a set computation a reader cannot do by eye.

- Decision: as recommended (lead, 2026-11-17: "the most correct long term option")
- Row: pending, numbered with the spec PR

### D6: cycles

Between packages, cycles are impossible by construction (a requirement names a published version) and
`dev_dependencies` may not reach the package itself, directly or through the graph. A dev dependency that
depends on the package under test would produce two builds of one package in one test binary with
distinct types (Cargo's known trap), so it is an error naming the path. Between modules of one package,
cycles are allowed. Yan modules have no import-time effects (top-level code is constants evaluated at
stage 0 under the hermetic rules), the elaborator orders items and not modules, and recursive item groups
are already hashed as one (3.6); the failure modes that make cycles costly elsewhere (Python import-time
execution, Haskell and OCaml per-module compile order) do not exist here. A genuine cycle among
initializers or among by-value type definitions is an error on the items, as today. `yan tree
--cycles` lists module cycles and the lint set (18.2) gains an opt-in lint for projects that want layering
enforced.

Options: (a) allow, with a report and an opt-in lint (recommended); (b) forbid (Gleam, Elm, OCaml,
Haskell): a cleaner dependency picture, at the price of an unrelated refactor to extract a shared module
before two functions can call each other, and `.hs-boot`-style escapes when it bites; (c) forbid by
default with an opt-out. Because the stated harm of cycles does not exist in Yan, forbidding them would
be a rule with a cost and no defect to prevent.

- Decision: as recommended (lead, 2026-11-17: "the most correct long term option")
- Row: pending, numbered with the spec PR

### D7: manifest, workspaces and sibling packages

The manifest stays `package.yd` (RFC 0044 D1) and gains no layout fields (D2). A workspace root lists
`members` as directory paths or a single trailing glob (`"crates/*"`); every member has its own
`package.yd`, and the workspace has one `yan.lock`, one store and one cache. A member refers to a
sibling by ordinary requirement, `"core": "@acme/core 1.2"`. While a member with that name and major
exists in the workspace, resolution binds the requirement to the member; the manifest never contains a
path. At publish the requirement is already the registry requirement, so there is no `path` plus
`version` pair to keep in step, and a workspace with no registry still resolves. A requirement that the
member's version does not satisfy is an error, not a silent fall back to the registry. `yan publish`
publishes members in dependency order.

There is no workspace dependency table and no version inheritance: version selection is the maximum of
the minimums and identity is (name, major) (19.1), so two members requiring `@a/b 1.2` and `@a/b 1.4`
resolve to one version without being told to. Members may differ in `edition`. The ceiling, consent and
supply-chain policy fields remain read in the root only (19.1), so a member cannot loosen the root.

Members must be cheap, because the package is the unit of capability ceiling (D9) and the way to give a
region of code a smaller ceiling is to make it a package. A member is a directory and a manifest of three
lines; `yan new --member` writes both.

Options: (a) as above (recommended); (b) Cargo's `path` dependencies plus `workspace.dependencies`
inheritance; (c) one package with per-directory ceilings. (b) is the double bookkeeping the survey found
in Cargo and Go (`replace`), and 19.1 already refuses path and git dependencies outside `overrides`. (c)
adds a second ceiling mechanism and a second consent unit beside the package.

- Decision: as recommended (lead, 2026-11-17: "the most correct long term option")
- Row: pending, numbered with the spec PR

### D8: tests, examples and benches

- Unit tests stay inline: `test_` functions and `ex` clauses in the module, with access to the module's
  private items. They are stripped from a dependent's build and from the closure used for capability
  computation, as today.
- `tests/<path>.yn|yag` are black-box tests: a test module sees the package exactly as a dependent does
  (its own package under the local name of its manifest `name`'s last segment, and `pub` items only) plus
  `dev_dependencies`. Each file is one module; `yan test` runs every `test_` function and `ex` clause in
  it. Helpers shared between tests go in `tests/support/*` and are plain modules of the `tests` root.
- `examples/<name>.yn|yag` are binaries built by `yan check` and `yan test` (so examples cannot rot) and
  run by `yan run --example <name>`. They see only `pub` items and `dependencies`, which keeps them honest
  as documentation.
- `benches/<name>.yn|yag` are binaries run by `yan bench` under the instruction-count harness.
- None of `tests/`, `examples/`, `benches/` or the inline test code is read by a dependent. The published
  archive holds `src/`, `bin/`, `ffi/`, `ub/`, `native/`, the manifest and the files named in 19.3,
  and omits `tests/`, `examples/`, `benches/` and `vendor/` unless the manifest lists fixtures. Inline
  tests ship with their modules.

Options: tests only inline (Zig, today); tests in a mirrored tree (Python, Java); tests beside files as
`x_test.yn` (Go). Inline tests give the author private access and a failing `ex` next to its function.
They cannot show how a dependent sees the package, which is what `tests/` is for. A mirrored tree and a
`_test` suffix add a convention with no property that the two forms above lack.

- Decision: as recommended (lead, 2026-11-17: "the most correct long term option")
- Row: pending, numbered with the spec PR

### D9: FFI, capabilities and the layout

Foreign modules stay under `ffi/` and `ub/` at the package root and nowhere else (RFC 0077, 0079 D4).
Their module paths are `ffi.<path>` and `ub.<path>`; `ffi` and `ub` are reserved roots (D4), so the path
itself tells a reader that a name is foreign, and a path-based fact is already how the compiler decides it
(`in_ffi_dir`). A safe wrapper is a `pub` item in an `ffi` module or, for a curated API, a `pub use` of
it from `src/lib`. C sources named by the manifest's `native` field live in `native/` and are shipped in
the archive; no other directory may hold a C source. The capability ceiling, consent (`allow_ffi`),
and the computed capability set are per package and are not attached to directories or modules. A
module-level ceiling is not introduced: the way to give a region of code a smaller ceiling is a member
package (D7), so there is one ceiling mechanism, which RFC 0079's closure check, the registry and the lock
already read. Cross-package visibility of a foreign library is as RFC 0079 D1 to D3 (library capabilities
named `"@org/name:lib"` in a ceiling); this RFC adds nothing there.

- Decision: as recommended (lead, 2026-11-17: "the most correct long term option")
- Row: pending, numbered with the spec PR

### D10: build outputs and generated state

Everything the toolchain writes lives under one directory, `.yan/`: store, cache, bundles, outputs and the
write-ahead state of `yan lock`. `.yan/config.yag` (19.1) is the one tracked file in it. `yan` creates
`.yan/.gitignore` containing `*` and `!config.yag` (and the equivalent hint for other tools), so no user
edits the project's own ignore file and no second ignore list can drift. Final artifacts of a build go to
`.yan/out/<profile>/<target>`; `yan build -o <path>` writes one binary elsewhere, `yan run` and `yan test`
write nothing outside `.yan/`. No file is written beside source, no `target/`, no `dist/` by default. The
content-addressed object store and global store are as in the build-system study (D4 there); this RFC
fixes only where the user-visible parts are. A command that would write outside `.yan/` or `-o` is a
defect.

Options: (a) one self-ignoring directory (recommended); (b) `target/` (Cargo) or `zig-out/` plus
`.zig-cache`, each needing an ignore line and each large; (c) global only (Go), where outputs have no
home to inspect.

- Decision: as recommended (lead, 2026-11-17: "the most correct long term option")
- Row: pending, numbered with the spec PR

### D11: tooling

Every structural operation is a subcommand of the one `yan` binary, derived from the same layout model
(principle 3), with no per-ecosystem helper tools:

- `yan new [--bin|--lib|--member]`, `yan init`: write the D2 tree (the templates already do).
- `yan tree` prints the package graph and module tree (`--cycles`, `--why`, `--public`, existing
  dependency views extended to modules).
- `yan mv a.b c.d`: moves a module file and rewrites every qualified reference and `use` in the
  workspace. It is a mechanical edit because names are always qualified (D4) and hash-neutral (D1), so
  the patch protocol can verify it by comparing item hashes before and after; `yan mv --item` likewise.
- `yan fmt` stays zero-option; it also sorts `use` lines and reports a file outside every source root.
- `yan doc` renders `pub` items from the name tables, with aliases (D5), and checks every `ex`
  clause; `yan api` prints the signature view of a package (the replacement for a separate interface
  file such as `.mli`); the semver check of 20.6 reads the same set.
- `yan check` reports layout errors (D1 to D4), cycle summaries and files outside source roots.
- `yan.lock` and `yan add` are as RFC 0044 and 0053.

- Decision: as recommended (lead, 2026-11-17: "the most correct long term option")
- Row: pending, numbered with the spec PR

### D12: migration from today's layout

Nothing breaks: today's packages (`package.yag`, `src/lib`, `src/main`) are already in the D2 shape, and a
lone script or `.yir` file still works as a package of one module whose path is its file stem (and, for
`.yir`, the explicit path in its `module` line, which must match). Steps, each a slice behind the one
before it:

1. Layout model: the one function that maps a path to (package, source root, module path) in `yan-db`,
   replacing the file-stem rule and the ancestor walks that RFC 0079 D5 left, with the layout-time errors
   of D1 and D4 and `yan tree`. Fixes the stem-collision bug (tally gap 11).
2. Cross-module names inside one package: qualified resolution over the D4 roots, `use` with the leading
   dot, `pkg` and the visibility checks, on both surfaces (closes tally gaps 1 and 16, and #1452 item
   one for the root package).
3. Cross-package names: dependency modules elaborated with their own manifest and ceiling from the
   package graph (RFC 0079 D6), `pub use`, public-dependency errors, type-identity messages across majors.
4. Workspaces: member binding by requirement, the dev-dependency cycle check, publish order.
5. `tests/`, `examples/`, `benches/` and the archive filter; `.yan/.gitignore`; `yan mv`.

Each slice lands with its conformance cases in `conformance/pkg` (multi-file package cases), which the
spec's case-with-row rule requires with the rows. Rows are numbered in the PR that writes the spec. The
tally package under `examples/tally/package` becomes the golden acceptance case for slices 1 to 3.

- Decision: as recommended (lead, 2026-11-17: "the most correct long term option")
- Row: pending, numbered with the spec PR

### D13: manifest and configuration are data files with their own extension

The manifest needs no computation and has none: `package.yag` is one typed constant built from literals,
read without running code, and anything else in it is an error (data profile, RFC 0033; spec 19.1 and
17.3). Nothing about it is Turing complete or dynamic. The problem is the name. A manifest spelled
`.yag` looks like code, so a reviewer, a scanner or an editor cannot tell from the path that opening or
building a dependency's manifest executes nothing, and `yan run package.yag` is a meaningful command
today. RFC 0033 D1 chose "a profile of Yan Agent with no extension of its own" to avoid a new surface; that
reasoning is about surfaces (a new surface adds meaning), not about whether data and code should be
distinguishable by name, and it did not weigh the cost above.

Proposal: a data file is a file with the extension `.yd` (the lead may choose the spelling; it must be
checked against existing registered extensions before it is frozen). It is parsed by the Yan Agent
parser and checked by the data-profile checker, so every benefit of the typed constant stays: constructors
checked against the `Package`, `Lints`, `Config` types, the unknown-field rule, a lossless tree for
`yan add` and `update`, one canonical printer, `parse[T]` and `dump` (RFC 0033). What changes is that the
profile is selected by the extension, not discovered by analysing a `.yag` file: a `.yd` file that
contains a function, an import, a call or a name that is not a constructor is an error on that line, a
`.yd` file is never run, compiled into a program or reachable from a module's name resolution, and
`yan run` refuses it by name. Tools, editors, linguists and reviewers know "data, executes nothing" from
the path alone.

Names that follow: `package.yd` (the manifest, the only name read), `.yan/config.yd` (19.1 configuration),
and any user file read by `std.data`. `yan.lock` stays TOML, machine-written and read by outside scanners
(RFC 0044 D3); `std.toml` stays for interop. A single-file script still carries its `Package` constant
in the script, because a script is code by nature; the script's constant is the same data-profile subset,
and the toolchain reads it without running the script. `yan metadata --json` prints the resolved graph,
each package's manifest fields and its module tree for editors and dependency bots, so outsiders never need
to parse `package.yd`.

Where D2 and D7 say `package.yag`, `package.yd` is meant.
Pre-0.1.0 there is no alias: a directory with `package.yag` and no `package.yd` is an error that names the new
file, and a `yan fix` step renames it; the old spelling is never read (fail loud).

Options:
(a) `package.yd` and `.yd` data files (recommended). Visibly data, one checker, no new parser or surface, typed
constructors and lossless editing kept. Cost: one more extension in the frozen-name list (the lead's call) and
a rename of the manifest and the configuration file.
(b) keep `package.yag` (RFC 0044 D1, RFC 0033 D1). Cheapest; leaves the executes-code impression and the
`yan run package.yag` question, which a lint can only paper over.
(c) a TOML manifest (`yan.toml`). Visibly data and familiar, but `Package`, `Lints`, `Python`, `I18n` and
`net.only(...)` become untyped tables that a second schema checker must follow; a script's manifest keeps a
second spelling; `yan add` needs a second lossless editor, which the self-hosted compiler would carry in Yan;
and Yan's own packages would not use the data format RFC 0033 gives the language.
(d) a neutral fixed filename with the `.yag` extension (for example `yan.package.yag`). The name still reads
as code.

Recommendation: (a). It meets the criterion that the manifest is visibly not code, keeps the one checker and
the lossless editor, and removes a class of "does this run?" questions. It does amend RFC 0033 D1 and the
extension list, which is why it is a card.

- Decision: (a), `.yd` data files and `package.yd` (lead, card, 2026-11-17). Spelling `.yd` confirmed (lead, 2026-11-17); no collision in github-linguist `languages.yml` on that date.
- Row: pending, numbered with the spec PR

## Rejected as a whole

- A content-addressed codebase with no source files (Unison): review, grep and agent editing would
  depend on one tool. Item hashes are used for identity and cache, not as the source of truth.
- Build files in the tree (dune, Bazel, build.zig, build.rs): code in the build is the supply-chain
  surface 19.5 removes.
- Optional features and conditional dependencies: already a non-goal for 0.1.0 (19.1).
- URL imports and VCS-path identities: 19.3 has a registry with a transparency log and `overrides` for
  the one case that needs a git commit.
- Separate interface files: `pub`, `pkg` and `yan api` give the same view without a second file to keep
  in step.

## Spec changes

3.5 and 3.6 (`pkg` modifier, module path not in the hash); 13.1 and 15.2 (`pkg`, `use` forms, `pub use`);
17 (name-resolution order and roots, source roots for std); 18.1 (commands `yan tree`, `yan api`,
`yan mv`, `--example`, `--member`; the `-o` rule); 19.1 (layout subsection, `yan metadata`, extension `.yd`: the tree of D2, workspace
member binding, dev-dependency cycle rule); 19.3 (archive contents by directory); 25 (new diagnostics for
the layout errors of D1, the root collision of D4, a private or `pkg` name in a `pub` signature, the
dev-dependency cycle and a file outside every source root; codes taken at numbering time); 12 (`ffi.`
and `ub.` module paths); 20.4 if the edition gate for `pkg` is wanted.

## Conformance cases

On a `conform/*` branch with the rows: `pkg/` multi-file packages for each layout error; a two-surface
package that resolves qualified names both ways; private, `pkg` and `pub` access from a sibling module, a
child, a dependent and a test; a root collision between a dependency and an own module; a module cycle
that checks clean and a by-value type cycle that does not; a `pub use` alias with equal item hashes; a
workspace whose members require different minimums; a dev-dependency cycle; `yan mv` leaving every item
hash unchanged.

## Open points the lead may want to settle outside the cards

- Whether `pkg` should be a hard keyword (a one-time reserved-word cost) or soft; recommended soft, with
  `pkg` position-only.
- Whether `lib` should instead be named after the package (Gleam, Julia): recommended not, because a
  package rename must not rename a file.
