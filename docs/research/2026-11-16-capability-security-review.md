# Capability security review: escapes from `uses` and the ceiling

Date 2026-11-16, main at 5f7a7e3. Read-only review; probes run on `yan run --backend interp|cranelift|machine`.

Question: can a program obtain host authority it was not passed or did not declare? Scope: 7.1 to 7.4, 9.6.1 (R400), 12, 17.6 (R674), RFC 0026 (R672 to R677), the machine host-call table (#1363), and the fs, env, proc, clock and random paths of the interpreter, the machine and native code.

## Findings

| # | Severity | Finding | Issue |
|---|---|---|---|
| F1 | High | `fs.sub` and `fs.only` follow symlinks out of the grant | #1388 |
| F2 | High | FFI gate not enforced: no `allow_ffi` check, `uses none` libraries skip the ceiling, any `ffi/` directory is an ffi module | #1389 |
| F3 | Medium | a `uses none` function performs I/O through a capability-capturing closure argument | #1390 |
| F4 | Low | interp and native FFI load `lib{link}.so` through the loader search path with an unvalidated link name | #1391 |

### F1 (High): attenuators are lexical only

Location: `crates/yan-interp/src/native.rs`, the `SubFs` and `OnlyFs` arms of fs read. The path is checked lexically, then `std::fs::read_to_string` follows any symlink. R674 requires no symlink out of the grant.

Repro: `mkdir box && ln -s /etc/hostname box/link`, then

```python
pub def main() uses io, fs:
    with fs = fs.sub("box"):
        io.print(fs.read("link") else e -> e.reason)
```

prints the host name on the interpreter. `yan mcp package --allow fs` grants `fs.sub` of the package root (R986), so a symlink committed in a served package reaches any file the server can read. Native code and the machine have no `fs.sub` yet and refuse loudly.

Fix: one resolver in yan-rt shared by every backend: `openat2` with `RESOLVE_BENEATH` on Linux, else per-component `O_NOFOLLOW` walks from a directory fd of the grant root.

### F2 (High): foreign calls with no capability, ceiling or consent

Three gaps compose:

1. `allow_ffi` is only a manifest key name (`crates/yan-elab/src/desugar.rs:188`); nothing checks it.
2. E314 (`desugar.rs`, ceiling check) reads only the `uses` lists of functions. A `uses none` extern adds nothing, so its library never meets the ceiling, contrary to 12.2 and R673.
3. `is_ffi` (`crates/yan-db/src/lib.rs:297`) finds the root by `yan.toml`, but manifests are `package.yag`. In a real package no root is found and any directory named `ffi` or `ub` at any depth qualifies.

Repro: a package with `capabilities=["io"]` and no `allow_ffi`; `src/ffi/main.yn` declares `unsafe extern "C" lib c link "c"` with `safe getpid() -> c_long uses none` and `safe kill(pid: c_long, sig: c_long) -> c_long uses none`, and `main() uses io` prints `getpid()`. Interp and Cranelift print the pid; `yan check` is silent. Any libc symbol with integer arguments (`kill`, `close`, `exit`) is reachable. The machine refuses: its host table is closed.

Fix: root marker `package.yag` and only `<root>/ffi/` and `<root>/ub/`; every extern library is in the ceiling whatever its `uses`; `allow_ffi` enforced at the root for every library reached, including through dependencies.

### F3 (Medium): `uses none` is not closed under Callable

```python
def apply(f: Callable[[], str]) -> str uses none:
    return f()

pub def main() uses io, fs:
    io.print(apply(lambda: fs.read("box/ok.txt") else "x"))
```

runs the read on the interpreter. The caller held `fs`, so this is not ambient authority, but `uses none` claims determinism and cacheability (7.2, 18.4) that caching, `yan audit` and the agent gate rely on. E306 already stops an escaping closure that captures a capability.

Fix: Callable types carry a `uses` set; a callee's `uses` must cover every Callable it calls (E301).

### F4 (Low): loader search path decides the code

`crates/yan-rt/src/ffi.rs` `open` builds `lib{link}.so`, `.so.6`, `.so.1`, `.dylib` and calls `dlopen`. `link` is not validated, so a `/` loads a cwd-relative path, and bare names follow `LD_LIBRARY_PATH` and `DYLD_*`. Fix: validate `link` as an identifier, resolve declared libraries once at startup and show the resolved path in `yan audit`.

## Checked, no finding

- **Machine host-call table** (`crates/yan-machine/src/ffi.rs`, #1363): a closed match on `(link, sym)` over pure `libm` functions; no `dlopen` or `dlsym`; unknown libraries and symbols are refused.
- **Host handler forging (R672)**: `with fs = fs.only(...)` and `fs.sub` attenuate only the held handle; E326 stops attenuating another capability. Closures that capture a capability and escape are E306 (probe confirmed).
- **Test defaults**: the interpreter's fs read returns "no host fs handler" under test; `proc` confinement (R998) runs `yan_sandbox::confine` before exec unless `.unconfined()`, which fails with `Unconfined` where Landlock is absent.
- **env, clock, random**: host reads happen only in the platform handlers bound before `main` (`env.rs`, `time.rs`, `rand.rs`, `yan-rt/src/envhandler.rs`); `env.only` filters before the host lookup.
- **`cluster` (R400)**: only the `sim` handler exists; no network path.
- **API keys**: `bench/r567/model.py` reads `ANTHROPIC_API_KEY` and `OPENROUTER_API_KEY` from the environment and stores neither; no key in tracked files.

## Not covered

- Panic and unwind across `with` scopes: not probed. Attenuation only narrows, so a scope left bound after a panic would be narrower, not wider; worth a case once supervisors restart inside `with`.
- `run --ub` raw pointers beyond the host table: the machine executes over abstract bytes, so a pointer cannot reach host memory; not probed further.
- Native fs, env and proc handlers beyond read: native has no attenuators yet (refuses loudly).
