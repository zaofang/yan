## 13. Yan Agent: lexical rules and grammar

Yan Agent is the surface written by language models. It is shaped like typed Python 3.12, covers layers L0 to L3 plus contracts, and every signature is explicit. Files use the extension `.yag`.

### 13.1 Lexical rules

- **Encoding.** UTF-8. Identifiers are ASCII; Unicode may appear only in strings and comments. Source has no byte order mark and uses LF line ends; yan fmt removes a BOM and converts CRLF, and an unformatted BOM is E105. A string literal whose content is not NFC is W840, with a fix-it that normalizes it; literal contents are never normalized silently. Yan meets the UTS #55 source-code-handling recommendations (R338).
- **Lexical safety.** The lexer rejects bidirectional control characters, zero-width characters and Unicode tag characters (U+E0000 to U+E007F) anywhere outside string literals (E105), which closes the "Trojan Source" class. `yan fmt` prints any such character inside a literal as an escape. W801 flags confusable names.
- **Value names** (variables, functions, fields, modules, capabilities): lowercase words joined by single underscores, matching `[a-z][a-z0-9]*(_[a-z0-9]+)*`. No leading, trailing or doubled underscore (E110). Longer than 32 characters is W111.
- **Type names**: PascalCase matching `[A-Z][a-z0-9]*([A-Z][a-z0-9]*)*`, with no two consecutive capitals, so `HttpError`, not `HTTPError` (E112, auto-fixable). Every name is therefore a well-defined word list (section 16).
- **Comments** run from `#` to end of line. A string literal as the first statement of a module, function or class is its docstring.
- **Watch expressions.** A comment line starting `#>` holds an expression that tools (the language server, the notebook) evaluate and display. The compiler ignores it, and it does not affect item hashes.
- **Layout.** Python's INDENT/DEDENT rules. Canonical indentation is 4 spaces. Newlines inside brackets are joined. Backslash continuation and `;` are errors (E103).
- **Literals.** Integers in decimal, `0x`, `0o` and `0b`, with `_` separators. Floats with `.` or exponent. Strings `"..."`, raw `r"..."`, bytes `b"..."`, C strings `c"..."` (interior NUL is E140), triple-quoted `"""..."""`, and f-strings `f"...{expr}...{expr:fmt}..."`. A `char` literal is `c'a'`; a plain `'a'` is a `str`, accepted by the lenient reader and printed double-quoted. `True`, `False`, `None`. Date, time and zoned values are constructor calls with literal arguments (`PlainDate(2026, 10, 30)`), validated at compile time against the pinned tzdb like Yan's literals (E540, E541). Unit quantities are products: `2.5 * bp`, `150 * ms`.
- **Template strings (R452).** `t"...{expr}...{expr:fmt}..."` has the grammar of an f-string and the type `Template`. Its literal parts are stage-0 constants, and each interpolation keeps its value, type and format spec. A `Template` never converts to `str` implicitly; `render(escaper)` or an API that takes a `Template` consumes it. std and first-party APIs that build queries, markup, URLs or shell commands take `Template`, never `str`, and log events take one so that each interpolation becomes a field. A `str` or f-string passed where a `Template` is expected is E233, whose fix changes `f` to `t`. Yan accepts the same literal.

**Keywords.** All are reserved except the soft keywords below (R347, R422, R373).

| Group | Keywords |
| --- | --- |
| From Python, same meaning | `def` `class` `type` `return` `if` `elif` `else` `for` `while` `in` `not` `and` `or` `is` `break` `continue` `pass` `lambda` `match` `case` `assert` `True` `False` `None` `as` `from` |
| From Python, adapted | `with` (binds a capability handler or opens a cancel scope); `from` (view sources in a signature) |
| Yan Agent only | `pub` `var` `take` `uses` `fail` `pre` `post` `ex` `invariant` `decreases` `cost` `par` `concurrent` `trait` `impl` `out` `become` `unreachable` |
| Rejected with a targeted diagnostic | `try` `except` `finally` `raise` (E120: use `fail` and `Result`); `global` `nonlocal` `del` `yield` `async` `await` (E121) |
| Accepted by the lenient reader only | `import`, and `from` at the start of a statement (rewritten to qualified names, N01) |

