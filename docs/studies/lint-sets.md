# Package-declared lint sets

Study and proposal, 2026-11-04. Docs only: no spec rows, no conformance edits, no diagnostic codes.
Items are labelled D1, D2 and so on; none carries a row number. Everything here is non-normative until
an RFC and accepted rows say otherwise.

Brief: item F9 of the language forums study.
Should `package.yag` name a lint set, std lints at chosen levels plus `@forbid` sets, that
`yan check` and `yan test` apply to the whole package? Cover prior art, how it fits the existing lint
rules (18.2), `@expect` and `@deny` (R797), profiles (R320), editions (R348) and the agent patch
protocol, then recommend and list open decisions.

## 1. Answer

Yes, as one literal manifest field, `lints`, that can only make the package stricter:

    package: Package = Package(
        name="@acme/ledger",
        version="0.4.0",
        lints=Lints(strict=True, deny=["W811", "W834"], forbid=["rc", "panic"]),
    )

- `strict=True` makes every build of the package's own code behave as `--strict`.
- `deny` makes each named W8xx lint an error in every build mode, as if every item carried `@deny`.
- `forbid` puts `@forbid(...)` on every function of the package, as proof obligations (5.7).

There is no package-level `allow`. `@expect(code, reason=...)` on an item stays the only suppression,
so every exception is local, has a reason, and is reported by W810 when it goes stale. The set is
applied identically by `yan check`, `yan test`, `yan build` and the agent tools, never applies to
dependents, and a patch that weakens it is a protected change. User-written analyzers stay out until
after 1.0.

## 2. The gap today

1. Lints warn by default and deny under `--strict` and `--level release` (18.2). Strictness is a
   command-line flag, so it lives in each CI script and editor setting, not in the package. Two
   people running `yan check` on the same commit can get different verdicts.
2. `@deny(code)` (R797) raises one lint inside one item. A team that wants W811 (wildcard on a sum)
   denied everywhere has to write it on every item, and a new item is unprotected by default.
3. `@forbid(rc, alloc, copy, panic)` (R447) is per item. Profiles (R320) apply restrictions to a
   whole program, but only std defines profiles, and a profile also fixes the platform and handlers,
   so it is too large a hammer for "this library never panics".
4. `yan test` does not promise to report lints at all, so a CI that only runs tests misses them. This
   is the Go complaint behind the forum thread: vet runs a fixed subset under `go test`, and the
   rest needs a second tool with its own configuration.

## 3. Prior art

