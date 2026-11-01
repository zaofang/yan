## 14. Xin: semantics, canonical form and lenient reader

### 14.1 Semantics (normative mapping to Yong)

**Headers (R210).** An item's header is its `def` line plus its docstring and contract lines. The header states everything a caller relies on: parameter types, the return type, the `fail` type, `uses` and examples. Nothing in a header is inferred, and nothing in a body changes what a caller sees. `yong sig`, `pack_context` and the language server show headers only, as valid Xin text, so a model reads its dependencies in the language it writes. An `ex` line may bind handlers for its call: `ex fetch("A") == Quote("A", 10.0) with net = fake_net({"/A": "10.0"})`.

**Bindings (R214, R421).** The first `x = e` in a function binds `x`. The binding is mutable when the function later assigns it (`x = e`, `x += e`), assigns through it (`x.f = e`, `x[i] = e`), passes it as `&x`, or uses it as the receiver of a call whose callee's first parameter is `var` (`xs.append(v)`); the elaborator emits Yong `var` for it, and `let` otherwise. A later assignment must keep the binding's type (E201). `var x = e` on a local is accepted and normalized (N14). Unused bindings are lint W812, and a bare `_` binds nothing (R347).

**Tuples (R216).** Python's expression lists are tuples wherever one is meant. On the right of `=` and after `return`, several comma-separated expressions build a tuple: `lo, hi = 0, len(xs)`, `return a, b`. On the left of `=`, several names are a tuple target, and each name binds or assigns by the rule above; several places are a tuple of places, so `xs[i], xs[j] = xs[j], xs[i]` swaps. The whole right side is evaluated before any assignment. The printer keeps the form written.

**Functions.** Every parameter and the return type MUST be annotated (E240); `-> None` may be omitted, and a function returning `None` may fall off its end. Where inference would need an annotation inside a body, the error is E241. `var p: T` is `inout`; `take p: T` is `sink`. A call passes a `var` parameter as `&place` (E404 if missing, E405 if not allowed). Parameters before `/` are positional-only. `**fields: Expr[_]` is a named argument pack (10.1). `-> View[T] from p` returns a view (4.5). Compile-time value parameters go in the generic brackets: `def fixed[n: int](...)`.

**Calls (R282, R420).** `x.f(a)` and `f(x, a)` elaborate to the same call. The Xin tree records which form was written and the printer keeps it. The free form resolves `f` in scope (the current module, the prelude, or a qualified name); the dot form resolves it by the order of 17.2. If `f`'s first parameter is `var`, the receiver form is allowed only when `x` is a mutable place, and no `&` is written (SQ29); that call makes a local binding mutable (R421). If two parameters of the callee share a type, every argument for them must be passed by keyword (E210), except when the argument is a literal or a name equal to the parameter's name, and except for positional-only parameters and the receiver. A keyword argument for a `var` parameter is written `dst=&b`. The same rule applies to record constructors. The prelude's `min`, `max`, `atan2` and `hypot` are positional-only.

**Records.** `class C:` with annotated fields declares a record. Records have no methods: functions taking the record first serve that role through the dot form. Fields may have defaults. Every record derives `Eq`, `Hash` and `Show`, as Python dataclasses do, each only when every field implements it, so a record with a float field has no Hash (R344). Records are constructed positionally or with keywords under the rule for calls (`C(1, "a")`, `C(a=1, b=2)`). A record value is immutable unless its binding is mutable (see Bindings). `evolve(c, a=3)` returns a copy with fields changed.

