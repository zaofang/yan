## 15. Wen: the human surface

Wen is the surface people write and read. It covers layers L0 to L6, files use the extension `.wen`, and it is built in milestone M3; until then its text is produced only by `yong view --as wen`. Wen infers the return type of a single-expression body, and `uses` and `fail` on non-`pub` items. Everything else is written. Every Wen tree has exactly one printing: for every Wen module W, `wen(elaborate(W)) == fmt(W)` byte for byte, comments kept at their items, so the formatter and the projection are one printer. For that reason Wen has no `where` blocks (write `let`), no shadowing and no alignment. A function whose whole body is one `match` on its parameters also has one printing: the clause form, whose body is the match arms alone (15.2, 15.3, R266, R370). The formatter never prints that function as `= match p`.

### 15.1 Lexical rules

- **Encoding and safety.** As Xin (13.1): UTF-8, ASCII identifiers, and the lexical safety rules (E105).
- **Names.** Value names are snake\_case and type names PascalCase, exactly as in Xin (E110, E112), so both surfaces spell every name identically. Backticks quote foreign names only (section 16.2).
- **Comments.** ` --  ` (with the space) runs to end of line; `--` anywhere else is E132. ` ---  ` starts a Markdown doc comment. A line starting ` -->  ` holds a watch expression. Examples are `ex` clauses, not doc-comment lines.
- **Layout.** Blocks are indentation only, 2 spaces; tabs are E133. Newlines inside brackets are joined. A line whose first token is `.` or a binary operator continues the previous line. There is no `;` outside grid literals. A line whose first token is `fail` or `uses` also continues a function signature that has not yet reached its `=`, which gives the signature layout of 15.3 (R460).
- **Literals.** Integers, floats, `True`, `False` and `None` as in Xin. Strings `"..."`, raw `r"..."`, bytes `b"..."`, C strings `c"..."` (E140 on interior NUL) and triple-quoted strings; non-raw strings interpolate `\(expr)` and `\(expr:spec)`, and braces are literal. A non-raw string that contains `${` followed by a name or expression and `}` is lint W854, whose fix rewrites it to `\(...)` (R663). A char is `c'a'`.
- **Units, dates and grids.** Unit-suffixed numbers such as `150ms`, `64MB`, `2.5bp` and `5pct`, where the suffix must be a declared unit. A date is `2026-10-30` (PlainDate), a time `09:00:00`, an instant `2026-10-30T09:00:00Z`, and a zoned time `2026-10-30T09:00[America/Chicago]`, with an offset when it falls in a fold; there is no implicit local zone. These are checked at compile time (14.2). Grid literals separate elements by spaces and rows by `;`: `[1 2; 3 4]`; newlines inside the brackets are joined as elsewhere (R463). An element is a literal, a name or a parenthesized expression, with an optional attached sign. Inside a grid, a `-` or `+` after a space and before a non-space is a sign (SIGN in 15.2), so `[1 -2]` is two elements, and `a -b` is E134; a `(` after a space starts a new element (SPLPAREN in 15.2), so `[a (b)]` is two elements and `[a(b)]` one call. A grid has two elements or a `;`, so `[e]` is a list (R374). Display widths count graphemes.
- **Keywords.** Reserved: `let var take pub type trait impl dyn use as try if then else match is for in while loop break continue return par with unsafe extern and or not copy share self assert unreachable become fail` (`fail` as in Xin, R462). Soft (keywords only where the grammar expects them, ordinary names elsewhere): `safe lib link asm gen sub yield select concurrent from uses pre post ex invariant decreases ghost lemma reveal`, and `deadline` and `timeout` after `with` (R444, R374). A soft word follows Xin's next-token rule (13.1, R462).

### 15.2 Grammar

This is the complete Wen grammar. Notation as in 13.2; NEWLINE, INDENT and DEDENT come from the layout rules.

