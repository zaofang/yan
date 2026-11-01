## 16. Names, projection and round-trip

### 16.1 Names are word lists

Yong stores every identifier as a sequence of lowercase ASCII words plus a class, `value` or `type`:

| Class | Jian | Wen | Yong text |
| --- | --- | --- | --- |
| value | words joined by `_` | words joined by `_` | words joined by `_` |
| type | PascalCase | PascalCase | PascalCase |

PascalCase is split into words at each capital letter, which is why consecutive capitals are forbidden (E112). Digits attach to the preceding word. A `@capability` trait binds in `uses` under its value-class name (`HttpClient` is `http_client`).

### 16.2 Foreign names

Names that come from outside (C symbols, SQL columns, JSON keys, Python attributes) are stored verbatim with the flag `foreign` and printed unchanged by both surfaces. They are introduced only by extern blocks, `@export(name=...)`, schema declarations and the Python bridge. Within Wen, a foreign name that is not a valid Wen name is written in backticks.

### 16.3 The projection table

Some names and forms differ by surface on purpose, so each reads natively. The table is part of the edition (section 20.4), and the round-trip laws hold through it.

| Yong | Jian | Wen |
| --- | --- | --- |
| `List`, `Map`, `Set` (insertion-ordered) | `list`, `dict`, `set` | `List`, `Map`, `Set` |
| `Deque`, `Heap` (min-heap) | `deque`, `heap` | `Deque`, `Heap` |
| `SortedMap`, `SortedSet` | `sorted_dict`, `sorted_set` | `SortedMap`, `SortedSet` |
| `Soa` | `soa` | `Soa` |
| `BigInt` | `bigint` | `BigInt` |
| `Str`, `Bytes` | `str`, `bytes` | `str`, `Bytes` |
| `dec128` | `decimal` | `dec128` |
| `i8` ... `i128`, `u8` ... `u128` | `int8` ... `int128`, `uint8` ... `uint128`; `int` | `i8` ... `i128`, `u8` ... `u128`; `int` |
| `f16`, `bf16`, `f32`, `f64`, `f128` | `float16`, `bfloat16`, `float32`, `float128`; `float` | `f16`, `bf16`, `f32`, `f64`, `f128`; `float` |
| `Option[T]` | `T \| None` | `T?` |
| inout parameter | `var p: T` | `var p: T` |
| inout argument | `&x` | `&x` |
| sink parameter | `take p: T` | `take p: T` |
| failure in a signature | `-> T fail E` | `-> T fail E` |
| reify a failure | the call used as a value | `try f(x)` |
| deep copy, alias | `x.copy()`, `x.share()` | `copy x`, `share x` |
| interpolation | `f"{x:.2f}"` | `"\(x:.2f)"` |
| closure | `lambda x: e` | `x -> e`, or the implicit subject `.f` |
| record | `class C:` with fields | `type C = { ... }` |
| sum | `type S = A(...) \| B()` (R423) | `type S = A(...) \| B()` (R371) |
| loop forever, while | `while True:`, `while c:` | `loop`, `while c` |
| cancel scope | `with deadline(t):` | `with deadline(t)` |
| `par for` | `par [e for x in xs]` for values, `par for x in xs:` for effects (R427) | `par for x in xs` |
| `comptime` | compile-time parameters only | `#for`, `#if`, `#expr` |
| generator | none | `gen f(...) -> T` |
| type alias | `type X = T` | `type X = T` (R371) |
| `Newtype` | `type N = newtype[T]` | `type N = newtype[T]` (R371) |
| a body that is one match on the parameters | a match body (R426) | clause form (R266, R370) |
| `become Call` | `become f(x)` | `become f(x)` |

### 16.4 Projection operations

| Command | Input | Output |
| --- | --- | --- |
| `yong view --as wen F` | a Jian module | idiomatic Wen: `let` added, implicit-subject lambdas where a lambda is a single field-access chain, contracts and examples kept |
| `yong view --as jian W` | a Wen module | canonical Jian, for every item within the Jian subset |
| `yong view --as yong X` | either surface | Yong text |

Items outside the Jian subset are reported, not dropped. The report names the first construct that excludes each item (for example "uses `Ref` (L4)"). `view` takes `--edition`. Neither surface allows shadowing (E230), so projection renames nothing.

### 16.5 Round-trip laws

These MUST hold within each edition and MUST be tested on every conformance program (section 24):

1. For any canonical Jian module F: `jian(elaborate(F)) == F`, byte for byte.
2. For any canonical Jian module F: `jian(wen(F)) == F`, byte for byte.
3. For any Wen module W within the Jian subset: `elaborate(jian(W)) == elaborate(W)` as Yong, compared by item hash.
4. For any module X in either surface: the item hashes of `elaborate(X)` do not depend on formatting, comments or item order.
5. For any Wen module W: `wen(elaborate(W)) == fmt(W)`, byte for byte, comments kept at their items. A function whose whole body is one `match` on its parameters prints in clause form (15.3, R266), so this law keeps one printing per tree.

**Trivia through Yong.** Yong carries three hash-neutral kinds of trivia, so laws 1, 2 and 5 hold byte for byte: comments and docstrings; blank lines; and literal spelling (18.5). Comments attach as leading (on their own line, to the next node), trailing (on the same line, to the previous node) or dangling (in an empty construct or before a closer).

**Item identity.** An item's hash is defined in section 3.6. Two items with the same hash are the same item, whichever surface they came from.