**Soft keywords (R347, R422, R373, R462).** `var`, `take`, `uses`, `pre`, `post`, `ex`, `invariant`, `decreases`, `cost`, `concurrent`, `trait`, `impl`, `out`, `ghost`, `lemma` and `reveal` are keywords only where the grammar expects one (`steps` and `alloc` only inside a `cost` line, R610), as Python treats `match`, `case` and `type`, and Yan Agent's own match, case and type are soft the same way (R373); elsewhere they are ordinary names, so `out = []` binds a variable. Where the grammar accepts both a soft keyword and a name, the word is the keyword unless the next token is `=` or an augmented assignment, so `match = re.match(p, s)` and `type = "x"` bind variables (R462). `par`, `fail`, `become` and `unreachable` are reserved. The contract words `pre`, `post`, `ex`, `decreases` and `invariant` are keywords only in the contract region (the lines that open a function or loop body), and there only when the next token is not `=` or an augmented assignment. After the first other statement they are names, and a contract line there is E106 with a fix that moves it. In `post`, `out` always means the result; a parameter named `out` on a function with `post` is E230 with a rename fix. These grammar words are keywords only in one position each (R420): `newtype` after `type X =`, `where` after a newtype, `none` after `uses`, `deadline` and `timeout` followed by `(` after `with`, `old` in `post`, and `Nat` in a bound. The built-in type words of 13.2's `base` and `scalar` (`int`, `str`, `list`, `dict`, `Result`, `View`, `Callable` and the rest) are keywords only at the start of a type, so `list[int]` is the built-in type and `list(xs)` and `str(x)` are calls (R460).

### 13.2 Grammar

This is the complete Yan Agent grammar. Notation: `{x}` is zero or more, `[x]` optional, quoted text literal. NEWLINE, INDENT and DEDENT come from the layout rules.

