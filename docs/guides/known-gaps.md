# Known gaps

What Yan Agent cannot do yet, as of main `6ed0e44`, 2026-11-16. The issues are the source of truth: this page lists
what a newcomer meets first and links the rest. Each repro below is a file in
[`examples/real/gaps/`](../../examples/real/gaps/); `docs/guides/check.py` fails when one starts to pass, which is
the signal to delete its entry here. A fixed gap closes its issue; look there before trusting this page past
a week (DL14).

## Hit by ordinary programs

Found by the real-programs sweep ([examples/real/README.md](../../examples/real/README.md)). Each fails on every
backend, at `yan check`.

### [#1403](https://github.com/zaofang/yan/issues/1403): store into a dict held in a record field

`c.vals[k] = v` is E017. Workaround: pass the dict as a separate `var` parameter.

```yag gap=examples/real/gaps/field_index_place.yag
# Gap #1403: store into a dict held in a record field.
class Cache:
    vals: dict[str, int]

def put(var c: Cache, k: str, v: int):
    c.vals[k.copy()] = v

def main() uses io:
    c = Cache(vals={})
    put(&c, "a", 1)
    io.print(f"{c.vals.len()}")
```

Fixed since the sweep: #1398 (I001 on a constructor call in a match branch), #1399 (native `list.pop`,
`x in [lit, ...]`, `StrJoin`), #1400 (method call on a module-level const), #1405 (`Sender.close`), #1402 (`list.clear`, `list.remove`, `list.pop(i)`, `dict.pop`), #1404 (a module function no longer captures a method of the receiver's type; 17.2), #1401 (`.2f`, `04` and the other common f-string specs).

## Backend and checker coverage

| Issue | Gap |
|---|---|
| [#758](https://github.com/zaofang/yan/issues/758) | features that run only on the interpreter; backends refuse with an E-code (`tools.difftest --strict-refusals` lists them) |
| [#1242](https://github.com/zaofang/yan/issues/1242) | the rest of the 6.9 UB checker (`yan run --ub`) |
| [#1319](https://github.com/zaofang/yan/issues/1319) | `--ub` races: sync edges through channels, mutexes and atomics once the machine runs tasks |
| [#1215](https://github.com/zaofang/yan/issues/1215) | `extern` calls beyond integer and `f64` signatures |
| [#1218](https://github.com/zaofang/yan/issues/1218) | comptime closure specialization: inline body, `inout`/`sink` captures, E263 |
| [#1299](https://github.com/zaofang/yan/issues/1299) | `try_send` `Full[T]`: union failure members, duplicate std `Full` |
| [#1382](https://github.com/zaofang/yan/issues/1382) | `@wire` variant names collide across sums sharing a variant name |

## Python fidelity, packages and tooling

| Issue | Gap |
|---|---|
| [#709](https://github.com/zaofang/yan/issues/709) | `yan import` of Python methods and the remaining N-codes |
| [#767](https://github.com/zaofang/yan/issues/767) | package manager: directory indexes only |
| [#908](https://github.com/zaofang/yan/issues/908) | `yan mcp package` leftovers (RFC 0062) |
| [#899](https://github.com/zaofang/yan/issues/899) | `yan new` leftovers and proposed goldens |
| [#877](https://github.com/zaofang/yan/issues/877) | release pipeline holes |
| [#992](https://github.com/zaofang/yan/issues/992) | `std.sim`: PCT and `check_trace` |

## Spec debt and research

| Issue | Gap |
|---|---|
| [#1273](https://github.com/zaofang/yan/issues/1273) | implementation and cases for the one-batch rows R1002 to R1009 and E333 |
| [#825](https://github.com/zaofang/yan/issues/825) | diagnostic codes in the spec or catalogue with no crate symbol |
| [#1119](https://github.com/zaofang/yan/issues/1119) | tracker for forum-ranked gaps, after M1: Yan Meta (#1121), live programming (#1122), size arithmetic in types (#1123), rank polymorphism (#1124), one-shot effect handlers (#1125), proofs (#1126) |

Picking one up: the [contributor guide](contributing.md) gives the order of work. Found a new hole? File it as
a numbered item in the nearest gap issue, with a repro under `examples/real/gaps/`.