```
module    = {modattr} {item}
modattr   = "@@" NAME ["(" [args] ")"] NEWLINE                 -- module attribute, as @@prove (R466)
item      = {attr} ["pub"] (fundef | gendef | subdef | lemmadef | typedef | traitdef | impldef | constdef | externdef)
          | usedecl
attr      = "@" NAME ["(" [args] ")"] NEWLINE
usedecl   = "use" ["."] NAME {"." NAME} ["as" NAME] NEWLINE          -- aliases only

fundef    = sig "=" body
gendef    = "gen" sig "=" body                                         -- yields the declared type
subdef    = "sub" sig "=" body                                         -- yields one place (R457)
lemmadef  = "lemma" sig "=" body                                       -- pre and post as contract lines (R458, R465)
sig       = NAME [tparams] "(" [params] ")" ("->" fntype                 -- clauses after a function type are its own (R464)
            | ["->" vtype ["from" NAME {"," NAME}]] ["fail" vtype] ["uses" capitem {"," capitem}])
body      = expr NEWLINE
          | NEWLINE INDENT {contract} stmt {stmt} DEDENT
          | NEWLINE INDENT {contract} arm {arm} DEDENT                    -- clause form (R266, R370)
contract  = ("pre" | "post" | "ex" | "decreases") expr NEWLINE
          | "cost" bound {"," bound} NEWLINE            (R610; cost, steps and alloc are soft keywords here)
bound     = ("steps" | "alloc") "<=" expr
capitem   = NAME [":" TYPENAME] | TYPENAME | "none"                  -- TYPENAME: a capability set (R431)
tparams   = "[" tparam {"," tparam} "]"
tparam    = TYPENAME [":" bound {"+" bound}] | NAME ":" type          -- type or compile-time value
bound     = tpath ["[" type {"," type} "]"] | "Nat"
params    = param {"," param} [","]                                   -- at most one "/" and one "**", in that order, checked after parsing (R460)
param     = ["var" | "take" | "ghost"] (NAME | "self") [":" type] ["=" expr] | "/" | "**" NAME ":" type   -- type omitted only for self; ghost (R458, R465)

typedef   = "type" TYPENAME [tparams] "=" (record | variant {"|" variant} | atype | "newtype" "[" type "]" | "uses" capitem {"," capitem}) ["where" expr] NEWLINE   -- "= {" always starts a record
record    = "{" rfield {"," rfield} [","] "}"                          -- R437
rfield    = field | "/"                                                -- at most one "/", checked after parsing
field     = {attr} NAME ":" type ["=" expr]
variant   = attr variant | TYPENAME "(" [field {"," field}] ")"       -- B() when field-less; a sum iff every alternative is a variant (R423, R371)
traitdef  = "trait" TYPENAME [tparams] NEWLINE INDENT {"type" TYPENAME NEWLINE}
            {{attr} sig ["=" body] NEWLINE} ["derived" "=" body] DEDENT
impldef   = ["unsafe"] "impl" tpath ["[" type {"," type} "]"] "for" type NEWLINE INDENT
            {"type" TYPENAME "=" type NEWLINE} {{attr} fundef} DEDENT
constdef  = "let" NAME ":" type "=" expr NEWLINE                  -- top level, stage 0 (R444)
externdef = "unsafe" "extern" STRING "lib" NAME ["link" STRING] NEWLINE INDENT
            {xitem} DEDENT                                             -- ffi modules only
xitem     = {attr} ("safe" | "unsafe") (NAME "(" [xparam {"," xparam}] ")" ["->" vtype ["from" NAME {"," NAME}]]
            ["uses" "none"] ["nonblocking"] | "static" NAME ":" type) NEWLINE
          | "type" TYPENAME "drop" NAME NEWLINE
xparam    = NAME ":" (type | ("scoped" | "escaping") fntype) | "..."

stmt      = simple NEWLINE | compound
simple    = fallback [assignop expr] | bind | "ghost" bind            -- the left side of assignop is reinterpreted as a place; ghost (R465)
          | "return" [expr] | "fail" expr | "yield" expr | "reveal" NAME {"." NAME}   -- reveal (R458)
          | "break" [NAME] | "continue" [NAME] | "assert" expr | "unreachable" | "become" postfix
bind      = ("let" | "var") pattern [":" type] "=" (lambda | cond ["else" (NAME "->" expr | diverge | cond)])   -- the else is the let's: a fallback after a bare name, a let-else otherwise (R374, R464)
assignop  = "=" | "+=" | "-=" | "*=" | "/=" | "//=" | "%=" | "**=" | "&=" | "|=" | "^=" | "<<=" | ">>="
place     = NAME {"." NAME | "[" expr "]"}
compound  = ifstmt | matchstmt | forstmt | whilestmt | loopstmt | parstmt | selectstmt
          | withstmt | unsafestmt | stagestmt | blockbind
blockbind = ("let" | "var") pattern [":" type] "=" (NEWLINE INDENT stmt {stmt} DEDENT | parfor)
          | ("let" | "var") pattern [":" type] "=" cond "else" [NAME "->"] dblock   -- R433; only let and no type, checked after parsing
ifstmt    = "if" expr block {"else" "if" expr block} ["else" block]
matchstmt = "match" expr NEWLINE INDENT arm {arm} DEDENT
arm       = pattern ["if" disj] "->" (expr NEWLINE | block)
forstmt   = [NAME ":"] "for" ["var"] pattern "in" expr lbody
whilestmt = [NAME ":"] "while" expr lbody
loopstmt  = [NAME ":"] "loop" lbody
lbody     = NEWLINE INDENT {("invariant" | "decreases") expr NEWLINE} stmt {stmt} DEDENT
parstmt   = "par" ["concurrent"] NEWLINE INDENT stmt {stmt} DEDENT | parfor
parfor    = "par" ["concurrent"] ["(" "limit" ":" expr ")"] "for" pattern "in" expr block   -- also a let value
selectstmt = "select" NEWLINE INDENT selarm {selarm} DEDENT
selarm    = postfix ["is" pattern] "->" (expr NEWLINE | block)            -- recv(), send(v), after(d), deadline(t)
withstmt  = "with" (NAME "=" ["dyn"] expr | ("deadline" | "timeout") "(" expr ")") block   -- R444
unsafestmt   = "unsafe" block
stagestmt = "#" "for" pattern "in" expr block | "#" "if" expr block ["else" block]
block     = NEWLINE INDENT stmt {stmt} DEDENT

expr      = lambda | fallback
lambda    = (NAME | "(" [NAME {"," NAME}] ")") "->" expr
fallback  = cond ["else" (NAME "->" expr | diverge | cond)]
dblock    = NEWLINE INDENT stmt {stmt} DEDENT                          -- R433; the last statement diverges, checked after parsing
diverge   = "return" [cond] | "fail" cond | "break" [NAME] | "continue" [NAME] | "unreachable"
cond      = "if" disj "then" disj "else" cond | disj                 -- branches as in Xin (R464)
disj      = conj {"or" conj}
conj      = neg {"and" neg}
neg       = "not" neg | comp
comp      = bitor [compop bitor] | bitor "is" ["not"] pattern          -- no chaining
compop    = "==" | "!=" | "<" | "<=" | ">" | ">=" | "in" | "not" "in"
bitor     = bitxor {"|" bitxor}
bitxor    = bitand {"^" bitand}
bitand    = shift {"&" shift}
shift     = arith {("<<" | ">>") arith}
arith     = term {("+" | "-") term}
term      = unary {("*" | "/" | "//" | "%" | "@") unary}
unary     = ("-" | "+" | "~" | SIGN) unary | ("copy" | "share" | "try") unary | power
power     = cast ["**" unary]
cast      = postfix {"as" base}
postfix   = atom {"." NAME | "." TYPENAME | "." "(" NAME ")" | "(" [args] ")" | "[" ixarg {"," ixarg} "]" | "?"}
ixarg     = expr | slice | (TYPENAME | scalar) ":" (expr | slice)      -- labelled and multi-axis indexing (11.2)
slice     = [bitor] ".." ["="] [bitor]
args      = arg {"," arg} [","]
arg       = expr | "&" place | "become" postfix | NAME ":" ["become" postfix | expr]   -- "name:" alone passes name; become (R456)
atom      = literal | NAME | "_" | TYPENAME | "self" | ("(" | SPLPAREN) expr ["," [expr {"," expr}]] ")"
          | "[" [expr {"," expr}] "]" | "{" [expr ":" expr {"," expr ":" expr}] "}"
          | "{" expr {"," expr} "}" | grid | subject | "#" atom | asmexpr
subject   = "." NAME
grid      = "[" unary (gelem {gelem} {";" row} | ";" row {";" row}) "]"   -- two elements or a ";", so [e] is a list (R374, R463)
row       = gelem {gelem}
gelem     = [SIGN] (literal | NAME | TYPENAME | SPLPAREN expr ")")    -- the first element is read as a unary and must have this form (R463)
asmexpr   = "asm" "(" STRING {"," asmarg} ")"                           -- only inside unsafe
asmarg    = ("in" | "out" | "inout" | "lateout") "(" (NAME | STRING) ")" (expr | NAME ":" type)
          | "clobber_abi" "(" STRING ")" | "pure" | "nomem" | "readonly" | "nostack"
literal   = INT | FLOAT | STRING | BYTES | CHAR | TSTRING | UNITNUM | DATE | TIME | INSTANT | ZONED | "True" | "False" | "None"   -- 15.1

pattern   = alt {"|" alt}
alt       = "_" | literal [".." ["="] literal] | NAME
          | vpath ["(" [ppat {"," ppat}] ")"]                          -- ".." only last, checked after parsing
vpath     = {NAME "."} TYPENAME                                        -- a variant of another module, as schema.Grocery (R652)
          | "(" pattern "," pattern {"," pattern} ")"
          | "[" [spat {"," spat}] "]" | alt "as" NAME
ppat      = pattern | NAME ":" [pattern] | ".."
spat      = pattern | ".." [NAME]

type      = vtype | fntype
vtype     = base ["?" | "|" base {"|" base}]                          -- unions only after fail
atype     = nbase ["?" | "|" base {"|" base}] | fntype                -- alias right side
fntype    = "par" fnsig | fnsig
fnsig     = "(" [type {"," type}] ")" "->" (fntype | vtype ["fail" vtype] ["uses" fcaps])   -- R464
fcaps     = capitem | "(" capitem {"," capitem} ")"                    -- several capabilities are parenthesized (R464)
base      = nbase | "{" field {"," field} "}"
nbase     = tpath ["[" targ {"," targ} "]"] | scalar | "dyn" TYPENAME ["[" type {"," type} "]"]
          | "(" ")" | "(" type ")" | "(" type "," type {"," type} ")"   -- () is unit (R374)
          | "Ref" "[" type "]" | "Ptr" "[" type "]" | "Vec" "[" NAT "," scalar "]"
targ      = type | NAT
tpath     = TYPENAME | NAME "." tpath                                   -- qualified type names, as http.HttpError
scalar    = "i8" | "i16" | "i32" | "i64" | "i128" | "u8" | "u16" | "u32" | "u64" | "u128"
          | "f16" | "bf16" | "f32" | "f64" | "f128" | "dec128" | "int" | "float" | "bool" | "char" | "str"
          | "Addr"
```