```
module    = {modattr} [docstring] {item}
modattr   = "@@" path ["(" [args] ")"] NEWLINE       # module attribute, as @@prove (R466)
item      = {decorator} ["pub"] (funcdef | classdef | typedef | traitdef | impldef | constdef | lemmadef)
decorator = "@" path ["(" [args] ")"] NEWLINE
docstring = STRING NEWLINE

funcdef   = "def" NAME [tparams] "(" [params] ")" ["->" rettype] [uses] ":" fbody
rettype   = (type ["fail" errtype] | "fail" errtype) ["from" NAME {"," NAME}]   # T fail E; fail E alone returns None (R214, R420); type is never Result (R425)
uses      = "uses" ("none" | capref {"," capref})
capref    = path ["as" NAME]                                  # uses Kv as cache (R461); a TYPENAME may name a capability set (R431)
tparams   = "[" tparam {"," tparam} "]"
tparam    = TYPENAME [":" bound {"+" bound}]                  # type parameter
          | NAME ":" type                                     # compile-time value parameter
bound     = path ["[" type {"," type} "]"] | "Nat"
params    = param {"," param} [","]                           # at most one "/" and one "**", in that order, checked after parsing (R460)
param     = ["var" | "take" | "ghost"] (NAME | "_") ":" type ["=" expr] | "/" | "**" NAME ":" type   # ghost (R458, R465)
fbody     = NEWLINE INDENT {contract} {stmt} DEDENT           # a leading string statement is the docstring (13.1); clause form is lenient only (N29, R426)
contract  = ("pre" | "post" | "decreases") expr NEWLINE
          | "cost" bound {"," bound} NEWLINE              # R610
          | "ex" expr ["with" NAME "=" expr {"," NAME "=" expr}] NEWLINE   # handlers for the example (R210)
bound     = ("steps" | "alloc") "<=" expr
lemmadef  = "lemma" NAME [tparams] "(" [params] ")" ":" fbody   # pre and post as contract lines (R458, R465)

classdef  = "class" TYPENAME [tparams] ":" NEWLINE INDENT [docstring]
            (field {field} | "pass" NEWLINE) DEDENT
field     = NAME ":" type ["=" expr] NEWLINE
typedef   = "type" TYPENAME [tparams] "=" (variant {"|" variant} | base {"|" base} | "uses" capref {"," capref} | "newtype" "[" type "]") ["where" expr] NEWLINE   # sum iff every alternative is a variant (R423)
variant   = TYPENAME "(" [ifield {"," ifield}] ")"          # inline variant; Name() when field-less (R214, R423)
ifield    = NAME ":" type ["=" expr]
traitdef  = "trait" TYPENAME [tparams] ":" NEWLINE INDENT [docstring] {assoc} {decorator} sigdef
            {{decorator} sigdef} DEDENT
assoc     = "type" TYPENAME NEWLINE
sigdef    = "def" NAME [tparams] "(" [params] ")" ["->" rettype] [uses] ":" ("..." NEWLINE | fbody)
impldef   = "impl" path ["[" type {"," type} "]"] "for" type ":" NEWLINE INDENT
            {"type" TYPENAME "=" type NEWLINE} funcdef {funcdef} DEDENT
constdef  = NAME ":" type "=" expr NEWLINE                  # package: Package = Package(...) is literal-only (19.1)

stmt      = simple NEWLINE | compound
simple    = exprs [[":" type] "=" exprs | augop expr]       # exprs is reinterpreted as a target, a place or a tuple of places (R428, R216)
          | "ghost" tname [":" type] "=" expr                 # ghost binding (R458, R465)
          | "return" [exprs] | "become" postfix | "fail" expr | "break" | "continue" | "pass"
          | "assert" expr | "unreachable" | "reveal" path    # reveal (R458)
exprs     = expr {"," expr}
augop     = "+=" | "-=" | "*=" | "/=" | "//=" | "%=" | "**=" | "&=" | "|=" | "^=" | "<<=" | ">>="
place     = NAME {"." NAME | "[" expr "]"}                    # assignment left side, after reinterpretation
target    = tname | tname "," tname {"," tname}               # binding left side after reinterpretation, and a for target
tname     = NAME | "_"
compound  = ifstmt | whilestmt | forstmt | matchstmt | withstmt | parstmt
ifstmt    = "if" expr ":" block {"elif" expr ":" block} ["else" ":" block]
whilestmt = "while" expr ":" lbody
forstmt   = "for" target "in" expr ":" lbody
lbody     = NEWLINE INDENT {("invariant" | "decreases") expr NEWLINE} stmt {stmt} DEDENT
matchstmt = "match" expr ":" NEWLINE INDENT case {case} DEDENT
case      = "case" pattern ["if" expr] ":" block
withstmt  = "with" (NAME "=" expr | "deadline" "(" expr ")" | "timeout" "(" expr ")") ":" block
parstmt   = "par" ["concurrent"] ["(" "limit" "=" expr ")"] (":" NEWLINE INDENT stmt {stmt} DEDENT | "for" target "in" expr ":" lbody)   # par for statement (R427)
block     = simple NEWLINE | NEWLINE INDENT stmt {stmt} DEDENT

pattern   = "_" | literal | path ["(" [parg {"," parg}] ")"]   # name, qualified constant or class pattern; case Name(): when field-less (R423)
          | "(" pattern "," pattern {"," pattern} ")" | pattern "as" NAME
parg      = pattern | NAME "=" pattern                        # keyword patterns follow positional ones, checked after parsing

expr      = lambda | fallback
lambda    = "lambda" [NAME {"," NAME}] ":" expr
fallback  = cond ["else" (NAME "->" cond | cond | "return" [cond] | "fail" cond | "break" | "continue")]
cond      = disj ["if" disj "else" cond]
disj      = conj {"or" conj}
conj      = neg {"and" neg}
neg       = "not" neg | comp
comp      = bitor [compop bitor]                              # no chaining (E104; lenient N22)
compop    = "==" | "!=" | "<" | "<=" | ">" | ">=" | "in" | "not" "in" | "is" ["not"]
bitor     = bitxor {"|" bitxor}
bitxor    = bitand {"^" bitand}
bitand    = shift {"&" shift}
shift     = arith {("<<" | ">>") arith}
arith     = term {("+" | "-") term}
term      = unary {("*" | "/" | "//" | "%" | "@") unary}
unary     = ("-" | "+" | "~") unary | power
power     = postfix ["**" unary]
postfix   = atom {"." (NAME | TYPENAME) | "(" [args] ")" | "[" (expr | slice) "]" | "?"}   # qualified names are field accesses here, resolved by name lookup
args      = arg {"," arg} [","]
arg       = expr | "&" place | "become" postfix               # become in a constructor (R456; E143 elsewhere)
          | NAME "=" ("&" place | "become" postfix | expr)   # keyword inout argument (R282)
atom      = literal | fstring | NAME | TYPENAME | "_" | "..." | "(" expr ")" | "(" expr "," [expr {"," expr}] ")"
          | "[" [expr {"," expr}] "]" | "{" [kv {"," kv}] "}" | "{" expr {"," expr} "}"
          | comprehension | "par" ["concurrent"] comprehension
comprehension = "[" expr "for" target "in" disj ["if" disj] "]"
          | "{" kv "for" target "in" disj ["if" disj] "}" | "{" expr "for" target "in" disj ["if" disj] "}"
kv        = expr ":" expr
slice     = [expr] ":" [expr] [":" [expr]]                 # step (R286)
literal   = INT | FLOAT | STRING | BYTES | CHAR | "True" | "False" | "None"   # 13.1
fstring   = FSTRING | TSTRING                                 # f"..." and t"..." (R452)

type      = base ["|" "None"]
base      = scalar | "str" | "bytes" | "char" | "bigint" | "decimal" | "None"
          | "list[" type "]" | "dict[" type "," type "]" | "set[" type "]"
          | "deque[" type "]" | "heap[" type "]" | "sorted_dict[" type "," type "]" | "sorted_set[" type "]"
          | "soa[" type "]" | "tuple[" type {"," type} "]" | "Result[" type "," errtype "]"
          | "View[" type "]" | "MutView[" type "]" | "StrView"
          | "Array[" atarg {"," atarg} "]" | "Table[" type "]" | "Quantity[" type "," dim "]"   # Array: axes, then the element type, split after parsing
          | ["par"] "Callable[[" [type {"," type}] "]," type [callsfx] "]"   # (R373); par prefix (R436)
          | path ["[" (type | INT) {"," (type | INT)} "]"]
scalar    = "int" | "float" | "bool"
          | "int8" | "int16" | "int32" | "int128"                # int is 64-bit; int64, float64 lenient (N27)
          | "uint8" | "uint16" | "uint32" | "uint64" | "uint128"
          | "float16" | "bfloat16" | "float32" | "float128"
errtype   = path {"|" path}
callsfx   = "," ("fail" "(" errtype ")" ["," "uses" "(" NAME {"," NAME} ")"] | "uses" "(" NAME {"," NAME} ")")
atarg     = INT | type                                        # an axis is a literal size, a size variable or a label type
path      = NAME [ptail] | TYPENAME                           # a.b.C or a.b.c
ptail     = "." (NAME [ptail] | TYPENAME)
dim       = NAME {("*" | "/") NAME} ["**" INT]
```

