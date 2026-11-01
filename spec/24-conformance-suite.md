## 24. Conformance suite

The conformance suite is the executable form of this spec. It lives in `conformance/`, MUST grow with every feature, and its runner is independent of either implementation.

### 24.1 Layout and test format

```
conformance/
  jian/pass/   jian/fail/   jian/run/
  wen/pass/    wen/fail/    wen/run/
  yong/pass/   yong/fail/   yong/run/      -- yong/run/ for M0 (R468)
  ub/          semver/      sim/
  patch/       roundtrip/   golden/
  solver/                                  -- 8.6 solver goals (R378)
  pkg/                                     -- package and toolchain diagnostics (R479)
```

- `pass/` files must check with no errors.
- `fail/` files carry header lines `# expect: E501 at 14:9` (Wen and Yong: `-- expect:`), one per expected diagnostic. Exactly those codes must be reported, at those positions, and no others.
- `run/` files carry `# stdout:` blocks and optionally `# exit: 101`. They must produce exactly that output on every executor.
- `ub/` files must be flagged under `--ub` with the named item of 6.7, or run clean.
- `semver/` holds one old/new package pair per row of the API evolution table (20.6) with the expected bump. pkg/ holds one package tree per case, with an optional earlier version, the toolchain command to run and the expected diagnostic, for the codes of sections 18 to 20 that need a whole package (R479).
- `patch/` cases contain a module, a patch and the expected result or P-code.

### 24.2 Required coverage

| Area | Minimum |
| --- | --- |
| Lexing and layout | every lexical rule of both surfaces; each lenient normalization N01 to N30 with an input and expected canonical output |
| Parsing | every grammar production of Yong, Jian and Wen in one `pass` file; every E1xx code in one `fail` file |
| Types | inference, records, sums, newtypes, generics, traits, coherence (E220), method resolution (E250), named argument packs |
| Ownership | each convention; value semantics (mutating a copy never changes the original); every E4xx code of section 25 (R378); the runtime exclusivity panic; views and `from` |
| Memory model | every UB item in 6.7 flagged by `--ub`; DRF-SC litmus tests under `explore`; every happens-before edge in 6.5 |
| Capabilities | E301, E306, E307, E314, E317 to E319; fake handlers in tests; default handlers in `main`; `@splittable`; `net.only` |
| Failure | `?`, `else`, `else err ->`, `From`, injection, E310 to E313; each panic kind; exit statuses 1 and 101 |
| Contracts | `pre`, `post`, `old`, `ex` as tests; property-test shrinking to a minimal case; every E6xx code of section 25; the enforcement-level matrix (`check`, `release`, `prove`) of 8.5 (R378, R379) |
| Numbers | overflow panics; `/`, `//`, `%` on negative operands; strict floating point; libm results bit-identical across tier-1 platforms; tier-R reductions |
| Concurrency | `par` and `par for` results identical across 1, 2 and 16 workers over 100 runs; lowest-index failure; cancellation runs drops; drain; every E7xx code of section 25 (R378) |
| Stage 0 | every E26x and E27x code of section 25 (R378); materialization; derives; hygiene; budgets |
| FFI | E901 to E911; the C profile with `_Static_assert` checks; callbacks; abort on foreign unwind |
| Time | literal checks (every E54x code, R378); tz edge cases against the pinned tzdb; virtual time |
| Data | Arrow layouts; deterministic queries across thread counts; `QueryError` |
| Round-trip | the laws of 16.5 (and N7 for each niche surface) on every conformance program, run by `conform.py --laws` (R564) |
| Python bridge | every M1 `run` program exported with `yong export --python` produces identical output under CPython 3.12 |
| Determinism | hash-key independence (two keys, identical outputs); record and replay; `std.sim` seed replay |
| Safety | `Wire` decoder fuzz targets with limits enforced; patch protection and invisible-Unicode rejection; actor reentrancy, deadlock, timeout and supervisor-intensity scenarios |
| Stability | every `semver/` pair; E001, E002, E008, E012, E013, W014 |

### 24.3 Golden programs

G1 is hello world (`io.print`), G3 is the portfolio program of 14.5 under `std.sim`, and G4 is a word count over `fake_fs`. G2 parses trades and computes volume-weighted average price:

```
class Trade:
    sym: str
    px: float
    qty: int

class BadLine:
    line: str

def parse_line(line: str) -> Trade fail BadLine:
    p = line.split(",")
    if len(p) != 3:
        fail BadLine(line=line.copy())
    px = p[1].parse_float() else fail BadLine(line=line.copy())
    qty = p[2].parse_int() else fail BadLine(line=line.copy())
    return Trade(sym=p[0].copy(), px=px, qty=qty)

pub def vwap(ts: list[Trade]) -> dict[str, float]:
    ex vwap([Trade(sym="A", px=10.0, qty=1), Trade(sym="A", px=20.0, qty=3)]) == {"A": 17.5}
    acc: dict[str, tuple[float, int]] = {}
    for t in ts:
        n, v = acc.get(t.sym) else (0.0, 0)
        acc[t.sym.copy()] = (n + t.px * float(t.qty), v + t.qty)
    return {s.copy(): nv[0] / float(nv[1]) for s, nv in acc.items()}
```

`p[0]` through `p[2]` compile because the length check proves the bounds.