**Precedence** is Python's, tightest first: postfix, `as`, `**`, unary, multiplicative, additive, shifts, `&`, `^`, `|`, comparison and `is`, `not`, `and`, `or`, conditional, fallback `else`, lambda. **Mixes that require parentheses** (E101, auto-fixable) are the same as Xin's (13.2). Chained comparisons are E104. As in Xin, `type X = ...` is a sum exactly when every alternative is a variant, so a one-variant sum is written `type N = N(v: T)` and a field-less one `type U = U()` (R371).

**Factorings (R460, R464).** Wen uses the R428 factorings, and three places need two tokens of lookahead. A clause-form body's line is read as an expression or pattern and is an arm when `->` follows it at the top level; no statement has a top-level `->`, because a lambda is not a statement. A `(` in an expression starts a lambda's parameters, and a `(` in a type starts a function type, exactly when `) ->` closes it. Conditional branches are disjunctions, as in Xin, so an `else` after `if a then b else c` is a fallback. `fail` and `uses` after a function type belong to it, and several capabilities there are parenthesized: `g: () -> int uses (io, net)`. The `else` after a `let` belongs to the `let`: a fallback when the pattern is a bare name, a let-else that must diverge otherwise. With those three places marked, the grammar has no LALR(1) conflicts. `/` in records and parameters, `..` in patterns, and the diverging last statement of a `dblock` are checked after parsing.