**Sums, aliases and newtypes (R347, R214, R423).** Sums are syntactic. `type S = ...` declares a sum exactly when every alternative is an inline variant `Name(f: T, ...)`, or `Name()` when it has no fields: `type Tree = Leaf(value: int) | Node(left: Tree, right: Tree)`. Each variant declares a class that belongs to that sum, and constructing a variant yields a value of the sum type. Patterns spell a field-less variant `case Name():`. Any other `type X = T` is an alias, as in Python 3.12: `X` and `T` are the same type. So `type IoErr = FsError | Timeout` is an alias, usable only where a union is allowed (after `fail` or in `Result`), whether its classes are local or imported. `None` is never a sum alternative, and mixing inline variants with other alternatives is E232. The lenient reader turns a class used only as an alternative of one sum into an inline variant of it (N24), and `case Name:` on a variant into `case Name():` (N25). A newtype is `type N = newtype[T]`: a distinct type with no implicit conversion, built with `N(v)` and unwrapped with `.value`. A `where` clause adds a refinement predicate and is legal only on a newtype.

**Match.** `match` must be exhaustive (E503). On a sum type, `case _` is forbidden (E502), so adding a variant breaks every match that should handle it, except for `@non_exhaustive` sums from another package and open C enums, which require it. On scalars and strings, `case _` is required unless the cases are provably exhaustive.

**None and narrowing (R285, R421).** `T | None` is `Option[T]`. `x is None` and `x is not None` narrow a local's type in the branch they guard, as in mypy. For a mutable local the narrowing lasts until the next assignment to it, the next call that passes it as `&x`, or the next call that uses it as a `var` receiver. `x else y` unwraps or falls back. `x?` propagates `None` when the function returns an optional type.

**Results (R214, R424, R425).** A fallible function is declared `-> T fail E`, Yong's own form, and `-> None fail E` is written `-> fail E`. `Result[T, E]` is the type of a reified result and is never a return type; `-> Result[T, E]` is accepted by the lenient reader and normalized (N23). `return v` returns success; `fail e` returns failure; `Ok(v)` and `Err(e)` construct and match explicitly, and in a `T fail E` function `return Ok(v)` and `return Err(e)` are normalized to `return v` and `fail e` (N26). `x?` propagates, injecting into a union or converting through `From` (E310, E313). `x else y` handles both `None` and `Err`; `x else err -> h` binds the failure; `x else return` and `x else fail e` exit. Reification is explicit (R424): a call to a fallible function not followed by `?` or `else` is a `Result` value only where a `Result` is expected, namely an annotated binding, a `Result`-typed parameter, field or return of a reified value, a `match` scrutinee, an operand of `==` or `!=`, and `ex` and `assert` lines. Anywhere else it is E315, with the machine-applicable fix `?` when the enclosing function can propagate the failure, and `else ...` with a hole otherwise. A lambda passed to a non-escaping parameter may use `?`.

**Copies, sharing and views.** `x.copy()` is a deep copy (E413 when missing); `x.share()` aliases an `Rc`, `Arc`, `Sender` or actor handle, and `x.copy()` on one is E416. `View[T]`, `MutView[T]` and `StrView` are second-class borrows (section 4.5); `v.copy()` gives the owned value.

**Indexing (R287).** `xs[i]` on a list and `d[k]` on a dict are checked by the bounds solver under the rule for undecided checks below: proven accesses compile with no check, a counterexample is E501, and an unknown access compiles with a bounds check that panics and lint W820. `.get` returns `T | None`. Assigning `d[k] = v` always compiles; it inserts or replaces. A literal negative index `xs[-k]` means `xs[len(xs) - k]` (N16). Slices take an optional step, `xs[a:b:c]`, clamp as in Python, and never panic; a constant slice proven out of range is W807, and `xs.slice_exact(a, b) -> list[T] | None` refuses to clamp.

**Undecided checks (R211).** Every static check of a value-level fact (bounds, keys in a dict, refinements, `pre` at a call site, exclusivity disjointness; R372) has three outcomes. Proven: no runtime check. Counterexample found: an error that shows the failing input (for example `i = 3, len(xs) = 3`). Unknown: the program compiles with a runtime check that panics, and lint W820 marks the spot; W820 is denied under `--strict` and `--level release`, so a release build contains no unproven check unless an `@expect` with a reason allows it. Type, capability and ownership errors are always definite and stay errors. Integer overflow that the solver does not rule out compiles to the check of 3.4 with no lint, in both surfaces, and a scalar match that is not provably exhaustive is E503 (R372).

