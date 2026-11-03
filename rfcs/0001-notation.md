# 0001: Yan Notation, the third surface

- Status: accepted (the lead accepted every decision below on 2026-11-01)
- Research: [Yan Notation 2, kitchen sink](https://claude.ai/artifact/Urz3zEhuESTobLRf8aqk2X),
  [Yan Notation 1, dense ASCII surface](https://claude.ai/artifact/DubBokGE1zKEpoVzwYKfAz),
  [positional local names](https://claude.ai/artifact/V6Do54hGXcokurJ45NHF6f),
  [research track: eight extensions](https://claude.ai/artifact/CgK4yYLoCXJ55YaPrKffm8)
- Rows: R516 to R535

## Summary

Chang (畅, "unhindered", from 兰亭集序 畅叙幽情; formerly Chang 畅) is Yong's third surface, beside Xin and Wen. It is the dense,
operator-rich surface and the language-design playground: any form that makes it more expressive
is admitted as long as it can be built and lowers to the shared core. Every Yan Notation module elaborates
to the same Yan Core items, with the same item hashes, as its Yan projection. Yan Notation adds no semantics of
its own. A form that needs something Yan lacks waits for that Yan change.

This RFC records the decisions, then gives the reference a Yan Notation implementation is built from:
lexicon, precedence, grammar, the spelling-note model, and one entry per feature with its syntax,
lowering, printing, static rules and tests. The spec gets the normative core (sections 16.6, 3.6,
16.5, law 8, 26.6); this file stays the full reference until the Yan Notation build at M4, when the grammar moves
into the spec as section 16.6's appendix.

## Decisions

### D1: Yan Notation is a full third surface, spelling kept (K4)

Yan Notation is a surface like Yan Agent and Yan: files `.ynt`, a reader, a printer, a lenient reader, and the
round-trip law of 16.5 restated for Yan Notation. Several spellings may elaborate to one tree; the author's
choice is kept beside the item as hash-neutral trivia (the fourth kind, after comments, blank lines
and literal spelling). The alternative, one printing per tree as in Yan Notation 1, would have kept only about
a third of the catalog.

- Decision: accepted, 2026-11-01 (card "Full surface, spelling kept (K4)")
- Row: R516

### D2: Admission rules K1 to K5

A Yan Notation form is admitted only if it passes all five:

- **K1 Same core.** It lowers to Yan Core that Yan can already write, or to a Yan Core extension that has
  been accepted (D7, D8). Yan Notation never has private semantics.
- **K2 No invented hashed names.** A lowering never creates or drops a name the item hash includes.
  With positional locals (D4) that is parameters, fields, variants, capabilities, type parameters
  and items.
- **K3 Recognisable.** The printer has a decidable recogniser for the lowered shape, and the
  grammar stays LL(1) apart from the four factorings listed in the grammar section.
- **K4 Spelling kept like comments.** `chang(elaborate(F), spelling(F)) == fmt(F)` byte for byte.
  Spelling is not hashed. A tree with no notes prints with each shape's default spelling.
- **K5 Five facts visible.** Mutation, failure, compile time, concurrency and capability use keep a
  mark in the text. A form may move the mark to the definition (variadics, `@prop`) but never drop it.

- Decision: accepted, 2026-11-01
- Row: R517

### D3: Law 8 reads per surface

Law 8 ("each surface has exactly one canonical printing of any program") gains: "for Yan Notation, of any
program together with its spelling notes". Without notes, Yan Notation also has exactly one printing.

- Decision: accepted, 2026-11-01 (follows from D1)
- Row: R518

### D4: Positional locals in the item hash (3.6)

Locals, closure parameters, labels and join points are hashed by binding order (`%0`, `%1`, ...),
not by name. Parameters, fields, variants, capabilities, type parameters and items stay named,
because labelled arguments, `info(f)`, `Wire` and stage-0 reflection read them. Written local
names live in a per-surface name table that is hash-neutral trivia. Compiler-generated contract
and assertion text (the message a failed `post` prints) is fixed as a string literal before
hashing, so renaming a local never changes a hash. This settles the open item on closure parameter
names and lets Yan Notation admit shadowing, for-else, general parallel assignment and nested `?` in `try`.

The surfaces keep their own rules: Yan Agent and Yan still reject shadowing (E230). The change is only
in what is hashed.

- Decision: accepted, 2026-11-01
- Row: R519

### D5: Keyboard rule

Every Yan Notation token outside strings and comments is ASCII: nothing an average programmer cannot type.
Unicode is allowed in strings and comments, as in all of Yan (identifiers are ASCII, R338).
Unicode operator glyphs are not admitted, even as aliases.

- Decision: accepted, 2026-11-01 ("i dont want anything the average programmer wouldnt have in
  his keyboard, im not against unicode in general just those glyphs")
- Row: R520

### D6: The open rule and the readmitted forms

The lead's rule: accept anything that grows Yan in capability and power and can be implemented;
accept anything for Yan Notation that makes it more expressive, more powerful or sweeter. Under it, 21 of
the 23 forms the Yan Notation 2 study had rejected are admitted (two only in part), plus nested `?` in
`try`. Four stay out because they cannot be built or break D5: Unicode operators, juxtaposition
calls and implicit multiplication (ambiguous grammar), global monkey patching (changes items
other packages already hashed), and multi-shot continuations (duplicate linear values).

- Decision: accepted, 2026-11-01
- Row: R521

### D7: Research track, eight extensions (post-1.0)

Accepted for research, not normative for 1.0. Each names the 1.0 text it would amend if adopted.

1. Lexical effect handlers with one-shot resume (Effekt). Amends 10.5 for one-shot segments.
2. Information-flow labels `T@L` with `declassify` behind a `Declassify[L]` capability (Jif,
   FlowCaml). Amends law 4 (a sixth fact).
3. Graded types, 0/1/many first (Granule, Idris 2).
4. Typed staging: `Code[T]` at stage 0, splice output hashed as ordinary text (MetaOCaml, Scala 3).
   Amends 10.4 for typed code only.
5. Per-reader spelling: `yan view --as <surface>` for review and diff (Hazel, MPS).
6. Bidirectional grammars: one definition gives each surface's parser and printer (Boomerang).
7. Incremental computation: compiler queries keyed by item hash, later a `Cell[T]` library
   (Adapton, Salsa).
8. Synthesis from examples: `yan fill` for holes, tool-only, output committed as source (Myth,
   Hazel).

- Decision: accepted for research, 2026-11-01
- Row: R522 to R529

### D8: Yan Core additions behind readmitted Yan Notation forms (post-1.0 research)

1. Positional type parameters, for anonymous generics (`some Trait`).
2. `std.sym.Sym`, an interned text type hashed by content (Yan Notation symbols).
3. `std.any.Any` with type ids from the identity hash, and checked downcast (Yan Notation `any`).
4. Panic recovery at a task boundary behind a `Recover` capability; needs unwinding in the
   recovered task only (Yan Notation `recover`).
5. Software transactional memory behind an `Stm` capability, under `uses nondet` (Yan Notation `atomic`).
6. `eval` behind an `Eval` capability: parse, check and run on the reference interpreter with only
   the capabilities passed in (Yan Notation `eval`).

- Decision: accepted for research, 2026-11-01
- Row: R530 to R535

## Review findings

A review pass on 2026-11-01 checked every Yan Notation 2 entry against the spec at `spec/`. These were
fixed here and on the study page.

| # | Entry | Problem | Fix |
| --- | --- | --- | --- |
| 1 | Postfix modifiers, lockstep loops | Used `push`; R477 makes `append` the only list append | `append` |
| 2 | Tacit trains | Used `%`, which Yan Notation does not have (`%%` is remainder), and `#`, which is compile-time evaluation, not length | `(\| +/ / len \|)` |
| 3 | Lockstep loops | `for a in xs & b in ys` parses as `for a in ((xs & b) in ys)` | The iterable in a `for` header is parsed above the bitwise level (grammar `iterexpr`) |
| 4 | Ternary vs postfix `if` | `x := a if c else b` is ambiguous with the postfix modifier | After `if cond`, a following `else` makes it the conditional expression; otherwise a modifier. Decided after `cond`, one token |
| 5 | Implicit self `@x` vs attributes | A lone `@x` line reads as an attribute | `@NAME` at a statement start followed by a newline, NAME, `(` or `@` is an attribute; write `self.x` or `(@x)` there |
| 6 | Macros | Backtick quasiquote collides with infix words | MetaOCaml brackets `.< e >.` and escape `.~e` |
| 7 | Brace blocks | `{ x }` could be a block or a set literal | A brace block is admitted only where a block is expected (after `:=` of a function, `then`, `else`, `do`, loop headers, arms); elsewhere `{` opens a literal |
| 8 | Probabilistic `~` with shadowing | With shadowing admitted, `p ~ d` on a bound `p` could mean a new sample | Infix `~` never shadows: on a bound name it always observes. A second sample takes a new name (rule `chang-trace-rebind`) |
| 9 | try/catch example | The Yan lowering caught every failure of `fs.read`, not only `NotFound` | Lowering rule states that unnamed failure types propagate, with `finally` copied first |
| 10 | Notes on where, sections, yield*, try, memo, methods | Still cited rejections that D4 and D6 lift | Rewritten |
| 11 | Inheritance | `extends` would invent a field name | The field is the reserved name `super`, written by `extends` itself |
| 12 | Scans vs lambda | `max\ equity` vs `\x -> e` | `\` after an operand is the scan operator; a lambda only starts an operand |
| 13 | User fixity | The study said "no user fixity" and also readmitted it | File-local `infix` declarations relative to an existing operator; imported operators keep first-character precedence |
| 14 | Swap | Limited to permutations because of temporaries | Any parallel assignment, with positional temporaries (D4) |
| 15 | Bit-syntax example | `hdr(<<...>>)` had no signature line | Clause form needs a `::` signature line, as every clause function does |
| 16 | Regex captures | `int_of(lo)?` used `?` on an Option | Captures bind `StrView`; the example uses `parse_int(lo)?` |

## Reference

This part is what a Yan Notation reader, printer and lenient reader are built from. Where it says "as Yan",
the rule in section 15 applies unchanged. Diagnostics are named by slug (`chang-...`); each gets a
code from its section 25 family when the grammar moves into the spec at M4.

### 1. Files and encoding

- Files end in `.ynt`. A package may mix `.yag`, `.yn` and `.ynt` modules; a module is one file.
- Encoding, line ends, BOM handling, E105 and W840 are as 13.1 and 15.1 (R338). Every token outside
  string literals and comments is ASCII (D5).
- Layout is by indentation, as Yan (15.1): an indented line opens a block, a dedent closes it. A
  brace block `{ s; s }` is an alternative spelling of an indented block (entry B4).

### 2. Lexical structure

**Comments.** `--` to end of line. `---` is a doc comment, attached as Yan's docstring. `-->` is a
watch expression, as Yan.

**Identifiers.** `[a-z_][a-z0-9_]*` for values, `[A-Z][A-Za-z0-9]*` for types, variants and traits,
as Yan. `r#name` writes a foreign name (16.2); Yan's backtick form is taken by infix words.

**Keywords.** Reserved everywhere:

```
pub type trait impl use const for while loop if then else is in break continue return take copy
share from with region deadline timeout spawn select gen yield unsafe asm dyn self assert as
concurrent try catch finally defer errdefer unless until repeat do where by test actor supervise
caps deriving via not and or True False None goto label fallthrough async await recover atomic
effect handle resume extends infix infixl infixr null any some macro
```

Contextual, keywords only in one position:

| Word | Keyword where | Elsewhere |
| --- | --- | --- |
| `group`, `into`, `order`, `desc`, `select` | inside a `from` query | ordinary names |
| `forall` | after `test "..." :=` or inside a contract brace | ordinary name |
| `rel` | before `(` at the start of an expression | ordinary name |
| `like` | inside an `infix` declaration | ordinary name |
| `memo` | never (it is `std.memo`) | ordinary name |

`take` keeps its Yan meaning (a sink parameter) and is a query word only inside `from`.

**Literals.**

| Form | Meaning | Lowers to |
| --- | --- | --- |
| `42` `1_000` `0xFF` `0b1010` `3.5` | numbers, as Yan | same |
| `5s` `64MB` `2.5bp` `20%` `1usd` | unit-suffixed numbers, as Yan 11.4; `%` directly after digits is percent | `Quantity` values |
| `"a {x} b"` | interpolation; `{x:spec}` formats, `{x=}` prints `x=` then the value; `{{` is a literal brace | the R478 concat chain |
| `re"..."` `sql"..."` `ein"..."` | tagged literals, stage-0 `@literal` calls | as Yan |
| `x"89 50 4e"` | hex bytes, spaces ignored | `b"\x89\x50\x4e"` |
| `b"..."` `c'a'` | bytes, char | as Yan |
| `'SPX` | symbol (D8.2) | `std.sym.Sym` constant |
| `<<1:8, n:16>>` | bit literal (entry P4) | `Bytes` |
| `_\|_` | unreachable | `unreachable` |
| `???` | typed hole | the hole node (R212) |
| `null` | `None` at an option type | `None` |

**Operator tokens.** Lexing is longest match. The full token set:

```
:=  <-  ->  =>  |>  ||>  |?  |-  ||  |||  ??  ??=  ???  ?!  ?>  ?-  !!  /\  \/  \/=  >>>  <<<
<<  >>  ><  %%  //  ..  ..=  _|_  --  ---  -->  ++  ...  +/  */  <>/  +\  */\  <>\  +.*  =~
<~  <?  .{  .?  .!  .<  >.  .~  (|  |)  ::  :-  =  !=  <  <=  >  >=  +  -  *  /  ^  !  &  |
~  #  @  $  '  \  ,  ;  :  .  (  )  [  ]  {  }
```

`(|` and `|)` are tokens only when written with no space inside (`(| f g h |)`); `( |` is a paren
and a bar. `.<` opens a quote only at the start of an operand. `>.` closes one only inside a quote.

**Spacing.** Longest match makes `x<-1` an assignment, `a=~b` a regex match and `x:-1` a rule head.
The reader accepts them; the lenient reader warns (`chang-spacing`) and suggests the spaced form.

**User operators.** Yan Notation 1 rule (F21): `@op("<+>")` lends an operator spelling to a function or trait
method. An operator is a run of `+ - * / % < > = ! & | ~ ^ :` that is not in the reserved set above.
Its level comes from its first character (`*` `/` `%`: multiplicative; `+` `-`: additive; `<` `>`
`=` `!`: relation; `&` `|`: bitwise; `~` `^` `:`: relation), unless the same file declares it with
`infix` (entry F16).

### 3. Precedence

Tightest first. Statement forms sit below all of them.

| Level | Operators | Assoc. | Notes |
| --- | --- | --- | --- |
| 1 postfix | `f(x)` `x[i]` `.f` `.?f` `.!` `?` `?!` `'` `<? m(x)` | left | `'` after an operand with no space is transpose |
| 2 prefix | `#` `copy` `share` `<~` `*` `@` `.~` `+/` `*/` `<>/` `+\` `*/\` `<>\` | right | `*` only in `unsafe`; `.~` only inside `.< >.`; `@x` only in a method (entry T8) |
| 3 power | `^` | right | inside `[ ]`, a leading `^k` counts from the end |
| 4 unary | `-` `!` | right | |
| 5 composition | `>>>` `<<<` | right | no mixing with levels 6 and 7 without parentheses |
| 6 multiplicative | `*` `/` `//` `%%` `@` `+.*` `` `word` `` `f \ xs` | left | `@` is infix only after an operand |
| 7 additive | `+` `-` `++` | left | `+` numeric, `++` Concat |
| 8 range | `..` `..=` `by` | none | `1..` unbounded |
| 9 shift | `<<` `>>` | left | `<<` at an operand start opens a bit literal |
| 10 bitwise | `&` `><` `\|` | left | inside a comprehension head, `\|` needs parentheses |
| 11 relation | `=` `!=` `<` `<=` `>` `>=` `in` `is` `=~` | chain | same-direction chains only (entry C5) |
| 12 and | `/\` | right | `/\ i in r => e` is a quantifier |
| 13 or | `\/` | right | `\/ i in r => e` is a quantifier |
| 14 pipeline | `\|>` `\|\|>` | left | |
| 15 conditional | `if c then a else b`, multi-way `if`, `a if c else b` | right | |
| 16 fallback | `??` `?>` | right | |
| 17 lambda | `\x -> e` `\\| P => e` | right | |

Statement level: `:=`, `<-`, compound assignment (`+=` and the rest), `\/=`, `??=`, send
`ch <~ v`, match `e |?`, postfix `if`/`unless`/`for`/`while`, `with` suffix.

### 4. Grammar

EBNF as in 15.2: `{x}` repeats, `[x]` is optional, `INDENT`/`DEDENT`/`NEWLINE` come from layout.
Rules marked (F) are factorings that need more than one token of lookahead; there are four.

```
module      = [ "@@" NAME NEWLINE ] [ doc ] { use } { fixity } { item }
use         = "use" path [ ".{" NAME { "," NAME } "}" ] NEWLINE
fixity      = ( "infix" | "infixl" | "infixr" ) OP "like" OP NEWLINE

item        = { attribute } [ "pub" ] ( fn | sig_clauses | typedef | traitdef | impldef
              | capsdef | actordef | effectdef | macrodef | test | const )
attribute   = "@" NAME [ "(" args ")" ] NEWLINE                -- see T8 for @x
const       = "const" NAME [ ":" type ] ":=" expr NEWLINE

fn          = [ receiver "." ] NAME [ tparams ] "(" [ params ] ")" [ ret ] [ capset ]
              ( ":=" body | NEWLINE )
receiver    = "(" param ")"                                      -- extension definition, F12
params      = param { "," param } [ "," vparam ] | vparam
param       = [ "~" ] NAME [ ":" type ] [ "=" expr ]
vparam      = "*" NAME ":" "[" type "]"                          -- variadic, F18
ret         = "->" type [ "!" type ]  |  "!" type
capset      = "|-" capref { "," capref }
body        = expr NEWLINE | NEWLINE INDENT { contract } stmts [ where ] DEDENT | brace
where       = "where" binding { ( ";" | NEWLINE ) binding }
sig_clauses = NAME [ tparams ] "::" "(" params ")" [ ret ] [ capset ] NEWLINE
              [ INDENT { contract } DEDENT ] clause { clause }  -- (F) clause heads
clause      = NAME "(" pattern { "," pattern } ")" [ "if" expr ] ":=" body

contract    = "?{" expr "}" | "!{" expr "}" | "={" expr "}" | "@{" expr "}" | "-{" expr "}"
              NEWLINE                                            -- pre post ex invariant decreases

typedef     = "type" TNAME [ tparams ] [ "extends" type ] ":=" tbody [ deriving ]
              [ NEWLINE INDENT { fn } DEDENT ]                   -- methods inside, T5
tbody       = record_t | variant { "|" variant } | type
deriving    = "deriving" TNAME { "," TNAME }
actordef    = "actor" TNAME ":=" record_t NEWLINE INDENT { fn } DEDENT
capsdef     = "caps" TNAME ":=" capset NEWLINE
effectdef   = "effect" TNAME [ tparams ] "{" opsig { ";" opsig } "}" NEWLINE
macrodef    = "macro" NAME "(" params ")" [ ret ] ":=" body
test        = "test" STRING ":=" body

stmts       = stmt { stmt }
stmt        = simple [ modifier ] NEWLINE | compound
modifier    = ( "if" | "unless" ) expr | "for" binder "in" iterexpr | "while" expr
simple      = binding | assign | expr | "return" [ expr ] | "break" [ LABEL ] | "continue"
              | "!!" expr | "goto" LABEL | "fallthrough" | ch_send | "defer" simple
              | "errdefer" simple
binding     = lhs ":=" expr [ "else" divergent ]                -- (F) lhs is an expr reread
assign      = places "<-" exprs | place assignop expr | place "\/=" expr | place "??=" expr
ch_send     = expr "<~" expr
compound    = if_stmt | for_stmt | while_stmt | loop_stmt | repeat_stmt | try_stmt | par_stmt
              | with_stmt | region_stmt | unsafe_stmt | label_stmt | recover_stmt
              | atomic_stmt | handle_stmt | supervise_stmt | match_stmt
for_stmt    = [ LABEL ":" ] "for" binder "in" iterexpr { "&" binder "in" iterexpr }
              block [ "else" block ]
binder      = pattern | NAME "," NAME                            -- index, value (C7)
iterexpr    = expr at level 9 (shift) or tighter                 -- & and in need parentheses
repeat_stmt = "repeat" [ contract ] block "until" expr NEWLINE
try_stmt    = "try" block [ "catch" NEWLINE INDENT { "|" NAME ":" type "=>" body } DEDENT ]
              [ "finally" block ]
par_stmt    = ( "||" | "|||" ) [ "(" args ")" ] block
label_stmt  = "label" LABEL NEWLINE
match_stmt  = expr "|?" NEWLINE INDENT arm { arm } DEDENT
arm         = "|" pattern [ "if" expr ] "=>" body

expr        = lambda
lambda      = "\" params "->" expr | "\|" pattern "=>" expr { "|" pattern "=>" expr } | fallback
fallback    = cond [ ( "??" | "?>" ) fallback ]
cond        = "if" expr "then" expr "else" cond | multi_if | pipe [ "if" pipe "else" cond ]
multi_if    = "if" NEWLINE INDENT { "|" expr "=>" expr NEWLINE } "|" "_" "=>" expr DEDENT
pipe        = or { ( "|>" | "||>" ) or }
...         -- levels 13 to 1 follow the precedence table, one rule per level, as 15.2
operand     = literal | NAME | "(" expr ")" | tuple | list | map | set | record | comp
              | genexpr | train | quote | bits | query | "???" | "_" | "$" [ DIGIT ]
              | "(" OP [ expr ] ")" | "(" expr OP ")"            -- sections, F5
record      = ".{" [ field { "," field } ] "}" | ".{" "..." expr { "," field } "}"
comp        = "[" expr "|" gen { "," gen } "]" | "{" ... "}" | "||" "[" ... "]" | "||" "{" ... "}"
genexpr     = "(" expr "|" gen { "," gen } ")"
gen         = binder "in" iterexpr | NAME ":=" expr | expr      -- generator, local, filter
train       = "(|" operand operand operand { operand operand } "|)"
quote       = ".<" expr ">."
query       = "from" NAME "in" expr NEWLINE { qclause NEWLINE }
qclause     = "where" expr | "group" "by" expr "into" NAME | "select" expr
              | "order" "by" expr [ "desc" ] | "take" expr

pattern     = ... as Yan 15.2, plus: "?" NAME [ "(" pattern { "," pattern } ")" ]
              | STRING "++" NAME | bits_pat | regex_pat | ".{" field_pat { "," field_pat } "}"
bits_pat    = "<<" segment { "," segment } ">>"
segment     = ( NAME | "_" | INT ) [ ":" INT ] [ "/" ( "le" | "rest" ) ]
regex_pat   = REGEX "(" NAME { "," NAME } ")"

type        = TNAME [ "[" types "]" ] | "[" type "]" | "{" type "}" | "{" type "=>" type "}"
              | type "?" | "{" fields "}" | scalar "<" unit ">" | type "{" expr "}"
              | "some" TNAME [ "[" types "]" ] | "any" | "dyn" TNAME | "(" types ")"
```

The four factorings: (1) a binding's left side is parsed as an expression and reread as a pattern
when `:=` or `<-` follows; (2) `.{` after a receiver is a functional update when the next tokens are
`NAME :` or `NAME .`, and a cascade when the next token is `.`; (3) a `::` line announces clauses,
so each following line starting with the same NAME is a clause head; (4) `pipe "if"` reads a
condition and then looks at one token: `else` makes a conditional expression, anything else ends
the expression and the `if` is a statement modifier.

### 5. Spelling notes

A spelling note records which of several spellings produced a node. It is the fourth kind of
hash-neutral trivia (16.5).

- **Shape.** Per item, a list of `(node path, choice)` pairs. The node path is the position of the
  node in the item's Yan Core tree; the choice is a small integer naming one entry's spelling (the
  entry's "Kept" list fixes the numbering). Only nodes whose spelling differs from the default are
  recorded.
- **Storage.** In the `.ynt` file the spelling is the text itself. When a Yan Notation item passes through
  a tool that keeps trivia (the formatter, `yan view`, patches made in Yan Notation), the notes travel with
  the item as comments do. A Yan Core-level or Yan-level edit keeps the notes of untouched nodes; a node
  whose shape changed falls back to the default.
- **Validity.** A note is used only if the recogniser for that choice still matches the node;
  otherwise it is dropped silently, and the printer uses the default.
- **Law.** For any canonical Yan Notation module F: `chang(elaborate(F), spelling(F)) == F` byte for byte
  (16.5 law 6, new). For any Yan Core item Y: `chang(Y, {})` is defined and canonical.

### 6. Name table

Locals are hashed by position (D4). The written names live in a name table per item and surface:
`(binder position, name)`. The Yan Notation printer takes names from the Yan Notation table, then the Yan table, then
generates `x`, `y`, `z`, `x1`, ... avoiding names in scope. A shadowing `:=` gets a new position
and the same written name.

### 7. Printing

The printer walks the Yan Core tree. At each node it tries recognisers in the order the entry lists
them; a spelling note picks among the matches. With no note it takes the default (the "Folded"
spelling where one exists). Closures print shortest first: implicit subject, hole `_`, `$`,
composition, section, then `\x ->`. Canonical text is one space around binary operators except
`^`, `..`, `.` and postfix forms; arms align on `=>`; multi-way `if` arms align on `=>`.

### 8. Lenient reader

As 14.4 for Yan Agent: it accepts Yan tokens (`==`, `let`, `var`, `fail`, `uses`, `match`, `else` as
fallback) and the spacing forms above, and rewrites them. It never changes meaning.

### 9. Yan Notation 1 base forms

Yan Notation 2 keeps every Yan Notation 1 form (study rows F1 to F30) except the match introducer, which moves from
`e |>` to `e |?`. Each lowers one to one:

| Yan Notation | Yan | Notes |
| --- | --- | --- |
| `x := e`, `x: T := e`, `~x := e` | `let x = e`, `var x = e` | `~` is the mutation mark |
| `x <- e`, `x += e` | `x = e`, `x += e` | assignment needs a `~` binding (E403) |
| `a = b`, `a != b` | `a == b`, `a != b` | `=` is equality everywhere |
| `-> T ! E`, `\|- io, net` | `-> T fail E`, `uses io, net` | |
| `x?`, `x ?? y`, `x ?? e => h`, `!! e`, `x?!` | `x?`, `x else y`, `x else e -> h`, `fail e`, `try x` | |
| `\x -> e`, `_ * 2`, `.len()`, `f >>> g` | closures, as Yan | |
| `+/ xs`, `*/ xs`, `<>/ xs`, `+/ p in ps => e` | `sum`, `prod`, Monoid fold, `sum_by` | fixed order (R382) |
| `\|\|`, `\|\| x in xs`, `\|\|[e \| x in xs]`, `\|\|(limit: 16)[...]`, `\|\| <>/ x in xs => e` | `par`, `par for`, collecting `par for`, `limit:`, `par fold` | |
| `?{ }` `!{ }` `={ }` `@{ }` `-{ }` | `pre` `post` `ex` `invariant` `decreases` | |
| `.{a: 1}`, `.{1, 2}`, `{K => V}`, `[T]`, `{T}`, `T?` | records, maps, list, set, option | |
| `[e \| x in xs, c]` | comprehension | |
| `#e` | compile-time evaluation | |
| `@op("<+>")`, `a <> b` | user operator, Monoid combine | |
| `x'` | transpose | |
| `e \|?` then `\| P if g => e` arms | `match` | was `e \|>` in Yan Notation 1 |

### 10. Feature entries

Each entry gives the syntax, the lowering (as Yan, which 15.3 maps to Yan Core), how the printer gets
back to it, the static rules and the conformance cases to add. "Default" means the printer picks
it with no spelling note; "kept" means it prints only when a note asks.

#### Bindings

**B1 Destructuring bind.** Default.
`(lo, hi) := e`, `.{name, age} := e`, `[first, ..rest] := e else !! Usage`.
Lowers to `let` with the same pattern; a refutable pattern needs `else` with a diverging body
(R433), else `chang-refutable-bind`. A record pattern `.{a, b}` takes its type from the right side;
if that type is not a record, `chang-record-pattern`. Prints: any `let` with a pattern.
Tests: tuple, record, list-rest, refutable with and without `else`.

**B2 Parallel assignment.** Default.
`a, b <- b, a`; `a, b <- b, a + b`. Every right side is evaluated first, left to right, into
positional temporaries, then assigned left to right. A pure permutation of places lowers to
`std.mem.swap` calls (no temporaries). Rules: the left places must be distinct (`chang-assign-alias`)
and mutable (E403). Prints: a run of assignments whose right sides read only pre-run values of the
left places. Tests: swap, rotate three, Fibonacci step, aliasing failure.

**B3 `where` clauses.** Kept.
`f(...) := e` followed by an indented `where n := e; ...`. The bindings move to the top of the body
in dependency order (a topological sort; a cycle is `chang-where-cycle`). They may shadow parameters
only under the shadowing rule of B10. Prints when a note says so and the body's leading `let`s are
used only by the final expression.

**B4 Brace blocks.** Kept.
`{ s; s; e }` wherever a block is expected: after a function's `:=`, after `then`, `else`, `do`,
`=>`, and as a loop or `try` body. Elsewhere `{` opens a set, map or record-type literal. Same tree
as the indented block. Tests: one-line function body; `{x}` as a set in argument position.

**B5 Binding inside a condition.** Default.
`while (line := r.read_line()?) != ""` and `if (m := find(x)) != None`. Lowers to `loop` with the
binding, a test and `break` (for `while`), or to a `let` before the `if` whose scope is the `if`
(for `if`). The name is in scope in the body only (`chang-cond-binding-scope` on use after). Prints:
a `loop` whose first statements are a `let` and a test that breaks.

**B6 Cascade.** Default.
`req.{ .header("A", "b"); .timeout(5s) }`. Each `.m(args)` is an inout method call on the
receiver, in order. The receiver must be a `~` place (SQ29), else E403. Value of the cascade: `()`.
Prints: two or more consecutive inout calls on the same place with no other statement between.

**B7 Functional update.** Default.
`p.{pos.x: 0.0}`, `p.{name: "b", age: 3}`. Lowers to nested `evolve`. Consumes `p` at its last use;
elsewhere E413 asks for `copy p`. Paths may go through records only (not lists or maps:
`chang-update-path`).

**B8 Record spread.** Kept spelling of B7: `.{...p, name: "b"}`. Same tree. The spread must come
first and only once.

**B9 Assign if absent.** Default.
`m[k] ??= e` lowers to `if k not in m: m[k] = e`; `o ??= e` on an option place lowers to
`if o is None: o = Some(e)`. `e` is evaluated only when absent. The solver learns `k in m` (8.6).

**B10 Shadowing.** Default (D4, D6).
`x := x + 1` makes a new local with the same written name. The right side sees the old binding.
Rules: shadowing a `~` binding is allowed; shadowing a parameter is allowed; shadowing inside the
scope of a view that borrows the old binding keeps the old binding alive until the view ends (no new
rule: the old local is just another local). Infix `~` (probabilistic) never shadows. Prints: two
binders whose name table entries are equal. Yan prints the second as `x1`... unless its own table
has a name. Tests: shadow in loop body, shadow parameter, shadow then closure capture (captures the
binding visible at the closure).

#### Functions

**F1 Signature lines and clause equations.** Default for a function whose body is one match on its
parameters (the 15.3 clause form, R266).
`gcd :: (a: int, b: int) -> int` then contract lines, then `gcd(a, 0) := a` and `gcd(a, b) := ...`.
A clause binder with a parameter's name means that parameter; any other binder is a local.
Every clause must have one pattern per parameter (`chang-clause-arity`) and the same name
(`chang-clause-name`). Non-exhaustive clauses are E-level as for `match`.

**F2 Clause guards.** Default. `step(n) if n %% 2 = 0 := ...` is a guarded arm.

**F3 Pipeline.** Kept.
`x |> f(a)` is `f(x, a)`; with a `_` among the arguments, `x` goes there instead, and only one `_`
may appear (`chang-pipe-hole`). `x |> f` with no parentheses is `f(x)`. Uniform call syntax makes
the method form the default printing.

**F4 Composition both ways.** Default. `f >>> g` is `x -> g(f(x))`; `g <<< f` is the same tree and
prints only when kept. No mixing with arithmetic without parentheses (`chang-compose-mix`).

**F5 Operator sections.** Default.
`(+ 1)`, `(2 *)`, `(_ - 1)`. `(- 1)` is the number. Lowers to a one-parameter closure. Prints for
a closure whose body is one binary operator with the parameter on one side and a literal or a name
bound outside on the other.

**F6 Reusable lambda parameter.** Default. `$` may appear several times and is the one parameter;
`$1`, `$2` name positional parameters of a multi-parameter closure. `$` and `_` may not mix in one
closure (`chang-hole-mix`). The closure's extent is the innermost enclosing argument or parenthesis.

**F7 Partial application by hole.** Default. `add(1, _)` is `x -> add(1, x)`. Arguments other than
the hole are evaluated when the closure is created. An escaping partial obeys E406.

**F8 Pattern-matching lambda.** Default. `\| Some(v) => v | None => 0` is `x -> match x ...`.

**F9 Trailing `do` blocks.** Default. `f(a) do` then an indented block passes `() -> block` as the
last argument. With parameters: `f(a) do \x ->`. The closure is non-escaping (4.6), so it may use
`?` and capabilities.

**F10 Extension method definitions.** Kept. `(s: str).shout() -> str := ...` defines
`shout(s: str)`. The receiver is the first parameter.

**F11 Uniform-access properties.** Default with `@prop`. `@prop area(self: Circle) -> float := ...`
lets `c.area` mean `c.area()`. Only for a function with exactly one parameter, `self`, no failure
type and no capabilities (`chang-prop-effect`).

**F12 Infix words.** Kept. ``a `div` b`` is `div(a, b)`, at the multiplicative level.

**F13 Memoised open recursion.** Default (library). `memo \rec n -> e` is `std.memo((rec, n) -> e)`.
`std.memo` owns a map for one top-level call and frees it after; deterministic.

**F14 `@memo`.** Needs D7.4 (typed staging). `@memo fib(n: int) -> int := ...` is a stage-0 macro
that adds a cache item and rewrites calls to go through it. Until staging lands, `chang-needs-staging`.

**F15 Ternary conditional.** Default for a short `if` expression: `a if c else b`. Never
`c ? a : b`, since `?` means failure. Level 15. Prints when both branches are operands of level 14
or tighter and fit on one line.

**F16 File-local fixity.** `infixl <+> like +` at the top of a file gives `<+>` the level and
associativity of `+` for this file only. Imported operators keep first-character precedence.
Rules: declared before first use (`chang-fixity-late`), only for operators defined or imported in the
file, one declaration per operator.

**F17 Tacit trains.** Default for forks. `(| f g h |)` is `x -> g(f(x), h(x))` (an APL fork):
`(| +/ / len |)` is the mean. A two-element train `(| f g |)` is `x -> f(g(x))`. Longer trains group
from the right, as J. Each element is a function value or an operator.

**F18 Variadic parameters.** Default. `f(*xs: [int])` takes the rest of the positional arguments as
a list: `f(1, 2, 3)` is `f([1, 2, 3])`. `f(*ys)` passes a list as is. The `*` in the signature is
the allocation mark (K5). Only the last positional parameter may be variadic.

#### Patterns

**P1 Match on several values.** Default. `(op, x, y) |?` then arms. Lowers to `match` on a tuple
that the optimizer never materialises.

**P2 Active patterns.** Default. `@pattern f(s: S) -> T?` (or `-> bool`) declares an extractor.
In a pattern, `?f(p)` calls `f` on the scrutinee and matches `Some(p)`; `?f` with a `bool` result is
a test. `f` must be pure (no capabilities, no failure: `chang-pattern-effect`). Arms keep source order.

**P3 String-prefix patterns.** Default. `"GET " ++ path` matches a `str` starting with the literal
and binds the rest as a `StrView` into the scrutinee (store it with `copy`).

**P4 Bit syntax.** Default. In a pattern, `<<ver:4, ihl:4, _:8, len:16, _/rest>>` matches a
`View[u8]`; widths are in bits, big-endian unless `/le`; `NAME/rest` binds the remaining bytes and
must be last and byte-aligned. As a literal, `<<1:8, n:16>>` builds `Bytes`. A pattern fails if the
view is too short; its length test proves every index. Widths must sum to whole bytes
(`chang-bits-align`); a field over 64 bits is `chang-bits-width`.

**P5 Regex patterns.** Default (library). `re"(\d+)-(\d+)"(lo, hi)` matches with `std.regex` and
binds captures as `StrView`s. The capture count must equal the names (stage-0 error, R484).

#### Control

**C1 Postfix modifiers.** Kept. `stmt if c`, `stmt unless c`, `stmt for x in xs`, `stmt while c`
apply to one simple statement. Marks stay on the statement.

**C2 `unless`, `until`.** Kept spellings of `if not`, `while not`.

**C3 `repeat ... until`.** Default. `repeat` block `until c` runs the block, then tests `c`.
Lowers to `loop` with a final test and `break`. A `-{ e }` line may follow `repeat`.

**C4 Multi-way `if`.** Default for an `if`/`else if` chain of three or more arms. Arms in source
order; a final `| _ =>` arm is required (`chang-if-total`).

**C5 Comparison chains.** Default. `a < b <= c` is `a < b and b <= c`. All operators in a chain
face the same way (`<` and `<=`, or `>` and `>=`, or `=` alone: `chang-chain-mixed`), and each middle
operand is a place or a literal, so it is evaluated once.

**C6 Lockstep loops.** Default. `for a in xs & b in ys` is `for (a, b) in zip(xs, ys)`; stops at the
shorter. The iterable is parsed at the shift level or tighter, so `&` and `in` inside it need
parentheses.

**C7 Enumerated loops.** Default. `for i, x in xs` is `for (i, x) in xs.enumerate()`. The solver
gets `0 <= i < xs.len()` (8.6).

**C8 Stepped ranges and end indexes.** Default. `0..n by 2` is `range(0, n).step_by(2)`. `xs[^k]` is
`xs[xs.len() - k]` and `xs[a..^k]` is `xs[a..xs.len() - k]`; `^k` only on a place (`chang-end-index`).

**C9 Unbounded ranges.** Default. `n..` is `count_from(n)`, an iterator.

**C10 Generator expressions.** Default. `(e | x in xs, c)` is a lazy `iter` chain (R510);
`[e | ...]` collects. Nothing is allocated for the parenthesised form.

**C11 Comprehension-local bindings.** Default. `y := f(x)` among the generators binds once per
element.

**C12 Delegating yield.** Default. `yield* g` is `for x in g: yield x`.

**C13 for-else, while-else.** Default (D4). `for x in xs` block `else` block: the `else` block runs
when the loop ends without `break`. Lowers to a loop whose normal exit jumps to a join point holding
the `else` block and whose `break` skips it. Labelled `break` out of an outer loop skips it too.

**C14 `goto` and `fallthrough`.** Default (D6). `label L` marks a statement in the same function;
`goto L` jumps there. Lowers to a join point; locals live at `L` become its parameters. Rules:
the target is in the same function, in the same block or an enclosing one (`chang-goto-scope`); a
jump may not enter a scope or skip a binding that is used after `L` (`chang-goto-binding`); a jump
may not leave a `par` branch or a `with` block. A jump out of a block runs that block's `defer`s, as any exit does (X2). `fallthrough` as the last
statement of a match arm jumps to the next arm's body; that arm must bind no names
(`chang-fallthrough-binds`) and the current arm must not be the last.

#### Failure

**X1 try / catch / finally.** Default.
```
try
  stmts
catch
  | e: T1 => body
  | e: T2 => body
finally
  stmts
```
Each `?` in the `try` block becomes a `match try` at that site. Its `Err` arms are the `catch` arms
whose type is that site's failure type, or a variant set of it; an error no arm names re-raises
with `fail e` after the `finally` block. With D4, a `?` may sit anywhere in an expression: the
partial results to its left are bound to positional locals first, so evaluation order is kept.
`finally` is a `defer` on the block. Catches typed failures only; panics go to X7. Rules: an arm
whose type no site can fail with is `chang-catch-dead`; arm order matters only for overlapping types.

**X2 `defer`, `errdefer`.** Default. The statement is copied to every exit of the enclosing block
(normal end, `return`, `break`, `continue`, `?`, `!!`, `goto` out), in reverse order of
declaration; `errdefer` goes only to failing exits. Not run on panic (8.3). A deferred statement that
can fail must handle it (`chang-defer-fails`).

**X3 Propagate with context.** Default. `e ?> msg` is `e.context(msg)?`; the failure type needs a
context chain (8.1).

**X4 Typed hole.** `???` is the hole node (R212). The item checks; reaching it panics.

**X5 Force unwrap.** Default. `o.!` is `o else unreachable`.

**X6 Optional chaining.** Default. `a.?f` on `Option[A]`: if `A.f` is `Option[B]`, it is
`a.and_then(.f)`; otherwise `a.map(.f)`. Not defined on non-options (`chang-optchain-type`).

**X7 `recover`.** Needs D8.4. `recover` block `else e => body` runs the block as a child task;
a panic inside it becomes the failure `Panicked(msg)` and runs `body`. Needs `|- recover`.
Until the addition lands, `chang-needs-recover`.

#### Types

**T1 Refinements on signatures.** Default. `n: int{_ >= 0}` on a parameter is `pre n >= 0`; on the
result, `-> int{_ * _ <= n}` is `post out * out <= n`. `_` is the value. No refined type is made.

**T2 Units in type position.** Default. `float<m/s>` is `Quantity[float, m / s]`. `<` after a scalar
type name opens a unit.

**T3 `deriving`.** Kept placement of `@derive`.

**T4 Delegation.** Default (library). `impl Trait for T via .field` is `@delegate(Trait, field:
"field")`.

**T5 Methods inside the type.** Kept. Functions indented under a `type` are module-level functions
whose first parameter is `self: T` or `~self: T`.

**T6 Capability sets.** Default. `caps Svc := |- net, log` is `type Svc = uses net, log` (R431).

**T7 Actor declarations.** Default. `actor Book := {...}` with handler functions indented under it is
a record type plus handlers for `std.actor` (9.6).

**T8 Implicit self.** Default inside a function with a `self` parameter. `@x` is `self.x`; `@x <- v`
assigns. At a statement start, `@NAME` followed by a newline, a NAME, `(` or `@` is an attribute;
write `self.x` or `(@x)` there (`chang-attr-or-field` explains). Outside a method, `chang-self-missing`.

**T9 Inheritance as delegation.** Default (D6). `type B extends A := {extra fields}` adds a first
field `super: A` and, for each trait `A` implements, a delegating impl unless `B` writes its own.
Method lookup `b.f()` that finds no `f` for `B` tries `f(b.super)`, one level per `extends`, at
compile time. No subtyping: a `B` is not an `A`; pass `b.super`. Monkey patching stays out.

**T10 Anonymous generics.** Needs D8.1. `xs: some Iterator[int]` in a parameter makes a type
parameter with no name, hashed by position. In a result type it is an opaque return type.

**T11 Implicit widening.** Default (D6). In an arithmetic operand, assignment or argument, a value
is widened if the conversion is lossless: signed to wider signed, unsigned to wider unsigned or
wider signed, `f32` to `f64`. Lowers to the explicit conversion call. Never to or from floats and
integers, never narrowing.

**T12 `null`.** Default. `null` is `None` where an option type is expected (`chang-null-type`
otherwise).

**T13 Dynamic `any`.** Needs D8.3. `x: any` is `dyn Any`; `x is int` and `x |? | n: int => ...` are
checked downcasts.

#### Collections and literals

**L1 Concatenation.** `a ++ b` is `Concat`; `+` stays numeric. A moved left side is reused.

**L2 List spread.** `[0, ...xs, 9]` is `[0] ++ xs ++ [9]`.

**L3 Regex match.** `s =~ re"..."` is `re"...".is_match(s)`; the right side must be a `re` literal.

**L4 Hex bytes.** `x"89 50"`.

**L5 Debug interpolation.** `"{x=}"` prints `x=` and the value.

**L6 Symbols.** Needs D8.2. `'SPX` is a `Sym` constant interned at load.

#### Operators

**O1 Scans.** `+\ xs` is `xs.scan(add)`; `f \ xs` is `xs.scan(f)`. Left to right.

**O2 Inner product.** `a +.* b` is `math.ein("C[i, k] = A[i, j] * B[j, k]", a, b)`; on vectors,
`dot`. Other pairs (`max.+`) follow `f.g`. Fixed order (R382).

**O3 Einstein literals.** `ein"ij,jk->ik"(a, b)` rewrites at stage 0 to the R263 form.

**O4 Bounded quantifiers.** `/\ i in r => p` is `all i in r: p`; `\/ i in r => p` is `any`.

**O5 Unsafe deref.** `*p`, `*p <- v` inside `unsafe` are `p.read()`, `p.write(v)`.

#### Concurrency

**K1 Concurrent block.** `|||` is `par concurrent`.

**K2 Parallel pipeline stage.** `xs ||> f(_)?` is `par for x in xs: f(x)?`. Results in input
order; the lowest failing index wins (9.3). `?` inside the stage propagates out of the `par for`.

**K3 Parallel map comprehension.** `||{k => v | x in xs}` collects into a map in input order.

**K4 Channels.** `ch <~ v` is `ch.send(v)`; `<~ ch` is `ch.recv()`. Both fail; the `?` is written.

**K5 Actor calls.** Kept. `a <? m(x)` is `a.m(x)`, which can fail with `Timeout`.

**K6 Supervision.** `supervise one_for_one(max: 3, per: 5s)` block starts children under a
supervisor (R397).

**K7 Lattice join.** `c \/= v` is `c.put(v)` on a `Lub` cell; reading inside the block is E717.

**K8 Postfix `with`.** Kept. `s with rand = seeded(7)` is a one-statement `with` block.

**K9 async / await.** Default (D6). Inside `|||` (or `par concurrent`), `h := async e` starts `e`
as a branch and returns a handle; `await h` joins it and yields its result, failing as the branch
failed. Handles cannot leave the block (`chang-async-escape`); a handle never awaited is joined at
the block's end. No function colouring: `async` and `await` are expressions in ordinary functions.

**K10 Software transactional memory.** Needs D8.5. `atomic` block reads and writes `TVar` cells;
needs `|- stm` and `uses nondet`.

#### Effects and staging

**E1 Effects with one-shot resume.** Needs D7.1. `effect Ask { ask() -> int }` declares an effect;
a function that performs it writes `|- Ask`. A handler:
```
handle
  body
with Ask
  | ask() resume k => k(42)
```
`k` is used at most once (`chang-resume-twice`). Until handlers land, `chang-needs-handlers`.

**E2 Macros and quasiquote.** Needs D7.4. `macro m(e: Code[int]) -> Code[int] := .< .~e * 2 >.`;
`.< e >.` is a typed quote, `.~e` splices. Expanded at stage 0; the expansion is hashed as written
text. Until staging lands, `chang-needs-staging`.

**E3 `eval`.** Needs D8.6. `eval[T](src, caps: .{log})` parses, checks and runs `src` on the
interpreter with only the listed capabilities; fails with `EvalErr`. Needs `|- eval`.

#### Meta, data and tests

**M1 Datalog rules.** Library. `rel(edge: deps)` block of `head :- body` rules and `?- query`.
Rules are a stage-0 string checked against the relation types; semi-naive, sorted results.

**M2 Doctests.** Kept. `--- >>> expr` followed by `--- value` is an `ex` clause.

**M3 Query comprehensions.** Kept. `from t in xs` with `where`, `group by ... into`, `select`,
`order by ... [desc]`, `take n`; each clause is one `Table` operation (11.9).

**M4 Probabilistic models.** Library. In a function with exactly one `~t: Trace` parameter,
`x ~ d` samples into a new `x` when `x` is unbound and observes when bound. Never shadows
(`chang-trace-rebind` if `x` was bound by a shadowing `:=` in the same block).

**M5 Property tests.** Library. `test "name" := forall t: T => p` checks `p` for generated `t`
(seeded, shrinking). `T` needs `@derive(Arbitrary)`.

### 11. Out

| Form | Why |
| --- | --- |
| Unicode operators, `\name` escapes | D5 |
| Juxtaposition calls `f x`, implicit multiplication `2x` | collide with unit suffixes and postfix modifiers; the grammar would be ambiguous (law 3) |
| Global monkey patching, open classes | changes items other packages already hashed; breaks coherence (law 6) |
| Multi-shot continuations | resuming twice duplicates linear values and capabilities |

### 12. Conformance

Agents do not edit `conformance/`; a separate PR proposes the cases. Each entry above lists its
cases. In addition: every pass case in `conformance/yn/` gets a Yan Notation projection checked by
`chang(elaborate(W), {})` then `elaborate` back to the same item hashes; and every Yan Notation pass case
checks law 6 of 16.5 with and without spelling notes.

## Spec changes

- 1: the surfaces table and the reader diagram gain Yan Notation.
- 2: law 8 gains the Yan Notation clause (D3).
- 3.6: positional locals and the name table (D4).
- 16.5: law 6 for Yan Notation, and spelling notes as the fourth kind of trivia.
- 16.6 (new): Yan Notation, the third surface: D1, D2, D5, D6, with this RFC as the reference until M4.
- 23: M4 gains the Yan Notation reader, printer and lenient reader.
- 26.6 (new): research track, D7 and D8.