### 15.3 Semantics (normative mapping to Yong)

**Bindings.** `let x = e` binds an immutable name and `var x = e` a mutable one. Only `var` bindings and `var` parameters may be assigned (E403). There is no shadowing (E230). An unused binding is lint W812.

**Functions.** Parameters are always annotated (E240), lambda parameters need not be. A single-expression body `= e` may omit `-> T`; a block body declares `-> T` unless it returns unit, and its last expression is the result. A non-unit last expression in a unit body is E312. `pub` functions write `fail` and `uses` in full; others may omit them, and the elaborator infers both per strongly connected component and shows them as inlay hints. `var p: T` is inout, `take p: T` is sink and a plain parameter is let. A `self` parameter has type `Self`.

**Signature layout.** A signature that fits in 100 columns stays on one line. Otherwise each of the `fail` and `uses` clauses goes on its own line indented 4, with the `=` at the end of the last clause line. If the parameter list still does not fit, each parameter goes on its own line indented 4, with a trailing comma, and `) -> T` opens the next line, followed by any clauses that fit on it.

```
sync_prices(src: Url, dst: Path, since: PlainDate) -> Report
    fail SyncError
    uses net, fs, time =
  let rows = fetch(src, since)?
  write(dst, rows)
```

**Calls.** `x.f(a)` and `f(x, a)` are one call, resolved by the method rules (17.2). An inout argument is written `&place` (E404 missing, E405 extra); a receiver that is a mutable place takes no mark. If two parameters share a type, arguments are passed by label (`name: e`) unless they are positional-only (E210). `name:` alone passes the variable of that name, in calls, constructors and patterns. Explicit generic application is `parse[int](s)`.