**Strings.** Xin has no `s[i]` on `str` (E504). Use `s.byte_len()`, `s.char_count()` or `s.grapheme_count()`, and iterate with `s.chars()`, `s.bytes()` or `s.graphemes()`. The lenient reader rewrites `len(s)` to `s.char_count()` and a `str` iterated directly (`for c in s`, `sorted(s)`) to `s.chars()` (N21).

**Numbers.** `int` is a 64-bit checked integer; overflow panics, and the panic message suggests `bigint`. `/` on integers produces a correctly rounded `float`. `//` and `%` floor. `**` is power. `& | ^ ~ << >>` work on integer types through `Bits`. `bool` is not a number. Conversions are explicit: `float(x)`, `int(x)` (truncates toward zero, panics on NaN or out of range), `round(x)` (half-even), and the methods `wrap_as`, `sat_as`, `trunc_as`. `+` on `str` and on `list` concatenates (trait `Concat`).

**Effects.** Every function lists its capabilities in `uses`, except `alloc`. A user capability is named by the snake\_case form of its trait, or `uses Trait as name` (R461). A capability set is declared as in Wen, `type Svc = uses net, log, time`, and its PascalCase name may appear in any `uses` list, where it expands in place (R431). `print` is `io.print`. `with name = e:` binds a handler for the block.

**Contracts (R284).** `pre`, `post`, `ex` and `decreases` lines come first in a body, after the docstring, and are part of the header. In `post`, `out` is the result and `old(p)` is a `var` parameter's entry value. `ex` lines are optional at `yong check`. `yong publish` and `--level release` require at least one `ex`, or a `test_` function that calls it, for every `pub` function except `main` (E601). A recursive function MUST have `decreases` (E602), except where the lenient reader infers it (N05).

**Tests.** A top-level function named `test_...` that returns `None` is a test, whether `-> None` is written or not (R420); its `assert`s must hold. If it has parameters, it is a property test: inputs are generated from the parameter types by generators that read a ChaCha8 choice sequence, and failures are shrunk by editing that sequence with fixed passes (SQ27). Tests may bind fake handlers with `with`. Tests run with the default handlers of 7.3: an in-memory `fs`, seeded `rand`, virtual `time`, `fault = never` and no `net`, and sched = seeded(0) (R349); a test that needs a real capability binds it with `with`. Snapshot tests compare against stored output; `yong test --update` rewrites the snapshots as a patch for review. Test verdicts are cached as section 18.4 describes.

**Concurrency (R427).** In `par:`, each top-level statement is one branch, so a `for` inside `par:` is one sequential branch. A branch may not read a binding made by another branch in the same block (E705). `par for` has two forms: the comprehension `par [e for x in xs]` produces a list, and the statement `par for x in xs:` runs its body once per element for effects, and par(limit=n) for x in xs: bounds how many bodies run at once (R373); `par concurrent` applies to either, and to `par concurrent:`, and requires concurrent progress. `?` inside a `par` branch, comprehension or `par for` body propagates the lowest-index failure after cancelling later branches. `with deadline(t):` and `with timeout(d):` open cancel scopes; expiry surfaces as `Timeout` in the failure type. A function parameter typed par Callable\[\[A\], R\] may be called from several branches, under the capture rules of 4.6 (R436). An actor handler is a function whose first parameter is var self: S; actor.spawn(e, mailbox=..., overflow=...) and cluster.spawn(e, placement=...) start actors, and h.f(args) on an Actor\[S\] or Remote\[Actor\[S\]\] sends a message (R435). That call does not make h mutable under R421. Actors come from `std.actor`, channels from `std.chan`.

