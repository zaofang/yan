# Tally slice (proposed golden program G5)

A small vertical slice of the flagship from the product study: a card payments service that
authorizes requests, charges an interchange fee and settles per merchant. It exists to find where
the language and the toolchain hurt when the parts are used together.

| Path | What it is | State on main |
|---|---|---|
| `g5_tally.xin` | the slice in one Xin module | runs on the interpreter |
| `g5_tally.wen` | the same slice in Wen, written by hand | runs; same output, same Yong, same item hashes |
| `package/` | the slice as the flagship means it: schema, Wen fee module, Xin service, ceiling, simulated faults | does not load; each file stops at a gap below |

```sh
yong run examples/tally/g5_tally.xin
yong run examples/tally/g5_tally.wen
diff <(yong hash examples/tally/g5_tally.xin) <(yong hash examples/tally/g5_tally.wen)
diff <(yong view --as yong examples/tally/g5_tally.xin) <(yong view --as yong examples/tally/g5_tally.wen)
```

Expected output:

```
1 approved A86
2 approved A82
3 declined luhn
4 declined limit
5 approved A22
6 declined amount
7 declined issuer
8 approved A75
corner-shop gross=5249 fees=77 net=5172
air-lines gross=48000 fees=1200 net=46800
bookstore gross=2500 fees=72 net=2428
```

## What held

- The two surfaces elaborate to byte-identical Yong, and all 16 items (types, trait, impl,
  functions, the std instances) have the same salted and identity hashes. Wen's clause form and
  Xin's `match` give the same item.
- E210 caught a real ambiguity: `send(limit, pan, minor)` with two `int` parameters had to be
  called by keyword.
- `yong test` runs the `test_` functions and, since #100, every `ex` line: a wrong
  `ex fee(10000, Grocery()) == 151` fails as `g5_tally.fee#ex1`.

## Gaps

Ordered by how much of the flagship each one blocks. "Spec" means the fix needs a card.

### Blocks the slice's shape

| # | Gap | Effect on the slice |
|---|---|---|
| 1 | No cross-module names. Every command loads one file; `fees.fee(x)` from another module is "not supported yet: a projection of a literal". | Schema, Wen fee rules and Xin service cannot be separate modules, so there is no mixed-surface package. |
| 2 | `package.xin` is never read (`Package` is an unsupported type) and E314 is not implemented: `uses fs` in a package whose ceiling is `["io"]` checks clean. | Flagship step 3 (a prompt-injected file write fails at compile time) cannot be shown. `package/src/leak.xin` is that change. |
| 3 | `@capability` traits are unsupported in the elaborator. | The acquirer is a generic value parameter, `authorize[A: Acquirer](acq: A, ...)`, not `uses acquirer`; a test cannot bind a fake handler with `with`. |
| 4 | `fault.maybe`, `rand.*` and `log.*` are unsupported; `std.sim` does not exist. | No simulated acquirer outages, no logging; step 5 is out of reach. |
| 5 | No `decimal`, `Money`, `Secret`, `derive(Wire)` or `json`. | Amounts are `int` minor units, the card number is a plain `str`, and there is no wire schema or codec. |
| 6 | Native code (Cranelift) stops at the trait impl; with the trait removed, at `std.prelude.min` (reached through `zip`). | Step 6 (native everywhere) fails on the smallest service. |
| 7 | No Xin to Wen printer (`view --as wen`, M3). | The Wen twin was written by hand; the review view of agent code does not exist yet. |

### Bugs

| # | Bug | Status |
|---|---|---|
| 8 | Xin lexer: after a bracket spanning lines closes, the layout pass runs mid-line, so the next statement is E100 (`xs = [\n 1,\n]` then any statement). | Fixed here (`crates/yong-xin/src/lexer.rs`, test `statement_after_multiline_brackets`). Conformance unchanged: M1 309/358, M3 381/488. |
| 9 | `yong fmt` on a Wen file deletes every `---` doc comment. The Wen printer has no comments yet, but `fmt` writes its output anyway. | Open. `fmt` should refuse a file it cannot print losslessly. |
| 10 | Neither printer wraps lines over 100 characters (15.4, 18.5): `fmt` joins the 8-line batch literal into one 800-character line. | Open. |
| 11 | `yong test` names modules by file stem, so `g5_tally.xin` and `g5_tally.wen` collide and their tests are dropped silently: "2 of 2", where each file alone has 8 (2 `test_` functions and 6 `ex` lines). One file the toolchain cannot load anywhere under the root (here `package/`) aborts the whole run. | Open. |
| 12 | W812 says to rename an unused `pan` to `_pan`, and E110 rejects `_pan`. In a trait impl the parameter cannot be dropped either. | Open. |
| 13 | W813 (pub function without `ex`) fires in Wen but not in Xin for the same items. | Open. |
| 14 | Xin `x is P` is unsupported for any pattern but `None`; Wen's `is` works. Both files use a helper `approved(d)` instead. | Open. |
| 15 | Wen rejects a tuple type such as `Map[str, (int, int)]`, which the 15.2 grammar allows. | Open. |

### Spec

| # | Gap |
|---|---|
| 16 | A Wen pattern cannot name a variant from another module: `alt` takes a bare `TYPENAME` and `use` aliases only modules. `package/src/fees.wen` cannot match on `schema.Mcc`. Xin's pattern takes a `path`. |
| 17 | 11.7 gives no way to write a `Money` value in either surface. `package/` uses `100.00usd` (Wen) and `1000.00 * usd` (Xin) by analogy with units. |

### Friction

| # | Friction |
|---|---|
| 18 | No digit value for a `char` and no `str` slicing, so Luhn is `str(c).parse_int() else -1` over `chars()`. |
| 19 | A lambda whose body ends in `else` does not infer its result: `.map(c -> ... else -1).collect()` needs `let digits: List[int]`. |
| 20 | Wen has no comprehensions. Written the natural way (a comprehension in Xin, `.map` in Wen) the same logic gets different hashes, so the Xin file mirrors Wen's `.map` to keep the items identical. |
