## 3. Yan: kinds, types and terms

Yan is a typed intermediate language in administrative normal form. Every intermediate value is named, every parameter and argument states its convention, every capability is an explicit parameter, and every operator is a named function. Yan has a text format (`.yir`) used for golden tests and debugging; the grammar in 3.5 is that format, and it MUST round-trip through the Yan parser and printer. Yan performs no inference: every `let` carries its type and every generic application lists its arguments.

### 3.1 Kinds

Kinds other than `Type` exist only at compile time and MUST be erased before code generation.

| Kind | Inhabitants |
| --- | --- |
| `Type` | value types; also a stage-0 value (section 10.4) |
| `Nat` | natural literals and size variables (positional array axes, fixed arrays, `Vec` widths, user `Nat` parameters such as `SMatrix[R, C, T]`) |
| `Dim` | rational exponent vectors over base dimensions (units, section 11.4) |

### 3.2 Scalars

| Yan Core | Yan Agent | Yan | Notes |
| --- | --- | --- | --- |
| `i8` ... `i128` | `int8`, `int16`, `int32`, `int128`; `int` is 64-bit (`int64` is lenient, N27) | `i8` ... `i128`; `int` = i64 | two's complement; overflow checked (3.4) |
| `u8` ... `u128` | `uint8` ... `uint128` | `u8` ... `u128` |  |
| `f16`, `bf16`, `f32`, `f64`, `f128` | `float16`, `bfloat16`, `float32`, `float128`; `float` is 64-bit (`float64` is lenient, N27) | `f16`, `bf16`, `f32`, `f64`, `f128`; `float` = f64 | IEEE 754; `f128` is software, correctly rounded `+ - * / sqrt fma` and conversions |
| `dec128` | `decimal` | `dec128` | IEEE 754-2008 decimal128 (11.7) |
| `bool`, `char` | `bool`, `char` | `bool`, `char` | `char` is a Unicode scalar value (4 bytes), never a grapheme |
| `Addr` | `Addr` | `Addr` | unsigned, the target's address width (u64 on 64-bit, u32 on CHERIoT); never a length or index; `size_of[Ptr[T]]` may exceed it |

Large integers are the library type `BigInt` (Yan Agent `bigint`).

### 3.3 Strings and bytes