**Records, sums, aliases and newtypes (R371).** `type R = { a: A, b: B = d }` declares a record, built as `R(a: 1, b: 2)` and immutable unless bound with `var`; `evolve(r, a: 3)` returns a changed copy. Records derive Eq, Hash and Show, as in Xin. `type S = A(x: X) | B()` declares a sum whose variants are introduced here; a field-less variant is written `B()` in the declaration and in patterns. Any other `type X = T` is an alias with Xin's meaning (14.1): `X` and `T` are the same type. `type N = newtype[T]` declares a newtype, built with `N(v)` and unwrapped with `.value`, optionally refined with `where`. Fields and variants take attributes and --- doc comments on the line above; when any field has one, the formatter prints one field per line with a trailing comma. A record may end its fields with / to make the fields before it positional-only, so V2(1.0, 2.0) is legal and exempt from E210. std attributes include @wire(name: "customSlug") and @cli(short: "b"); field docs feed --help and yong doc (R437).

**Generics and traits.** `name[T: A + B, N: Nat](...)` declares type and `Nat` parameters with bounds; there is no where-clause. A trait takes an indented list of signatures, each with an optional `= body` default, and `type Item` declares an associated type. `impl Trait[Args] for Type` takes indented definitions. `dyn Trait` is a type.

**Optional values.** `T?` is `Option[T]`. `x is None` and `x is not None` narrow as in Xin. `x?` propagates None in a function returning an optional type, and `x else y` falls back.

**Failure.** `-> T fail E` is Yong's `-> T fail E`. `fail e` returns a failure. `x?` propagates, injecting into a union or converting through `From`. `x else y` handles None and failure; `x else err -> h` binds the failure; `x else return` and `x else fail e` exit. `try x` gives `Result[T, E]` for `match`. `.context(msg)` wraps a failure. Closures passed to non-escaping parameters may use `?`. In let P = e else and x else err ->, the fallback may also be an indented block whose last statement diverges (return, fail, break, continue, unreachable), as in Swift's guard (R433).

**Lambdas and the implicit subject.** `x -> e` and `(a, b) -> e` are closures. `.a.b(...)` in argument position elaborates against the parameter's expected type: a function type `R -> T` gives a closure; a type implementing `Subject[R]` (`ColExpr`, the one column-expression type of 5.6, for `Soa`, tables and frames; R448) gives a column expression rooted at the receiver's current row type R, and several `.field` uses in one argument are one subject. Implicit-subject closures may not nest (E135).