**Not in Xin.** Arenas and `Ref`, layout control and proof attributes (L4); defining reflective code, stage-0 loops and generators (L5); `unsafe`, `Ptr`, `RawCell`, extern blocks, `Vec` and `asm` (L6). `@export` and applying derives and attribute functions are allowed.

### 14.2 Compile-time literal checks

Calls that construct dates, times and zoned values from literals are checked at compile time: an impossible date is E540, and a zoned value in a gap or in an unresolved fold (no offset given) is E541. A `strftime` or `strptime` pattern given as a literal is checked at stage 0 against the value's type (E542). Adding a `Duration` in `day` or `week` units to a `ZonedDateTime` is W817, with the fix `Period(days=n)`. These rules apply to Wen literals too.

### 14.3 Canonical form

The printer MUST produce exactly one text for a given Xin tree:

- 4-space indentation, LF line endings, no trailing whitespace, one final newline.
- One blank line between top-level items; inside a body, author blank lines are kept, at most one in a row and none at the body's start or end (18.5, R376).
- Items in author order. Patches append new items at the end of the module.
- Double-quoted strings, choosing f-strings where the source used them.
- Minimal parentheses, plus those required by E101.
- Lines over 100 characters are wrapped inside brackets with one argument per line and a trailing comma. Nothing is aligned.
- Contracts in the order `pre`, `post`, `ex`, `decreases`.
- Locals print as `x = e`, never `var` or `let`; mutability is inferred (R214).
- Fallible returns print as `-> T fail E`, and `-> None fail E` as `-> fail E`.
- A sum prints on one line, variants with fields inline as `Name(f: T)`, while it fits in 100 characters; past that, one variant per line after `=`, each starting with `|`.
- An unfilled hole prints as `...` (R212).
- Copies print explicitly as `.copy()`; the autofix inserts them, the printer never drops them (R283 rejected).
- `-> None` is omitted (R425); `int` and `float` print for the 64-bit types (N27).
- Sums print with inline variants, a field-less one as `Name()`, and its pattern as `case Name():` (R423).
- A function whose body is one `match` on its parameters prints as a `match` body; Xin has no clause form (R426).
- Calls print in the form written, dot or free (R420).
- The fixes for W823 (redundant local annotation) and W824 (guard a fallback replaces) are safe fixes applied in the agent loop (R429).

### 14.4 Lenient reader

The lenient reader accepts these deviations and normalizes them, reporting a note (N-code) instead of an error. `yong check --strict` disables it. Every Xin form an edition retires stays accepted by the lenient reader forever and normalizes to the current form (section 20.4); its acceptance set only grows.