| System | Where the set lives | Levels | Sharing | Notes |
| --- | --- | --- | --- | --- |
| Cargo `[lints]` (Rust 1.74, RFC 3389) | `Cargo.toml` | allow, warn, deny, forbid, with priority | workspace only, opt-in with `lints.workspace = true` | never applies to dependencies (cap-lints); groups like `clippy::all` pull in new lints on upgrade |
| Rust custom lint profiles ([RFC PR 3926](https://github.com/rust-lang/rfcs/pull/3926)) | `Cargo.toml` | named profiles selected per command | workspace | the forum source; motivated by different sets for CI and local work |
| Go vet under `go test` ([issue 82464](https://github.com/golang/go/issues/82464)) | none; fixed subset in the toolchain | pass or fail | none | asks for workspace analyzers in `go test`; golangci-lint fills the gap with a separate YAML file |
| Swift 6.2 (SE-0443, SE-0480) | `Package.swift` `swiftSettings` | `treatWarning(_, as: .error)`, `treatAllWarnings` | none | per diagnostic group; package settings never apply to clients |
| C# / .NET | `.editorconfig`, `Directory.Build.props` | none, suggestion, warning, error | directory inheritance | `AnalysisLevel` pins the rule set to an SDK version so upgrades do not add errors |
| Kotlin | Gradle `allWarningsAsErrors` | warning or error | build logic | all or nothing |
| Ruff | `pyproject.toml` `select`, `ignore`, `extend` | on or off | `extend` from a path | rule selection by prefix is the main footgun: new rules under a selected prefix appear on upgrade |
| ESLint flat config | `eslint.config.js` | off, warn, error | shareable configs are npm packages | configs are code, so reading one runs it |
| Zig | none | unused names are errors | none | no warnings exist, so there is nothing to configure |

Three lessons carry over:

1. **Keep the set beside the package, not in CI.** Every system that moved configuration into the
   manifest (Cargo, Swift, .NET) did it so a checkout gives one verdict everywhere.
2. **Groups and prefixes break upgrades.** Cargo groups, Ruff prefixes and ESLint `recommended` turn
   a toolchain upgrade into new errors. .NET solves it by pinning the set to a version. Yan already
   has the rule (R348): a new lint only warns until the next edition, even under `--strict`.
3. **Sets that are code, or come from other packages, are a supply-chain surface.** ESLint configs
   run JavaScript; Cargo deliberately limits inheritance to the workspace.

## 4. Proposal

### 4.1 The field

`lints: Lints` in `package.yag`, built from literals like the rest of the manifest (19.1), so tools
read it without running code. Fields, all optional:

| Field | Meaning |
| --- | --- |
| `strict: bool` | every build of the package's own code behaves as `--strict` |
| `deny: list[str]` | W8xx codes that are errors in every build mode inside the package |
| `forbid: list[str]` | any of `rc`, `copy`, `panic`: `@forbid` on every function of the package |

A code in `deny` that is not a W8xx lint, or a retired one, is an error at manifest read, the
manifest analogue of the existing `@deny` check. `alloc` is not accepted in `forbid`: leaving `alloc`
out of the capability ceiling already says that, and the manifest should not have two ways to say
one thing (D5).

### 4.2 Only stricter

The set raises severity and never lowers it. There is no `allow`, no `warn` that undoes `--strict`,
and no way to turn a lint off for the package. This keeps the 18.2 rule that `@expect` is the only
suppression, and it keeps every exception visible next to the code it excuses.

An item may still `@expect` a code the package denies, with its reason (D3). That is the deliberate
difference from item `@deny`, where an inner `@expect` is an error: a package-wide deny is a default
for a large body of code, and without a local escape hatch teams will drop the code from the list
instead. An item-level `@deny` keeps its stronger meaning.

### 4.3 Editions and new lints

Naming a code is an explicit opt-in, so a code in `deny` is denied even if it was introduced in the
current minor release; R348 protects people who did not ask. `strict=True` follows R348 exactly as
`--strict` does: a lint added in a minor warns until the next edition. There are no groups or
wildcards (D2), so a toolchain upgrade never turns a passing package into a failing one.

### 4.4 Which commands apply it

`yan check`, `yan test`, `yan build`, `yan run` and the agent tools (`check_patch`, `spec_check`)
all apply the same set to the package's own code, and `yan test` fails before running tests when a
denied lint fires (D6). The set is part of the check cache key, so changing it re-checks without a
clean build. Command-line `--strict` can only add to it.

### 4.5 Workspaces and dependencies

- In a workspace, members inherit the root's `lints` and may add to it, never remove (D4). This is
  Cargo's workspace lints with inheritance on by default, which suits a single-owner monorepo.
- A dependency's set never applies in the consumer's build. Diagnostics in dependencies stay capped
  at warnings, as for a dependency's `@deny` today (18.2).
- A dependency's `forbid` is still a fact about its code, so the consumer's own `@forbid(panic)` can
  rely on it when proving reachability; this is the existing transitive rule, unchanged.
- Sets are not shared across packages. A named set published by another package is deferred (D8).

### 4.6 Agents and review

The lint set is a gate the human owns, like the capability ceiling. A patch that removes a code from
`deny` or `forbid`, or sets `strict` to false, is a protected target (18.3) needing a human
confirmation token; a patch that only adds to the set is not (D7). `yan review` lists lint-set
changes beside capability changes. Agent tools report the package's set in their context so a
model sees denied lints as errors from its first patch, not at CI.

### 4.7 What stays out

- **User-written analyzers.** Running package code inside the checker breaks literal-only manifests,
  check determinism and the capability story. After 1.0, if at all.
- **User-defined profiles.** A profile (R320) also shapes the platform, handlers and stack bound.
  `lints.forbid` covers the "no panics in this library" case without opening profiles to users.
- **Per-directory or per-module sets.** Item attributes already handle the local case; a module-level
  `@deny` can be a separate small question (D10).

## 5. Cost

- Spec: one manifest field row (19.1), a paragraph in 18.2, one sentence on protected targets in
  18.3, one manifest-read error code (25). No new syntax, no Core change.
- Toolchain: parse one more literal field, seed each item's lint levels and forbid set from it, add
  it to the cache key. The forbid obligation reuses the per-item prover, so a package-wide
  `panic` forbid costs what writing it on every item costs today.
- Conformance: cases for deny across build modes, `@expect` under a package deny, a retired code in
  the list, workspace inheritance, and a dependency's set capped at warnings.

## 6. Open decisions

- **D1. Adopt the field.** Add `lints: Lints(strict, deny, forbid)` to `package.yag`, only stricter,
  applied by every command. Recommended: yes.
- **D2. No groups or wildcards.** Codes are listed one by one; only `strict` covers "all". Recommended:
  yes. Alternative: named std groups pinned per edition, as .NET `AnalysisLevel` does.
- **D3. `@expect` under a package deny.** Allowed, with its reason. Recommended: yes. Alternative:
  E277-style error, which makes package deny as strong as item `@deny`.
- **D4. Workspace inheritance.** Members inherit the root set by default and may only add.
  Recommended: yes. Alternative: Cargo's explicit opt-in per member.
- **D5. `alloc` in `forbid`.** Rejected in favour of the capability ceiling. Recommended: yes.
  Alternative: accept it as a synonym and lint the duplicate.
- **D6. `yan test` fails on denied lints before running.** Recommended: yes. Alternative: report them
  after the run so test results still appear.
- **D7. Weakening the set is a protected patch target.** Removals and `strict=False` need a human
  token; additions do not. Recommended: yes.
- **D8. Shared sets across packages.** Deferred. If wanted later, a set would be a literal constant in
  a dependency, read without running code, and pinned by `yan.lock`.
- **D9. Registry display.** Show a published package's lint set and forbid list on its registry page
  and in `yan add` output as a quality signal. Recommended: yes, informational only.
- **D10. Module-level attributes.** Whether `@deny` and `@forbid` may sit on a module as well as an
  item. Recommended: leave for a separate question; not needed for F9.
- **D11. User-written analyzers.** Out of 1.0. Recommended: record as a non-goal for 1.0 in section 26.

## 7. Decisions (Antonio, cards, 2026-11-04)

D1, D5, D6, D7, D8, D9 and D11 accepted as recommended. Four answers differ from the recommendation:

- D2: named std groups are allowed in the set, pinned per edition as .NET `AnalysisLevel` does, besides
  listing codes one by one. The RFC must say how group membership is pinned and changed per edition.
- D3: `@expect` under a package deny is an error (E277-style), so a package deny is as strong as an
  item `@deny` and cannot be locally excused.
- D4: workspace inheritance is explicit opt-in per member (Cargo style), not inherited by default.
- D10: `@deny` and `@forbid` may sit on a module as well as an item, decided in this work.