**Copies, sharing and views.** The elaborator inserts moves at last use. Storing a heap-backed value that is still used needs `copy x`, a deep copy (E413). `share x` makes an O(1) alias of a `Share` value; `copy` on one is E416. `View[T]`, `MutView[T]` and `StrView` are second-class borrows (4.5).

**Indexing and strings.** As in Xin, an `xs[i]` that cannot be proven in bounds compiles with one runtime check, which the editor shows as a hint; Wen raises no W820 for it (R372). `@forbid(panic)` forbids such checks. Slices clamp as in Xin. No `len` and no integer indexing on `str` (E504). Maps index as in Xin (R287): `m[k]` reads with a check and `m[k] = v` inserts or replaces. `m[k]` is a mutable place when `m` is, so `m[k].hits += 1` works, and `m[k] op= v` inserts `Default` first when `k` is missing (R438). `for var x in xs` binds `x` to the mutable place of each element of a mutable place `xs` (`List`, `Deque`, `Soa` rows, `Map` values through `.values()`); the body may not otherwise touch `xs` (E402), and the loop lowers to an index loop with no bounds checks (R434).

**Patterns.** `match` must be exhaustive (E503), decided with guards as facts (8.6, R606). On a sum, `_` is lint W811 (deny under `--strict` and release) rather than E502. `e is P` is a bool expression whose bindings scope over the branch it guards. `let P = e else d` requires `d` to diverge; P is never a bare name, since let x = e already binds (R374). Or-patterns, range patterns (`0..=9`), slice patterns on `View`, `StrView` and `List` (`[p, q, ..]`, `[p, ..rest]`, with `rest` a view of the scrutinee) and named-field patterns (`Rect(w:, h: 0.0)`) are available. A variant pattern may name another module's variant by its qualified name, `schema.Grocery()`, as Xin's may (R652).

**Effects and handlers.** `uses` lists capabilities, never failure. `with name = e` binds a handler for the block and `with name = dyn e` makes it dynamic.

**Concurrency.** `par` runs each top-level statement of its block as a branch; `par for x in xs` collects a `List` in input order, or streams when its body is unit, with `limit:`. `par concurrent` requires concurrent progress (E708). `select` waits on channel operations, `after(d)` and `deadline(t)`; the first ready arm in source order wins, and a recv arm's pattern matches `T?`, where None means the channel closed (E711 for any other arm form). `select` needs `uses nondet`. `with deadline(t)` and `with timeout(d)` open cancel scopes, as in Xin (R444). Actors (R435): an actor handler is a function whose first parameter is `var self: S`, the state. `actor.spawn(e, mailbox:, overflow:)` starts an actor locally and `cluster.spawn(e, placement:)` on a node (R400, R412); Wen has no spawn keyword. `h.f(args)` on an `Actor[S]` or `Remote[Actor[S]]` sends a message, found by 17.2 in the module that declares S, and its fail type is the handler's plus Timeout (9.6), plus RemoteError for a Remote handle (R408). `@reentrant` marks an actor handler.

**Contracts and tests.** `pre`, `post`, `ex`, `decreases`, `cost` (R610) and loop `invariant` lines open a body, in that order; `yong doc` renders `ex` lines under the doc comment. A `pub` item without `ex` is lint W813. A top-level function named `test_...` is a test, as in Xin.

**Numbers and operators.** As Xin: `int` is a checked i64, `/` on integers gives a float, `//` and `%` floor, `**` is power, and `& | ^ ~ << >>` are bitwise. `as`, `wrap_as`, `sat_as` and `trunc_as` convert. The percent unit is `pct`.

**Layers.** L4: `Arena`, `Ref`, `@layout` and the proof attributes. L5: `#expr`, `#for`, `#if`, user attributes, `derived` bodies, the reflection API and generators. L6: `unsafe`, `Ptr`, `RawCell`, extern blocks, `Vec` and `asm`. Extern blocks appear only in `ffi` modules (E902). None of L4 to L6 may appear in Xin.