| Code | Accepted input | Normalized to |
| --- | --- | --- |
| N01 | `import m`, `from m import a` | removed; uses of `a` become `m.a` |
| N02 | any consistent indentation, including tabs | 4 spaces |
| N03 | single-quoted strings | double-quoted |
| N04 | `Optional[T]`, `List[T]`, `Dict[K, V]`, `Set[T]`, `Tuple[...]`, `Union[A, B]` (error position) | `T \| None`, `list`, `dict`, `set`, `tuple`, `A \| B` |
| N05 | missing `decreases` on structural recursion over a sum or list | inferred measure |
| N06 | `a + b * c`, `a and b or c` and other mixes covered by E101 | parentheses inserted by standard Python precedence |
| N07 | `HTTPError`-style names | `HttpError`, renamed throughout the module |
| N08 | `@dataclass` on a class | removed |
| N09 | methods with `self` inside a class body | lifted to top-level functions with first parameter `self: C` |
| N10 | `x == None`, `x != None` | `x is None`, `x is not None` |
| N11 | CRLF, trailing whitespace, missing final newline | canonical |
| N12 | `s[i]` on a `str` | `s.char_at(i)`, with a note that it costs O(n) |
| N13 | type S = A \| B whose alternatives are undeclared PascalCase names | inline field-less variants A() \| B() (R347, R423, R371) |
| N14 | `var x = e` or `let x = e` on a local | `x = e`; mutability is inferred (R214) |
| N15 | `if xs:`, `while s:`, `not n` on a list, str, number or optional | the explicit test by type: `len(xs) > 0`, `s != ""`, `n == 0`, `x is not None` |
| N16 | `xs[-k]` with a literal `k` | `xs[len(xs) - k]`, a checked index (R287) |
| N17 | `d.get(k, v)` | `d.get(k) else v` |
| N18 | `f(e for x in xs)` as the only argument | `f([e for x in xs])` |
| N19 | a quoted forward reference such as `"Node"` in a type | `Node` |
| N20 | `X = T` at top level where `T` is a type; `class E(Exception):` | `type X = T`; `class E:` with the base dropped |
| N21 | `len(s)` and `for c in s` (or `sorted(s)`) on a `str` | `s.char_count()` and `s.chars()` |
| N22 | `a < b < c` with `b` a name or literal | `a < b and b < c` |
| N23 | `-> Result[T, E]` as a return type | `-> T fail E` (R214) |
| N24 | a class used only as an alternative of one sum | an inline variant of that sum (R423) |
| N25 | `case Name:` on a variant | `case Name():` (R423) |
| N26 | `return Ok(v)`, `return Err(e)` in a `T fail E` function | `return v`, `fail e` (R425) |
| N27 | `int64`, `float64` | `int`, `float` (R425) |
| N28 | `not k in d`, `not x is None` | `k not in d`, `x is not None` (R425) |
| N29 | a function body of `case` arms (clause form) | `match` on the parameter, or on the tuple of parameters (R426) |
| N30 | a class whose `__init__(self, ...)` only assigns `self.f = e` or `self.f: T = e` | the record's fields: `f: T = e` for an annotated or literal default, `f: T` with the parameter's type for an assigned parameter; other methods are lifted as N09 (R217) |

The reader never inserts `_ =` before an unused result, because that would hide the bug E312 exists to catch; it emits a note with a machine-applicable fix instead. `raise`, `try` and `except` are not normalized: they are errors (E120) with a suggested rewrite, because their semantics differ from `Result`.

### 14.5 Complete example

```
"""Fetch quotes concurrently and value a portfolio."""

class Position:
    sym: str
    qty: int

class Quote:
    sym: str
    px: float

type PortErr = MissingQuote(sym: str) | Fetch(msg: str)

impl From[http.HttpError] for PortErr:
    def convert(e: http.HttpError) -> PortErr:
        return Fetch(msg=e.msg.copy())

def fetch_quote(sym: str) -> Quote fail PortErr uses net:
    body = http.get_text(f"https://px.example/{sym}")?
    px = json.parse_float(body) else fail Fetch(msg=f"bad price: {sym}")
    return Quote(sym=sym.copy(), px=px)

pub def value(ps: list[Position], qs: dict[str, Quote]) -> float fail PortErr:
    pre len(ps) > 0
    ex value([Position(sym="A", qty=2)], {"A": Quote(sym="A", px=10.0)}) == Ok(20.0)
    total = 0.0
    for p in ps:
        q = qs.get(p.sym) else fail MissingQuote(sym=p.sym.copy())
        total += float(p.qty) * q.px
    return total

pub def main() -> fail PortErr uses io, net:
    ps = [Position(sym="SPX", qty=10), Position(sym="NDX", qty=5)]
    quotes = par [fetch_quote(p.sym)? for p in ps]
    qs = {q.sym.copy(): q for q in quotes}
    total = value(ps, qs)?
    io.print(f"portfolio value: {total}")

def test_missing_quote_fails():
    assert value([Position(sym="X", qty=1)], {}) == Err(MissingQuote(sym="X"))

def test_value_scales_with_qty(n: int):
    assert value([Position(sym="A", qty=n)], {"A": Quote(sym="A", px=1.0)}) == Ok(float(n))
```
