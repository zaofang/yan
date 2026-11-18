# Red-team demo: an untrusted plugin against Yan's authority checks

A host app (`host/`) loads a plugin written by an untrusted agent. The host's ceiling is
`["io", "fs"]`, its filesystem grant is `fs.sub("data")`, and it has no `allow_ffi`. Each directory
under `attacks/` is one hostile plugin behaviour; `attacks.toml` is the manifest (command, expected
exit and output) and the single source for this table and for the test
`crates/yan-cli/tests/redteam_plugin.rs`, which runs on every CI build.

```sh
yan run host/src/main.yag                    # the benign path: prints the content of data/ok.txt
uv run python examples/redteam-plugin/report.py   # reruns every attack, rewrites this table and audit.txt
```

A row marked **gap** is not stopped today. Its entry in `attacks.toml` pins today's behaviour, names
the open issue, and fails the test when the gap closes, so the row must then be updated.

## Results

<!-- results:begin -->
| # | Attack | Stage | Result | First line of output |
|---|---|---|---|---|
| 01 | symlink out of `fs.sub` | run | refused | `not granted` |
| 02 | `..`, absolute and `x/../..` paths through the grant | run | refused | `not granted` |
| 03 | `uses none` externs (`getpid`, `kill`) in the plugin's own `ffi/` | check | refused | `E023 ffi/sys.yn:1:1 this module holds foreign code and the root's 'allow_ffi' does not name its package` |
| 04 | the same externs hidden in a transitive dependency | resolve | refused | `E023 yan.lock:0:0 '@agent/helper' holds foreign code ('ffi/sys.yn') and the root's 'allow_ffi' does not name it` |
| 05 | plugin declares `net`, beyond the root's ceiling | resolve | **gap** #1590 | `wrote yan.lock` |
| 06 | closures that capture `fs` and `time` passed to `uses none` functions | check | refused | `E301 src/main.yag:11:20 main.main: 'apply(lambda: fs.read("x") else "x")' the closure captures capability 'fs', which 'apply' does not declare in 'uses'` |
| 07 | lock record that disagrees with its vendored archive | locked build | refused | `E020 yan.lock:0:0 'yan.lock' records a foreign set for '@r/lib' that its archive does not have` |
| 08 | extern `link "../evil"`: refused when the program runs | run | refused | `yan: not supported yet: cannot load library '../evil'` |
| 08b | extern `link "../evil"`: no diagnostic at check | check | **gap** #1391 | `` |
| 09 | private item or capability handle smuggled across packages | check | **gap** #1563 | no fixture |
| 10a | attack 01 on the Cranelift backend | run | refused | `yan: not supported yet: call of 'std.fs.sub' in native code` |
| 10b | attack 01 on the machine backend | run | refused | `yan: not supported yet: call of 'std.fs.sub' in native code` |

Generated at 6df03b8 on 2026-11-17.
<!-- results:end -->

## `yan audit`

`audit.txt` holds the output for the attacks that carry foreign code: the audit lists the library and
the package that brought it, so a reviewer sees the foreign code without reading the plugin. It does
not yet print per-package ceilings or a capability digest.

## Notes

- Attacks 01 and 02 are refused by the runtime, not at check: `fs.sub` resolves every path beneath
  the granted directory (`yan_rt::beneath`) and returns `not granted`. Any new `fs` operation must use
  that resolver. The native and machine backends refuse `std.fs.sub` loudly (10a, 10b).
- Attack 04 is decided from the registry `.info`; the dependency's source is not elaborated yet (#1452).
- Attack 07 reuses the fixture of conformance case `pkg/e020_locked_libs_edited`.
- Replay hook (INNOV-3): once `yan replay` exists, run attacks 01 and 02 twice and compare traces.