**Clause form (R266, R370).** A function whose body is match arms (`pattern -> ...`, one per line after any contracts) elaborates to the same Yong as a body that is one `match` on its parameters: on `p` when there is one parameter, and on the tuple `(p1, ..., pn)` when there are several, each arm's pattern then being a tuple pattern. The formatter prints a function in clause form exactly when its whole body, contracts aside, is that one `match` with no other statement. The item hash and the printing are therefore the same whichever way the author wrote it. Xin has no clause form (R426). `become f(x)` maps to Yong `become` (3.5).

### 15.4 Canonical form

- 2-space indentation, LF line endings, no trailing whitespace, one final newline.
- One blank line between top-level items; inside a body, author blank lines are kept, at most one in a row and none at the body's start or end (18.5, R376).
- Items in author order; patches append new items at the end of the module.
- Signature clauses in the order `-> T from p`, `fail E`, `uses`, laid out as 15.3 says. Clauses the author left to inference are not printed.
- Contracts in the order `pre`, `post`, `ex`, `decreases`, `cost`.
- Double-quoted strings, minimal parentheses plus those E101 requires, and nothing aligned.
- Lines over 100 characters wrap inside brackets, one element per line with a trailing comma. Method chains wrap with one leading-`.` call per line.
- A lambda whose body is a field-access chain on its parameter prints as the implicit subject.

**Wen prints every Yong item (R768).** Wen is the review view, not the admission gate. A Yong extension (26.6) may land before Wen has idiomatic syntax for it; until then Wen prints its nodes in a generic canonical form that Wen also reads, so law 5 holds and the text stays writable by hand. The extension's RFC gives either the idiomatic Wen syntax or the generic form; the first extension that needs a generic form fixes its grammar for all of them. The generic form keeps every mark of the five facts (law 4).

### 15.5 Diagnostics in Wen

Message templates are keyed per code and rendered per surface, and fixes are rendered in that surface's syntax. Wen prose never uses Yong terms (inout, sink, projection, heap-backed, elaborate). A Wen diagnostic states what happened at the span, the rule in one sentence, then the fix and its cost. The JSON `message` field keeps the registry text. The voice is neutral, with no "I", "we" or blame (R296), and templates are rendered per surface as R297 says. A habit child (R662) is rendered after the fix as `note:`, naming the TypeScript or Python habit and the Wen form.

### 15.6 Example

The Xin program from 14.5, in Wen:

```
--- Fetch quotes concurrently and value a portfolio.

type Position = { sym: str, qty: int }
type Quote = { sym: str, px: float }
type PortErr = MissingQuote(sym: str) | Fetch(msg: str)

impl From[http.HttpError] for PortErr
  convert(e: http.HttpError) -> PortErr = Fetch(msg: copy e.msg)

fetch_quote(sym: str) -> Quote =
  let body = http.get_text("https://px.example/\(sym)")?
  let px = json.parse_float(body) else fail Fetch(msg: "bad price: \(sym)")
  Quote(sym: copy sym, px:)

pub value(ps: List[Position], qs: Map[str, Quote]) -> float fail PortErr =
  pre ps.len() > 0
  ex try value([Position(sym: "A", qty: 2)], {"A": Quote(sym: "A", px: 10.0)}) == Ok(20.0)
  var total = 0.0
  for p in ps
    let q = qs.get(p.sym) else fail MissingQuote(sym: copy p.sym)
    total += float(p.qty) * q.px
  total

pub main() fail PortErr uses io, net =
  let ps = [Position(sym: "SPX", qty: 10), Position(sym: "NDX", qty: 5)]
  let quotes = par for p in ps
    fetch_quote(p.sym)?
  let qs = quotes.index_by(.sym)
  io.print("portfolio value: \(value(ps, qs)?)")
```

A data-science fragment. Xin has the same types but spells the literals differently (`2.5 * bp`, `array([0.6, 0.4])`):

```
let r: Array[PlainDate, str, float] = returns(bars)
let vol = r.std(over: PlainDate) * sqrt(252.0)
let demeaned = r - r.mean(over: PlainDate)    -- broadcasts along PlainDate
let spx = r[str: "SPX"]
let fee = 2.5bp
let w = [0.6 0.4]
```
