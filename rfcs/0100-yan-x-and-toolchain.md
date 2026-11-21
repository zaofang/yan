# 0100: `yan x` tool entry points and `yan toolchain` at M1

- Status: accepted
- Research: #363 (T29); M1 audit row 4 (`docs/reports/2026-11-21-m1-audit.md`)
- Rows: R1140 (D1), D2 pending

## Summary

Spec 18 lists `yan x <pkg> <tool>` and `yan toolchain`, and 19.1 lists `tools` as "packages `yan x` runs", but no text says what `<tool>` names or what `yan toolchain` does before a release index exists. This RFC fixes both so the two commands can be built at M1.

## Decisions

### D1: `bins` names a package's tool entry points

A package declares `bins`, a literal map from tool name to a module path under `src/` whose `main` is the entry point: `bins={"fmt": "cli.fmt"}`. `yan x PKG TOOL` requires PKG to be in the root's `tools` and locked with kind `tool`, requires TOOL to be a key of PKG's `bins`, and runs that `main` under PKG's own capability ceiling (19.2: a tool package is checked as its own root), never the root's. Arguments follow `--`. A missing tool package, an unlocked tool package or an unknown tool is a diagnostic that lists the valid names. Options weighed: a module of the tool package with `main` and no new field (implicit, no discovery without reading every module); dropping TOOL (changes the 18 table row). Recommendation: `bins`.

- Decision: Antonio chose "New bins manifest field" on the decision card, 2026-11-21. Antonio accepted the RFC, including the field shape, on decision card cmsg_014jqsmRQfVdtFkGwF8cBScE2WxPutRUXrfkHqT9NX7M8z, 2026-11-21.
- Row: R1140

### D2: `yan toolchain` at M1 verifies offline

`yan toolchain` prints the installed version and the maximum `yan` requirement over the package and its locked dependencies (20.3), and exits 1 with a diagnostic when the installed toolchain is older. It fetches nothing. Fetching and signature checks (19.6) and `yan self update` move to the milestone that ships a release index; the 18 table row splits accordingly. Options weighed: a static toolchain index at M1 (needs an index and signing layout first); defer the command to M2. Recommendation: offline verify.

- Decision: Antonio chose "Offline verify now, fetch later" on the decision card, 2026-11-21.
- Row: (R number once accepted)

## Spec changes

- 18: the `yan x` row names `bins`; the `yan toolchain` row loses fetch and `self update` to a later milestone.
- 19.1: field table gains `bins`; 19.2 states the tool ceiling.
- 20.3: the fetch sentence says `yan toolchain` reports an older installed toolchain.