**Precedence** is Python's, tightest first: postfix, `**`, unary, multiplicative, additive, shifts, `&`, `^`, `|`, comparison, `not`, `and`, `or`, conditional, fallback `else`.

**Factorings (R428).** Law 3's one token of lookahead holds after these factorings. A statement that starts with an expression is parsed as `expr` and reinterpreted as a `place` or `target` when `=` or an augmented assignment follows. In a fallback, `else NAME` is reinterpreted as a binder when `->` follows. `{` decides dict or set at the first `:` or `,`, and a comprehension at `for`; `(` decides parenthesis or tuple at `,`. `not in` and `is not` are single operators. After `type X =`, `Name(` starts an inline variant. R460 adds seven more. `NAME =` in an argument list or class pattern is decided at `=`. `[` decides list or comprehension at `for`, and a subscript decides index or slice at `:`. At statement start, `par` begins an expression when `[` or `{` follows it (after an optional `concurrent`) and a statement otherwise. `pattern as NAME` is a suffix on a complete pattern. A trailing comma is decided by the token after it. A tuple target is an `exprs` reinterpreted as a `target`. A leading string statement is the docstring, and the order of `/` and `**` in a parameter list and of keyword patterns, and the split of `Array` axes from the element type, are checked after parsing. Soft keywords are resolved by position and the next-token rule (13.1, R462) before parsing. CI builds the LL(1) table of the factored grammar and fails on any conflict, and the constrained-decoding grammars of 21.3 are generated from the same source. With these factorings the grammar has no LALR(1) conflicts, which proves it unambiguous (R460; report https://claude.ai/artifact/BgkEJ6284UCgx2rJmMtGfa).

**Mixes that require parentheses** (E101, auto-fixable):

- `and` and `or` in one expression;
- unary minus applied to a `**` operand (`-x ** 2`);
- `%` combined with `*`, `/` or `//`;
- a bitwise operator mixed with comparison, arithmetic or a different bitwise operator;
- a conditional expression used as either side of a fallback `else`;
- an arithmetic or bitwise expression as the right operand of a fallback `else`: `d.get(k) else 0 + 1` must be written `(d.get(k) else 0) + 1` or `d.get(k) else (0 + 1)` (R288). The fix offers both readings.

Chained comparisons (`a < b < c`) are E104; the lenient reader rewrites them as `a < b and b < c` when `b` is a name or literal (N22). A comprehension has exactly one `for` clause (E107); nested iteration uses statements. In a call with a single argument, a generator expression `f(e for x in xs)` is read as `f([e for x in xs])` (N18).

**Fused comprehensions (R480).** A list comprehension whose value is only iterated MUST be lowered as a fused adapter chain with no intermediate list. It is only iterated when it is the iterable of a `for`, the single argument of `sum`, `min`, `max`, `any`, `all`, `count` or `len`, or the argument of `extend` or `collect`; the N18 reading of `f(e for x in xs)` is included. A comprehension that is bound, returned, stored or passed to any other parameter is built as written. Set and dict comprehensions are excluded, since their deduplication is observable, and so are `par` comprehensions. Results cannot change, because allocation is not observable (R385). R623 extends the guarantee to every chain of std adapters.