- `Str` (Yan Agent `str`) is immutable UTF-8 with a 23-byte small-string optimization. It has no O(1) integer indexing and no single "length". Views are explicit: `bytes()`, `chars()` (Unicode scalars) and `graphemes()` (UAX #29). Counts are `byte_len()`, `char_count()` and `grapheme_count()`.
- Slicing takes `StrIndex` values returned by searches. Slicing by byte offset checks that both ends fall on char boundaries and panics with kind `bounds` otherwise. A borrowed part of a `Str` is a `StrView` (4.5).
- Normalization (NFC, NFD) is explicit. Case mapping is locale-independent; locale-aware operations live in `@yan/i18n` (ICU4X).
- `std.unicode` (R331) holds every locale-free algorithm, all `uses none`: `nfc`, `nfd`, `nfkc`, `nfkd` and `is_nfc` (UAX #15); default full case mapping `to_lower`, `to_upper`, `to_title` and full case folding `casefold`, `eq_casefold`; `graphemes`, `words`, `sentences` (UAX #29 default rules) and `line_breaks` (UAX #14); `width` in terminal columns (UAX #11 with UTS #51 emoji presentation); `skeleton` (UTS #39); and typed UCD properties. `==` and `Ord` on `Str` compare scalar values, which equals UTF-8 byte order; canonical equivalence is opt-in through `nfc` or `eq_canonical`. f-strings and `Show` never consult a locale (R336).
- The Unicode version is pinned per edition, so grapheme, case, normalization and UAX #31 results never change under a fixed edition. Tables are generated from the UCD and shared by every implementation; the 1.0 edition pins Unicode 18.0 (SQ25, amended by R330).
- `Bytes` is arbitrary octets. `OsStr` holds platform strings: bytes on Unix, WTF-8 on Windows. Paths are `OsStr`.
- There is no symbol type: interned strings are `Interner[T]` in std.collections, which hands out `Copy` keys (R445).

**In-place concatenation (R388).** When the left operand of `Concat` is a uniquely owned value at its last use (`s = s + t`, `s += t`, `xs = xs + [x]`, `xs += ys`), the result reuses its block with amortized doubling growth. The heap form of `Str` is `{ptr, len, cap}` within its 24 bytes. W822 flags `+` in a loop on a `Str` or `List` that is not at its last use.

### 3.4 Integers and floats

There is no implicit widening or narrowing. Integer literals take any integer type whose range contains them, and any binary float type (`f16`, `bf16`, `f32`, `f64`, `f128`) in which their value is exactly representable, so `x * 2` and `f(3)` work for a float `x` and a float parameter; a variable never converts (R656). A one-character `str` literal (without a prefix or with `r`) likewise takes type `char` where `char` is expected, so `c >= "0"` and `"a" in cs` work for a `char` `c` and a list of `char` `cs` (R802); the left operand of `in` is checked against the element type (E201). Yan Agent's lenient reader also takes a `char` where `str` is expected as its one-character `str` (N36, R830); Yan does not. `x as T` is checked and panics with kind `overflow` when the value does not fit; `wrap_as`, `sat_as` and `trunc_as` convert explicitly. Arithmetic families are `wrapping_*`, `checked_*` (returning `Option`), `saturating_*` and `overflowing_*`. Yan Agent's lenient reader takes an `int` variable where a `float` is expected, or mixed with a `float` operand, as `float(x)` (N38, R833).

- Overflow is checked in every build mode. Builds never change behavior. The range solver (section 8.6) removes provable checks; hot paths use `wrapping_*`, `wrap_as` and `sat_as`.
- Integer division by zero panics with kind `div_zero`.
- Bit operators (R682). `& | ^ ~ << >>` work on the fixed-width integer types and `int` through `Bits`, on the two's-complement pattern. `>>` is arithmetic on signed types and logical on unsigned ones. `x << n` is `x * 2**n` exactly or panics with kind `overflow` when a shifted-out bit differs from the result's sign bit; `bits.wrapping_shl` discards those bits. The shift amount may be any integer type, and a negative amount or one at least the width panics with kind `overflow`. On `bigint`, `& | ^ ~` act on the infinite two's-complement form, `<<` and `>>` never overflow, `>>` floors, and a negative amount panics with kind `overflow`.
- `%` is the floored remainder and `//` floor division. `int / int` is a correctly rounded `float` (SQ21). `round()` is half-even to an integer.
- Float division by zero follows IEEE: ±inf or NaN.
- Floats are strict IEEE 754 (section 7.7): no contraction, reassociation or flush-to-zero unless `@fast_math` or `@unordered` under `nondet`. There is no floating-point environment: no global rounding mode and no exception flags (section 11.1).

**Deferred traps (R380).** A panic with kind `overflow` or `div_zero`, or from a failed `as`, MAY be raised after the operation that caused it, but MUST be raised before the next observable event: a capability operation, an atomic or volatile access, an FFI call, the function's return, or a loop back-edge outside the current strip of at most 64 iterations. Until then the wrapped value may flow only into arithmetic and into checked indexing. The panic names the first overflowing operation in iteration order, so its message is identical in every build mode and backend. This lets a checked loop vectorize: the compiler ORs the overflow flags of a strip and tests once.

### 3.5 Yan Core text grammar

This is the complete grammar of `.yir` files. `{X}` is zero or more, `[X]` optional, `"t"` a terminal. Line comments start with `--` and carry conformance headers.

```
Module   ::= "module" Path {Item}
Item     ::= {Attr} (Fn | Gen | Record | Sum | Newtype | Trait | Impl | Const | Test | Extern | Inst)
Inst     ::= "inst" (Fn | Gen)                            -- only after optimization (R772)
Attr     ::= "@" Name ["(" AttrArg {"," AttrArg} ")"]
AttrArg  ::= [Name "="] ConstVal

Fn       ::= ["pub"] ["unsafe"] "fn" Path [TParams] "(" [Param {"," Param}] ")"
             "->" Type [From] ["fail" Type] {Contract} Block
Gen      ::= ["pub"] "gen" Path [TParams] "(" [Param {"," Param}] ")"
             "->" Type [From] {Contract} Block                -- yields Type
Param    ::= ["comptime"] Conv Name ":" Type | "cap" Name ":" CapType
Conv     ::= "let" | "inout" | "sink"                -- R446
From     ::= "from" Name {"," Name}
CapType  ::= Path ["[" TArg {"," TArg} "]"]
Contract ::= "requires" Pred | "ensures" Pred | "decreases" Expr {"," Expr} | "example" Expr
           | "cost" CostBound {"," CostBound}             -- R610
CostBound ::= ("steps" | "alloc") "<=" Expr

Record   ::= ["pub"] "record" Path [TParams] "{" [Field {"," Field}] "}"
Field    ::= ["pub"] Name ":" Type ["=" ConstVal]
Sum      ::= ["pub"] "sum" Path [TParams] "{" Variant {"," Variant} "}"
Variant  ::= Name ["{" Field {"," Field} "}"]
Newtype  ::= ["pub"] "newtype" Path [TParams] "=" Type ["where" Pred]
Trait    ::= ["pub"] "trait" Path [TParams] "{" {"type" Name} {Sig} ["derived" Block] "}"
Sig      ::= "fn" Name [TParams] "(" [Param {"," Param}] ")" "->" Type [From] ["fail" Type]
             {Contract} [Block]
Impl     ::= ["unsafe"] "impl" Path ["[" TArg {"," TArg} "]"] "for" Type
             "{" {"type" Name "=" Type} {Fn} "}"
Const    ::= ["pub"] "const" Path ":" Type "=" ConstVal
Test     ::= "test" Path Block
Extern   ::= "unsafe" "extern" String "lib" Name ["link" String] "{" {XItem} "}"
XItem    ::= ("safe" | "unsafe") "fn" Name "(" [XParam {"," XParam}] ["," "..."] ")" "->" Type
             [From] ["uses" "none"] ["nonblocking"]
           | "type" Name "drop" Path
           | ("safe" | "unsafe") "static" Name ":" Type
XParam   ::= Name ":" Type | Name ":" ("scoped" | "escaping") FnType

TParams  ::= "[" TParam {"," TParam} "]"
TParam   ::= ["comptime"] Name ":" Kind ["is" Path {"+" Path}]
Kind     ::= "Type" | "Nat" | "Dim"                    -- R440
TArg     ::= Type | Nat | Dim

Type     ::= Scalar | "Str" | "StrView" | "Bytes" | "OsStr" | "Unit" | "Addr" | "Never"
           | "List[" Type "]" | "Map[" Type "," Type "]" | "Set[" Type "]"
           | "Tuple(" Type {"," Type} ")"
           | "Option[" Type "]" | "Result[" Type "," Type "]"
           | "View[" Type "]" | "MutView[" Type "]"
           | "Array[" AxisArg {"," AxisArg} "," Type "]" | "Table[" Type "]"
           | "Quantity[" Type "," Dim "]"
           | Path ["[" TArg {"," TArg} "]"]            -- nominal: record, sum, newtype
           | "dyn" Path ["[" TArg {"," TArg} "]"]
           | FnType
           | "{" Name ":" Type "|" Pred "}"             -- refinement
           | Type "|" Type                              -- anonymous error union, only after "fail"
           | "Ref[" Type "]" | "Ptr[" Type "]" | "Uninit[" Type "]" | "Vec[" Nat "," Scalar "]"
FnType   ::= "Fn(" [Conv Type {"," Conv Type}] ")" "->" Type ["uses" CapSet] ["fail" Type]
Scalar   ::= "i8" | "i16" | "i32" | "i64" | "i128" | "u8" | "u16" | "u32" | "u64" | "u128"
           | "f16" | "bf16" | "f32" | "f64" | "f128" | "dec128" | "bool" | "char"
AxisArg  ::= Nat | Name | Type
CapSet   ::= "none" | Name {"," Name}

Block    ::= "{" {Stmt} "}"
Stmt     ::= ("let" | "var") Pat ":" Type "=" Rhs
           | Place "=" Rhs
           | "if" Atom Block ["else" Block]
           | "match" Atom "{" {Pat "=>" Block} "}"
           | "loop" Label {"invariant" Pred} ["decreases" Expr {"," Expr}] Block
           | "break" Label [Atom] | "continue" Label
           | "jp" Name "(" [Param {"," Param}] ")" Block       -- join point, declared before its jumps
           | "jump" Name "(" [Atom {"," Atom}] ")"
           | "ret" Atom | "fail" Atom | "yield" Atom             -- yield only in a Gen body
           | "become" Call                                      -- guaranteed tail call (R260)
           | "with" Name "=" ["dyn"] Atom Block
           | "deadline" Atom Block                              -- cancel scope ending at a MonoInstant (R788)
           | "par" ["concurrent"] "{" {Block} "}"
           | "comptime" "for" Name ":" Type "in" Atom Block
           | "comptime" "if" Atom Block ["else" Block]
           | "assert" Pred | "unreachable"
           | "unsafe" Block
           | "asm" "(" String {"," AsmOperand} ")"                -- only inside unsafe
           | "drop" Name | "reuse" Name "as" Ctor                 -- only after optimization
           | Rhs
Rhs      ::= Atom | Call | Call "?" ["via" Path] | "try" Call
           | Ctor | Proj | "copy" Place | "move" Place | Closure
Call     ::= Path ["[" TArg {"," TArg} "]"] "(" [Arg {"," Arg}] ")"
Arg      ::= Conv Atom | "cap" Name
Ctor     ::= Path ["[" TArg {"," TArg} "]"] "{" [Name ":" Atom {"," Name ":" Atom}] "}"
           | "(" Atom {"," Atom} ")"
Proj     ::= Place
Place    ::= Name {"." Name | "[" Atom "]"}
Closure  ::= "fn" "[" {Conv Name} "]" "(" [Param {"," Param}] ")" "->" Type
             ["uses" CapSet] ["fail" Type] Block
Pat      ::= "_" | Literal | Name | Path ["{" Name ":" Pat {"," Name ":" Pat} ["," ".."] "}"]
           | "(" Pat {"," Pat} ")" | Pat "|" Pat | Literal "..=" Literal
           | "[" [Pat {"," Pat}] ["," ".." [Name]] "]"
Label    ::= "'" Name
AsmOperand ::= ("in" | "out" | "inout" | "lateout") "(" (Name | String) ")" Atom
           | "clobber_abi(" String ")" | "options(" Name {"," Name} ")"
Atom     ::= Name | Literal
ConstVal ::= Literal | Path ["{" [Name ":" ConstVal {"," Name ":" ConstVal}] "}"]
           | "[" [ConstVal {"," ConstVal}] "]" | "(" ConstVal {"," ConstVal} ")"
Expr     ::= Atom | Call                                       -- pure; in contracts only
Pred     ::= the decidable fragment of section 8.6, written with intrinsic calls over Atoms
Comment  ::= "--" to end of line
```

Rules on the grammar:

- Records, sums and newtypes are nominal. Tuples are structural.
- `Ptr`, `Uninit`, `asm` and `RawCell` are legal only inside `unsafe`. `Ref`, `Ptr`, `Uninit` and `Vec` are Yan-only (L4 to L6) and MUST NOT appear in Yan Agent-elaborated code. `Ptr[T]` is never null; a nullable pointer is `Option[Ptr[T]]`.
- `Call?` propagates a failure; `via` names the `From` conversion when the error types differ and injection does not apply (8.1). `try Call` reifies the outcome as a `Result`.
- Arithmetic and comparison are calls to named intrinsics (`add_checked_i64`, `eq_f64`, ...); there are no infix operators in Yan.
- Value-producing `if` and `match` are written with join points: `jp k(x: T) { ... }` is declared before the branch, and each arm ends in `jump k(a)`. Join points map one to one to Cranelift block parameters. A join point body that ends without `ret`, `jump`, `fail`, `become` or `unreachable` continues after the statement of its block that jumped to it (R764).
- The runtime exclusivity check of section 4.3 is the intrinsic `exclusive_check(i, j)`, which panics with kind `exclusivity`.
- `drop Name` and `reuse Name as Ctor` appear only after the ownership passes (5.3); in checked Yan Core either is a validator error (R770). `drop x` runs the drop glue of `x`'s type, which `yan-mir` generates per layout. There is no `dup`: Yan has no implicit reference counting.
- **Instances (R772).** An `inst` item is a generic `fn` or `gen` specialized by monomorphization or comptime specialization (`yan-opt`, 22.3). Its brackets hold concrete type, `Nat`, `Dim` and comptime arguments, never parameters: `inst fn id[i64](let x: i64) -> i64 { ... }`. A call `id[i64](x)` names the instance when one exists. The instance key is the generic item's hash plus the hashes of its arguments, so every module that instantiates `std.list.len[i64]` shares one instance. Instantiation depth is bounded at 64 nested instances; past the bound, polymorphic recursion is E275, reported once per cycle. `inst` items appear only in optimized Yan Core and are never item identities (3.6).
- `comptime for` and `comptime if` are the only code-generation forms (section 10.4). `select` (Yan), `share` and `par for` have no Yan Core forms: they are calls to `std.chan.select`, `Share.share` and the bound `sched` handler.

**Guaranteed tail calls (R260).** `become f(args)` ends the current function by calling `f`, and it MUST NOT grow the stack on any backend, in any build mode, including the reference interpreter. It is legal only where `ret` is, and never inside `par`, `with` or a `Gen` body. Its call MUST be a direct call or a call through an `Fn`-typed parameter, never a `dyn` method or a closure (E273 for both). The callee's parameter conventions and types, return type, failure type and capability set MUST equal the caller's after monomorphisation (E271). The caller's locals are dropped in reverse declaration order before the transfer. An argument that borrows a caller local, as a `let` or `inout` argument or a view `from` that local, is E420, whose fix moves or copies it. Cranelift lowers `become` to `return_call` with the `tail` convention, LLVM to a `musttail` call, and WebAssembly to `return_call`. Yan Agent and Yan spell it `become f(args)`.

**Become modulo constructor (R456).** `become` is also legal as the last-evaluated argument of a constructor in return position, as in `return Node(l, x, become insert(r, k))`, and constructors may nest. Every backend and the reference interpreter MUST lower it to destination passing: the constructor's block is allocated first, the call receives the open field as a `set` destination, and the recursion runs in constant stack. The R260 rules apply to the call (E273, E271, E420). A `become` in any other position inside a constructor is E143. Yan Agent and Yan spell it the same way.

### 3.6 Item identity

Each item has a content hash: BLAKE3 over its canonical Yan Core text with references replaced by the hashes of the items they name. A strongly connected component of recursive items is hashed as one group with in-group references as indices; each member's hash is `H(group).index` (SQ5). Item hashes ignore editions (section 20.4) and are salted by toolchain version. Watch expressions and comments do not affect them; `ex` clauses and contracts do.

**Positional locals (R519).** In the hashed text, locals, closure parameters, labels and join points are named by binding order (`%0`, `%1`, ...), not by their written names. Parameters, fields, variants, capabilities, type parameters and items keep their names, because labelled arguments, `info(f)`, `Wire` and stage-0 reflection read them. Each surface keeps the written names in a per-item name table, which is hash-neutral trivia (16.5). Compiler-generated text that quotes source, such as the message of a failed `post` or `assert`, is fixed as a string literal before hashing. Renaming a local therefore never changes an item hash. Shadowing stays E230 in Yan Agent and Yan; Yan Meta admits it (16.6). Provenance, patch bases and spec freeze use the identity hash: the same hash without the toolchain salt, stable across toolchain versions (R348).
