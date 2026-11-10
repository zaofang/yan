# Versioning policy

This page restates spec section 20; the spec wins on any difference.

## Toolchain versions

The toolchain is `1.minor.patch`. A minor ships every 12 weeks. Patches go to the latest minor;
security fixes also go to the previous minor for 12 weeks. There is no LTS (20.7).

A package that builds and passes its tests on 1.a builds, passes and produces bit-identical output for
deterministic code on every later 1.b, given the same edition, `yan` API version and `yan.lock` pins
(20.1).

## What a minor may and may not change

- Frozen at 1.0, additive after: core semantics, grammars per edition, the std and prelude API
  except `@preview` items, E and W codes, the JSON diagnostic schema, the patch protocol, MCP tool
  schemas, `sig` structure, `package.yag` and `yan.lock` formats, `yan_abi_v1` (20.2).
- Not frozen: IR text and item hashes, performance, message wording, the set of warnings, cache and
  replay-trace formats, the security defaults of tls, http and crypto.
- A minor adds warnings, never errors, except for the permitted breakage: soundness holes,
  capability-safety holes, determinism bugs, divergence from a normative rule, and the security
  defaults above. Each ships with an ecosystem run and, where mechanical, a `yan fix` rule.

## Package requirements

`package.yag` requires `yan = "1.x"`. The build uses the maximum over the package and its dependencies
and fetches it with `yan toolchain` if the installed one is older. std items newer than the package's
version are invisible to it (20.3).

## Previews

`@preview("name")` items need `preview = [...]` in `package.yag`, sit outside the compatibility
promise, and stabilize, are withdrawn or are re-justified within four minors. A published package at
1.0 or later must not enable one (20.6).

## Editions

An edition changes surface only, never core semantics. It is named by the year of the stabilizing
toolchain, with at most one per three years; `edition` is `"next"` or a four-digit year (20.4). Every
edition stays supported and `yan fix --edition` migrates a package.

## Release notes

Each release's notes are generated, not typed:

    uv run python -m tools.changelog v1.1.0 v1.2.0

They list merged PRs by branch prefix and the spec rows added between the two tags.
