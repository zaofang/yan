# 0026: Security model: unforgeable handlers, unconfined capabilities and agent confinement

- Status: accepted
- Research: [Security model: capabilities, FFI and agents](https://claude.ai/artifact/Hotw9YyR8LEvWE4GbB9gLi)
- Rows: R672 to R677

## Summary

The study threat-modelled capabilities, FFI, `yong mcp` and `yong agent`. Code cannot reach the host today only because no host `fs`, `net` or `proc` handler exists and extern calls never run. The spec gap that would matter once they exist is that a handler is any value implementing the trait (7.3) and std names host handlers, so any function could bind one and `uses` would bound nothing. D1 makes host handlers unforgeable. D2 to D6 name the capabilities that confer full process authority, give every confined capability an attenuator, freeze the meaning of contract clauses, confine the agent server, and close `embed` through symlinks. The implementation findings F1 to F8 are fixed separately in #119.

## Decisions

### D1: host handlers are unforgeable

A handler that reaches the host is built only by the platform before `main` (7.3), from the grants of a `yong_world` (12.7), or by attenuating a handle of the same capability already in scope (`with fs = fs.only(...)`). Std's host handler constructors are not nameable by user code. Binding one anywhere else is E326. Handlers written in Yong (fakes, recorders, replayers, wrappers) stay free, because they reach the host only through capabilities they are given.

- Decision: accepted as written (Antonio, card, 2026-11-01)
- Row: R672

### D2: unconfined capabilities

`proc`, `py` and every foreign-library capability give the holder the authority of the process: a child process, Python code or C code can do anything the process can. They form the unconfined class. Granting one in a ceiling is reported as granting every capability, gaining one blocks like a publisher identity change (19.4), and `yong audit` lists them first. A `uses none` extern item still requires its library in the package ceiling and the root's `allow_ffi` consent.

- Decision: accepted (Antonio, card "All five into one RFC", 2026-11-01)
- Row: R673

### D3: attenuators for every confined capability

Std ships `fs.only(read=..., write=...)` and `fs.sub(dir)`, `proc.only(programs=...)` and `env.only(vars=...)` at 1.0, beside `net.only`. File paths resolve beneath the granted directories: no `..` above them, no symlink out of them, using `openat2` with `RESOLVE_BENEATH` where the OS has it. Package ceilings may carry the same patterns (this makes R556 normative).

- Decision: accepted (Antonio, card, 2026-11-01)
- Row: R674

### D4: frozen meaning, not frozen text

Names in frozen `pre`, `post`, `ex`, `invariant` and `decreases` clauses resolve against the module as the spec was accepted, plus the prelude. An implement patch that adds an item a frozen clause names is P006. P006 protects the R300 test-hash closure: test functions, `@test_support` items and items under `tests/`. In the spec gate W806 is an error, and it also flags lookups keyed by `ex` inputs.

- Decision: accepted (Antonio, card, 2026-11-01)
- Row: R675

### D5: agent server confinement

A module name in a `yong mcp` request is dotted identifiers resolved beneath the server root, never through a symlink out of it. Every tool that runs code (`run_tests`, `spec_check`, `query`, `debug`) has step, heap and wall-clock budgets per call and per session; a step includes a loop iteration (R487). `sig`, the `quote` and `notes` of diagnostics, and test output carry repository text in the untrusted channel. `run_tests` and stage 0 run only in a repository the user has trusted, recorded by `yong trust`.

- Decision: accepted (Antonio, card, 2026-11-01)
- Row: R676

### D6: embed and symlinks

`embed(path)` checks the symlink-resolved target against the resolved package root (E267 otherwise). Version archives contain no symbolic links.

- Decision: accepted (Antonio, card, 2026-11-01)
- Row: R677

## Spec changes

- D1: 7.3, 12.7, 25 (E326).
- D2: 7.1, 12.2, 19.2, 19.4.
- D3: 7.1, 17.6.
- D4: 18.3, 21.1.
- D5: 21.4.
- D6: 10.4, 19.3.
