# Yan surfaces: syntax status and guide

This page answers one question for each of the 18 Yan surfaces: is its syntax defined, and what does
it look like? It is a guide, not the spec. Where they differ, `spec/` wins.

- **Defined**: the spec has a complete normative grammar. Examples below are copied from the spec,
  `conformance/` or `examples/`.
- **Partial**: some syntax exists (an accepted RFC grammar, a grammar sketch, or one example), but no
  complete grammar is in the spec. The existing text is quoted; the gaps are filled with **MOCK**
  syntax.
- **Undefined**: no syntax anywhere. (No surface is in this state today.)

Every block marked **MOCK** is non-normative. It is not a spec row, carries no row number or
diagnostic code, and exists only to show what a surface could look like while staying inside the one
core. A mock becomes real only through an RFC and accepted rows (see `CLAUDE.md`).

## Status

| # | Surface | Seal | File | Status | Syntax source |
| --- | --- | --- | --- | --- | --- |
| 1 | Yan Core | 言核 | `.yir` | Defined | spec 3.5 (complete grammar) |
| 2 | Yan Agent | 言信 | `.yag` | Defined | spec 13.1, 13.2 (complete grammar), 14 |
| 3 | Yan (plain Yan) | 言 | `.yn` | Defined | spec 15.1, 15.2 (complete grammar), 15.3 |
| 4 | Yan Meta | 言元 | `.ymt` | Partial | spec 16.6 (rules only); full reference grammar in RFC 0001 and 0029 |
| 5 | Yan Docs | 言书 | `.ydoc` | Partial | doc-comment language in spec 18.9 (R310); no `.ydoc` file format |
| 6 | Yan Schema | 言契 | `.ysd` | Partial | spec 16.7 row R537; example and grammar sketch in RFC 0002 D2 |
| 7 | Yan Prompt | 言询 | `.ypm` | Partial | R538; example and grammar sketch in RFC 0002 D3 |
| 8 | Yan Query Language | 言查 | `.yql` | Partial | R539; example and stage list in RFC 0002 D4 |
| 9 | Yan Shell | 言行 | `.ynsh` | Partial | R540; example and grammar sketch in RFC 0002 D5 |
| 10 | Yan Workflow | 言序 | `.ywf` | Partial | R541; example and lowering in RFC 0002 D6, no grammar |
| 11 | Yan Infra | 言基 | `.yinf` | Partial | R542; example and lowering in RFC 0002 D7, no grammar |
| 12 | Yan Binary Format | 言码 | `.ybf` | Partial | R543; example and lowering in RFC 0002 D8, no grammar |
| 13 | Yan Dataflow | 言川 | `.ydf` | Partial | R544; example and lowering in RFC 0002 D9, no grammar |
| 14 | Yan Delegate | 言代 | `.ydl` | Partial | R545 (research); one example in RFC 0002 D10 |
| 15 | Yan Probabilistic | 言随 | `.ypb` | Partial | R546 (research); one example in RFC 0002 D11 |
| 16 | Yan Protocol | 言礼 | `.ypc` | Partial | R547 (research); one example in RFC 0002 D12 |
| 17 | Yan Spec | 言规 | `.ysp` | Partial | R548 (research); one example in RFC 0002 D13 |
| 18 | Yan Law | 言法 | `.ylaw` | Partial | R549 (research); one example in RFC 0002 D14 |

**Counts: 3 defined, 15 partial, 0 undefined.** Plain Yan is the brand surface (seal 言); the other
17 are the equal-rank surfaces on the seal grid.

Known spelling drift, left for a spec PR:

- Spec 16.7 row R540 still gives Yan Shell the extension `.yosh`. The name decided on 2026-11-04 is
  `.ynsh` (command `ynsh`), used here.
- Spec 18.9 R310 still names the fence tags `xin` and `wen` (pre-rename). This page uses `yag`, `yn`
  and `ymt`.

## How every surface relates to the core

All surfaces lower to the shared HIR and then to Yan Core (spec 1, R558; `docs/architecture.md`).
Yan Agent, Yan and Yan Meta add syntax only (law 1). The niche surfaces (6 to 18) may give their own
items semantics of their own, but every item another surface calls has a boundary signature in Yan
Core types, with `fail` and `uses` (16.7, R568).

The same five facts are marked in every surface (law 4):

| Fact | Yan Core | Yan Agent | Yan | Yan Meta |
| --- | --- | --- | --- | --- |
| failure | `fail T`, `?` | `fail E`, `?` | `fail E`, `?` | `! E`, `?` |
| capability use | `cap io: Io` | `uses io` | `uses io` | `\|- io` |
| mutation of a passed variable | `inout` | `var` param, `&x` | `var` param | `~x` |
| concurrency | explicit tasks | `par` | `par` | `\|\|`, `\|\|\|` |
| compile-time evaluation | `comptime` | stage-0 literals | stage-0 literals | `#e` |

**Shared shape of the mocks.** To stay consistent with the one core, every niche mock below follows
the same rules:

1. Lexis is Yan's (15.1): two-space indentation, `--` comments, `--- ` doc comments, snake_case
   values, PascalCase types, unit literals such as `14d` and `1ms`, `t"..."` templates.
2. Outside a surface's own item forms, statements, expressions, types and patterns are plain Yan
   (15.2). A niche file can hold ordinary Yan functions next to its own items.
3. Each surface adds one or two item keywords (`record`, `prompt`, `query`, `layout`, `node`...).
   Item headers always end in Yan's signature clauses: `-> T`, `fail E`, `uses caps`.
4. `ex` lines are allowed in every item body, as in Yan.

---

## 1. Yan Core (`.yir`) — Defined

The shared IR and its text form, used for golden tests and debugging. Administrative normal form:
every intermediate value is named and typed, every argument states its convention (`let`, `inout`,
`sink`), every capability is an explicit `cap` parameter, every operator is a named function, and
nothing is inferred. Blocks use braces; `--` starts a comment.

Grammar head (spec 3.5):

```
Module   ::= "module" Path {Item}
Item     ::= {Attr} (Fn | Gen | Record | Sum | Newtype | Trait | Impl | Const | Test | Extern | Inst)
Fn       ::= ["pub"] ["unsafe"] "fn" Path [TParams] "(" [Param {"," Param}] ")"
             "->" Type [From] ["fail" Type] {Contract} Block
Param    ::= ["comptime"] Conv Name ":" Type | "cap" Name ":" CapType
Conv     ::= "let" | "inout" | "sink"
Contract ::= "requires" Pred | "ensures" Pred | "decreases" Expr {"," Expr} | "example" Expr
```

Example (`conformance/yir/run/failure_propagation.yir`):

```
module conf.failure_propagation

record Bad { code: i64 }

fn check(let x: i64) -> i64 fail Bad {
    let neg: bool = lt_i64(let x, let 0)
    if neg {
        let e: Bad = Bad { code: x }
        fail e
    }
    ret x
}

fn twice(let x: i64) -> i64 fail Bad {
    let y: i64 = check(let x)?
    let z: i64 = add_checked_i64(let y, let y)
    ret z
}
```

Capabilities are ordinary parameters (`conformance/yir/run/hello.yir`):

```
fn main(cap io: Io) -> Unit {
    std.io.print(cap io, let "hello, world")
    ret ()
}
```

## 2. Yan Agent (`.yag`) — Defined

The surface language models write: shaped like typed Python 3.12, canonical, every signature
explicit. Four-space indentation, `#` comments, docstrings, `def` and `class`. A lenient reader
(14.4) accepts common Python deviations and normalizes them.

Key spellings (13.2):

- Signature: `def f(x: T, var acc: list[int], take buf: bytes) -> R fail E uses io, net:`
- Failure: `fail E` in the return type, `?` to propagate, `x else fail E(...)` as fallback.
- Contracts open a body: `pre`, `post` (with `out` for the result), `ex`, `decreases`, `cost`.
- Sums: `type PortErr = MissingQuote(sym: str) | Fetch(msg: str)`; records are `class` with fields.
- Concurrency: `par:` blocks, `par for`, `par [f(x)? for x in xs]`.
- Handlers: `with net = fake_net():`; `try`, `except`, `raise`, `async` are rejected with fixes.

Example (spec 14.5, shortened):

```
type PortErr = MissingQuote(sym: str) | Fetch(msg: str)

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
    ...
```

A larger program: `examples/tally/g5_tally.yag`.

## 3. Yan (`.yn`) — Defined

The surface people read and write. Two-space indentation, `--` comments, `--- ` doc comments, no
`def`: a function is `name(params) -> T = body`. It infers the return type of a single-expression
body, and `uses` and `fail` on non-`pub` items. Records are `{ ... }` types, `let` and `var` bind,
`match` arms use `->`, and a function body can be a list of clause arms. Strings interpolate with
`\(expr)`; units, dates and grids are literals (`2.5bp`, `2026-10-30`, `[0.6 0.4]`).

Example (spec 15.6, the same program as above):

```
--- Fetch quotes concurrently and value a portfolio.

type Position = { sym: str, qty: int }
type Quote = { sym: str, px: float }
type PortErr = MissingQuote(sym: str) | Fetch(msg: str)

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

Clause-form bodies (`examples/tally/g5_tally.yn`):

```
bps(mcc: Mcc) -> int uses none =
  Grocery() -> 150
  Travel() -> 250
  Other(_) -> 290
```

## 4. Yan Meta (`.ymt`) — Partial

The dense ASCII surface and the lab, kitchen sink and canary of the language. Spec 16.6 gives its
admission rules (K1 to K5), lifecycle and `@lab(...)` opt-in, but not its grammar. The full reference
(lexis, operator tokens, a 17-level precedence table, grammar, spelling notes, printing) is in
accepted RFC 0001 ("Reference"), extended by RFC 0029. It moves into the spec at M4; the print-only
view `yan view --as ymt` ships in M3. So the syntax is designed but not yet normative, and no mock is
needed for the grammar.

Base forms (RFC 0001 reference section 9):

| Yan Meta | Yan |
| --- | --- |
| `x := e`, `~x := e` | `let x = e`, `var x = e` |
| `x <- e` | `x = e` |
| `a = b` | `a == b` |
| `-> T ! E`, `\|- io, net` | `-> T fail E`, `uses io, net` |
| `x ?? y`, `!! e`, `x?!` | `x else y`, `fail e`, `try x` |
| `\x -> e`, `_ * 2`, `f >>> g` | closures, composition |
| `+/ xs`, `+/ p in ps => e` | `sum`, `sum_by` |
| `\|\| x in xs`, `\|\|[e \| x in xs]` | `par for`, collecting `par for` |
| `?{ }` `!{ }` `={ }` `@{ }` `-{ }` | `pre` `post` `ex` `invariant` `decreases` |
| `.{a: 1}`, `{K => V}`, `[T]`, `T?` | record, map, list type, option type |
| `e \|?` then `\| P => e` arms | `match` |

**MOCK example** (the portfolio program, written by hand from the forms above; no Yan Meta reader
exists, so it is unchecked):

```
--- Value a portfolio.
type Position := {sym: str, qty: int}
type Quote := {sym: str, px: float}
type PortErr := MissingQuote(sym: str) | Fetch(msg: str)

pub value(ps: [Position], qs: {str => Quote}) -> float ! PortErr :=
  ?{ ps.len() > 0 }
  ={ value([.{sym: "A", qty: 2}], {"A" => .{sym: "A", px: 10.0}}) = Ok(20.0) }
  +/ p in ps => float(p.qty) * (qs.get(p.sym) ?? !! MissingQuote(sym: copy p.sym)).px

pub main() ! PortErr |- io, net :=
  ps := [.{sym: "SPX", qty: 10}, .{sym: "NDX", qty: 5}]
  quotes := ||[fetch_quote(p.sym)? | p in ps]
  io.print("portfolio value: {value(ps, quotes.index_by(.sym))?}")
```

## 5. Yan Docs (`.ydoc`) — Partial

What is defined: the documentation language inside doc comments and guide pages (18.9, R310 to
R316). It is CommonMark plus GFM tables with three additions: item links (`[Reader]`,
`[std.io.Reader]`, `[text][path]`) resolved by name and pinned to item hashes; tagged sections
(`Fails:`, `Uses:`, `Panics:`, `Safety:`, `See also:`); and fence tags naming a surface, where a
trailing `!` pins it. `yan test` runs every fence. What is not defined: a `.ydoc` file. Guide pages
are currently `docs/*.md` (R316).

**MOCK.** A `.ydoc` file is a guide page whose fences are checked code. It is the R316 guide page
with a fixed extension and a header fence:

```
doc        = header {block}
header     = "```yan-doc" NEWLINE {hline} "```" NEWLINE
hline      = ("title" STRING | "uses" capitem {"," capitem} | "surface" EXT | "order" INT) NEWLINE
block      = commonmark-block | fence | tagged
fence      = "```" [EXT ["!"]] NEWLINE code "```"       -- EXT is yn, yag, ymt, yql ...; no tag = header surface
tagged     = ("Fails:" | "Uses:" | "Panics:" | "Safety:" | "See also:") NEWLINE {commonmark-block}
```

```
```yan-doc
title "Reading files"
uses fs, io
surface yn
```

# Reading files

Open a rooted handle with [std.fs.root] and read lines with [std.fs.lines].

```
let logs = fs.root("./logs")
for line in fs.lines(logs / "app.log")?
  io.print(line)
```

Uses:
- `fs` to read under `./logs`; nothing outside it is reachable.
```

Lowering: none for prose. Each fence lowers as a test in the page's scope under the header's `uses`
(R315); item links resolve to item hashes (R311).

---

The niche surfaces (6 to 18) are the "Yan &lt;job>" family of spec 16.7. None is built before 1.0
(R550); each ships first as a print-only view, `yan view --as <ext>`. Each section quotes the RFC 0002
example, then gives a **MOCK** grammar for the surface's own forms (everything else is plain Yan, per
the shared rules above) and a second **MOCK** example.

## 6. Yan Schema (`.ysd`) — Partial

Wire types and remote interfaces with version history. Lowers to `derive(Wire)` types, refinements
and `@remote` traits.

RFC 0002 D2:

```
package acme.orders

record Order since v1
  id: OrderId
  sym: str
  side: Side
  qty: i32 where qty > 0
  limit: Money?
  tif: Tif = Tif.day since v2
  account: AccountId was owner since v3

enum Side
  buy
  sell

service Book
  place(o: Order) -> Ack fail Rejected
  cancel(id: OrderId) -> Ack fail NotFound
```

**MOCK grammar** (extends the RFC sketch):

```
module   = "package" path NEWLINE {use} {item}
item     = {attr} (record | enum | union | service | alias)
record   = "record" TYPENAME [tparams] [since] NEWLINE INDENT field {field} DEDENT
field    = {attr} NAME ":" type ["where" expr] ["=" const] ["was" NAME] [since] [removed]
enum     = "enum" TYPENAME [since] NEWLINE INDENT {NAME [since] NEWLINE} DEDENT
union    = "union" TYPENAME [since] NEWLINE INDENT {TYPENAME ["(" fields ")"] [since] NEWLINE} DEDENT
service  = "service" TYPENAME [since] NEWLINE INDENT method {method} DEDENT
method   = NAME "(" [params] ")" ["->" ["stream"] type] ["fail" type] [since] NEWLINE
alias    = "type" TYPENAME "=" type ["where" expr] NEWLINE
since    = "since" VERSION
removed  = "removed" VERSION
```

**MOCK example:**

```
package acme.billing

type Cents = i64 where self >= 0

union Payment since v1
  Card(token: str, last4: str)
  Bank(iban: str)
  Voucher(code: str) since v2

record Invoice since v1
  id: InvoiceId
  lines: List[Line]
  total: Cents
  memo: str = "" since v2
  @proto(7) due: PlainDate

service Billing
  issue(i: Invoice) -> InvoiceId fail Invalid
  watch(id: InvoiceId) -> stream Status fail NotFound since v3
```

## 7. Yan Prompt (`.ypm`) — Partial

Typed model programs: prompt, schema, validation and examples in one item. Lowers to
`model.generate_chat` and `validate` (17.4).

RFC 0002 D3:

```
type Triage = { severity: Sev, team: Team, summary: str where len(summary) <= 200 }

prompt triage(t: Ticket) -> Triage fail ModelError uses model =
  model "fast"
  system "You route support tickets. Reply only in the schema."
  user t"""
    Subject: {t.subject}
    Body: {t.body}
    """
  check r -> r.summary != ""
  ex triage(fixtures.outage) == Ok(fixtures.outage_triage)
     with model = fake_model(fixtures.outage_reply)
```

**MOCK grammar** (adds few-shot and tool sections to the RFC sketch):

```
prompt_item = "prompt" NAME [tparams] "(" [params] ")" "->" type ["fail" type] "uses" caps "="
              NEWLINE INDENT [model_line] {setting} role {role} [check] {ex} DEDENT
model_line  = "model" STRING NEWLINE
setting     = ("temperature" | "max_tokens" | "seed") expr NEWLINE
role        = ("system" | "user" | "assistant") (STRING | TSTRING) NEWLINE
            | "shots" expr NEWLINE                      -- list of (input, output) pairs
            | "tools" path {"," path} NEWLINE           -- functions the model may call
check       = "check" NAME "->" expr NEWLINE
ex          = "ex" expr ["with" NAME "=" expr {"," NAME "=" expr}] NEWLINE
```

**MOCK example:**

```
type Extract = { vendor: str, total: Money, due: PlainDate? }

prompt extract(doc: str) -> Extract fail ModelError uses model =
  model "small"
  temperature 0.0
  system "Extract invoice fields. Use null when a field is absent."
  shots fixtures.invoice_pairs
  user t"{doc}"
  check r -> r.total >= 0usd
  ex extract(fixtures.acme_pdf_text) == Ok(fixtures.acme_extract)
     with model = fake_model(fixtures.acme_reply)
```

## 8. Yan Query Language (`.yql`) — Partial

Typed, compiled, deterministic table pipelines in PRQL order. Lowers to `LazyFrame[R]` plans (11.9).

RFC 0002 D4:

```
query movers(trades: Table[Trade], since: PlainDate) -> Table[Mover] =
  from trades
  where day >= since and qty > 0
  derive notional = px * qty
  group sector, sym (notional = sum(notional), vwap = sum(px * qty) / sum(qty))
  sort sector, -notional
  take 5 per sector
  ex movers(fixtures.trades, 2026-10-01).len() == 15
```

**MOCK grammar** (the RFC lists the stages; this fixes their shape):

```
query_item = "query" NAME [tparams] "(" [params] ")" "->" type ["fail" type] "="
             NEWLINE INDENT source {stage} {ex} DEDENT
source     = "from" expr ["as" NAME] NEWLINE
stage      = "where" expr
           | "derive" assign {"," assign}
           | "select" (NAME | assign) {"," (NAME | assign)}
           | "group" keys "(" assign {"," assign} ")"
           | ("join" | "left join") expr ["as" NAME] "on" expr
           | "sort" ["-"] expr {"," ["-"] expr}
           | "take" expr ["per" keys]
           | "window" keys "sort" expr "(" assign {"," assign} ")"
assign     = NAME "=" expr
keys       = NAME {"," NAME}
```

Bare names resolve against the row type at that stage; `t.col` names a column of a joined input.

**MOCK example:**

```
query late_orders(orders: Table[Order], users: Table[User], today: PlainDate) -> Table[Late] =
  from orders
  where status == Status.open and due < today
  join users as u on user_id == u.id
  derive days_late = (today - due).days()
  select id, u.email, days_late
  sort -days_late
  ex late_orders(fixtures.orders, fixtures.users, 2026-11-04).len() == 3
```

## 9. Yan Shell (`.ynsh`) — Partial

Capability-safe shell scripts: authority stated in the header, bare words never run programs, and
interpolations are single argv entries. Lowers to line iterators, rooted handles and argv-only
`proc`. The spec row still reads `.yosh`; see the drift note above.

RFC 0002 D5:

```
#!/usr/bin/env -S yan run
main(args: Args) uses proc, fs, net =
  let logs = fs.root("./logs")
  let out = fs.root("./out", write)
  let backup = net.only(connect = ["backup.example.com:22"])
  for f in ls(logs, "*.log")
    if f.age > 1d
      lines(f) | grep("ERROR") | sort | uniq(count) > out / "\(f.stem).errors"
      gzip(f)?
  run t"rsync -a {out.path} backup.example.com:/srv/errors/" with net = backup ?
```

**MOCK grammar** (extends the RFC sketch):

```
script    = [shebang] {item}                       -- items are Yan functions; main is the entry
stmt      = pipeline NEWLINE | yan-stmt
pipeline  = stage {"|" stage} [redirect] ["?"]
stage     = builtin ["(" [args] ")"] | "run" TSTRING ["with" NAME "=" expr] | expr
redirect  = (">" | ">>") expr | "2>" expr
builtin   = "lines" | "grep" | "sort" | "uniq" | "head" | "tail" | "cut" | "wc" | "tee" | "ls"
          | "gzip" | "cat"
```

`run` is the only way to start a process. A non-zero exit is a typed `ProcError`.

**MOCK example:**

```
#!/usr/bin/env -S ynsh
--- Fail CI when the build log has new warnings.
main(args: Args) fail ProcError uses proc, fs, io =
  let ws = fs.root(".")
  run t"cargo build --release" > ws / "build.log" ?
  let n = lines(ws / "build.log") | grep("warning:") | wc
  if n > args.int("max", 0)
    io.print("\(n) warnings")
    fail ProcError.exit(1)
```

## 10. Yan Workflow (`.ywf`) — Partial

Durable workflows that survive crashes and deploys. Lowers to `@durable` functions, journaled steps
and durable timers. The RFC gives an example and lowering but no grammar.

RFC 0002 D6:

```
workflow onboard(user: UserId) -> Done fail OnboardError
    uses crm, billing, mail, timer =
  step account = crm.create_account(user)
  step billing.start_trial(account, 14d)
  par
    step mail.send(t"Welcome, {user}")
    step crm.schedule_call(account)
  wait 13d
  step mail.send(t"Your trial ends tomorrow")
  wait until billing.converted(account) or 2d
  Done
```

**MOCK grammar:**

```
workflow  = "workflow" NAME "(" [params] ")" "->" type ["fail" type] "uses" caps "="
            NEWLINE INDENT {wstmt} DEDENT
wstmt     = "step" [NAME "="] expr ["retry" expr] NEWLINE   -- one journaled capability call
          | "par" NEWLINE INDENT wstmt {wstmt} DEDENT
          | "wait" expr NEWLINE                             -- a Duration or an Instant
          | "wait" "until" expr "or" expr NEWLINE
          | "signal" NAME ":" type NEWLINE                  -- external input, journaled
          | "version" STRING NEWLINE INDENT {wstmt} DEDENT  -- code added in a deploy
          | "compensate" NEWLINE INDENT wstmt {wstmt} DEDENT
          | yan-stmt                                        -- pure code only
```

**MOCK example:**

```
workflow refund(order: OrderId) -> Refunded fail RefundError uses shop, pay, mail =
  step o = shop.order(order)
  signal approved: bool
  if not approved
    fail RefundError.denied
  step pay.refund(o.payment, o.total) retry 5
  compensate
    step shop.reopen(order)
  version "2026-11-mail"
    step mail.send(t"Refund for {order} sent")
  Refunded(at: o.total)
```

## 11. Yan Infra (`.yinf`) — Partial

Cloud resources as a typed program with plan and apply. Lowers to resource capabilities whose
results are `Deferred[T]`. Example and lowering only in the RFC.

RFC 0002 D7:

```
stack web(env: Env) -> Stack fail CloudError uses cloud =
  let net = cloud.vpc(cidr: "10.0.0.0/16")
  let db = cloud.postgres(net, size: Db.small, backups: 7d)
  let app = cloud.service(net,
    image: "ghcr.io/acme/web:1.4",
    replicas: if env == Env.prod then 3 else 1,
    env: {"DB_URL": db.url},
  )
  cloud.dns("acme.example", app.lb)
```

**MOCK grammar:**

```
stack     = "stack" NAME "(" [params] ")" "->" type ["fail" type] "uses" caps "="
            NEWLINE INDENT {istmt} DEDENT
istmt     = "let" NAME "=" expr NEWLINE                    -- a resource call returns a handle
          | "for" NAME "in" expr "keyed" expr NEWLINE INDENT {istmt} DEDENT   -- stable keys
          | "output" NAME "=" expr NEWLINE
          | "import" NAME "=" expr NEWLINE                 -- adopt an existing resource by id
          | yan-stmt                                       -- no branching on Deferred values
```

**MOCK example:**

```
stack workers(env: Env, regions: List[Region]) -> Stack fail CloudError uses cloud =
  import net = cloud.vpc_by_id("vpc-0a1b")
  for r in regions keyed r.code
    let q = cloud.queue(net, region: r, retention: 4d)
    let w = cloud.service(net, image: "ghcr.io/acme/worker:2.0", env: {"QUEUE": q.url})
  output dashboard = cloud.dashboard(net, title: "workers \(env)")
```

## 12. Yan Binary Format (`.ybf`) — Partial

Packet, file and register layouts with checked zero-copy parsers and writers. Lowers to view-backed
records over `View[u8]`. Example and lowering only in the RFC.

RFC 0002 D8:

```
layout Ipv4Header big_endian msb_first
  version: u4 = 4
  ihl: u4 where ihl >= 5
  dscp: u6
  ecn: u2
  total_len: u16 where total_len >= ihl * 4
  id: u16
  flags: u3
  frag_off: u13
  ttl: u8
  proto: IpProto as u8
  checksum: u16 = ones_complement(header)
  src: Ipv4
  dst: Ipv4
  options: bytes[(ihl - 5) * 4]
```

**MOCK grammar** (adds tagged unions, which the RFC lists as later):

```
item      = layout | variant_layout | yan-item
layout    = "layout" TYPENAME [order] [bitorder] NEWLINE INDENT lfield {lfield} DEDENT
order     = "big_endian" | "little_endian"
bitorder  = "msb_first" | "lsb_first"
lfield    = (NAME | "_") ":" ltype ["as" type] ["where" expr] ["=" expr] NEWLINE
ltype     = "u" INT | "i" INT | "bytes" "[" expr "]" | "bytes" "[" ".." "]"   -- ".." = rest
          | TYPENAME | ltype "[" expr "]"                                  -- array of a layout
variant_layout = "layout" TYPENAME "on" NAME ":" ltype NEWLINE INDENT
            {INT "=>" TYPENAME NEWLINE} DEDENT
```

`= expr` on a field is fixed (checked on parse) or computed (filled on write); `_` is padding.

**MOCK example:**

```
layout PngChunk big_endian
  len: u32
  kind: bytes[4]
  data: bytes[len]
  crc: u32 = crc32(kind, data)

layout Packet on tag: u8 little_endian
  1 => Ping
  2 => Data

layout Data little_endian
  seq: u16
  _: u16
  payload: bytes[..]
```

## 13. Yan Dataflow (`.ydf`) — Partial

Synchronous dataflow in the Lustre style, on the embedded profile. A node lowers to a state record and
a step function. Example and lowering only in the RFC.

RFC 0002 D9:

```
const dt: f32 = 0.001

node pid(err: f32, g: Gains) -> f32 =
  let i = 0.0 -> clamp(last(i) + err * dt, -g.windup, g.windup)
  let d = 0.0 -> (err - last(err)) / dt
  g.kp * err + g.ki * i + g.kd * d

@task(priority = 2)
pub node attitude(target: Angles, imu: Imu, g: Gains3) -> Motors every 1ms =
  let e = target - imu.angles
  mix(pid(e.roll, g.roll), pid(e.pitch, g.pitch), pid(e.yaw, g.yaw))
```

**MOCK grammar:**

```
item      = const | {attr} ["pub"] node | yan-item
const     = "const" NAME ":" type "=" expr NEWLINE
node      = "node" NAME "(" [params] ")" "->" type ["every" expr] "="
            (dexpr NEWLINE | NEWLINE INDENT {dbind} dexpr NEWLINE DEDENT)
dbind     = "let" NAME [":" type] "=" dexpr NEWLINE
dexpr     = dexpr "->" dexpr                 -- first tick left, later ticks right
          | "last" "(" NAME ")"              -- previous tick's value
          | "automaton" NEWLINE INDENT state {state} DEDENT
          | yan-expr                         -- pure, no allocation
state     = "state" TYPENAME NEWLINE INDENT {dbind} "out" dexpr NEWLINE
            {"until" expr "then" TYPENAME NEWLINE} DEDENT
```

**MOCK example:**

```
node debounce(raw: bool, n: u8) -> bool =
  let count = 0 -> if raw == last(raw) then min(last(count) + 1, n) else 0
  false -> if count == n then raw else last(out)

pub node door(open_btn: bool, closed_sw: bool) -> Motor every 10ms =
  automaton
    state Idle
      out Motor.stop
      until debounce(open_btn, 5) then Opening
    state Opening
      out Motor.up
      until not closed_sw then Idle
```

## 14. Yan Delegate (`.ydl`) — Partial (research)

Contracts whose bodies are generated by the agent loop and pinned by hash. `yan build` never calls a
model. One example in the RFC.

RFC 0002 D10:

```
dream dedupe_emails(xs: List[str]) -> List[str] =
  post len(out) <= len(xs)
  post all i in 0..len(out): all j in 0..len(out): i == j or lower(out[i]) != lower(out[j])
  ex dedupe_emails(["A@x.io", "a@x.io", "b@x.io"]) == ["A@x.io", "b@x.io"]
  ex dedupe_emails([]) == []
  body 3f9a...c21
```

**MOCK grammar:**

```
dream     = "dream" NAME [tparams] "(" [params] ")" ["->" type] ["fail" type] ["uses" caps] "="
            NEWLINE INDENT {contract} {hint} ["body" HASH] DEDENT
contract  = ("pre" | "post" | "ex" | "cost") expr NEWLINE      -- as Yan
hint      = "prefer" STRING NEWLINE                             -- prose for the generator only
          | "reuse" path {"," path} NEWLINE                     -- items the body may call
```

**MOCK example:**

```
dream slugify(title: str) -> str uses none =
  post out.all(\c -> c.is_ascii_lowercase() or c.is_ascii_digit() or c == '-')
  post not out.starts_with("-") and not out.ends_with("-")
  ex slugify("Hello, World!") == "hello-world"
  ex slugify("  a  b ") == "a-b"
  prefer "single pass, no regex"
  reuse std.text.to_ascii
  body 7c01...e9b
```

## 15. Yan Probabilistic (`.ypb`) — Partial (research)

Probabilistic models where `sample` and `observe` are capability operations and inference algorithms
are handlers. One example in the RFC.

RFC 0002 D11:

```
model coin_bias(flips: List[bool]) -> f64 uses prob =
  let p = sample beta(1.0, 1.0)
  for f in flips
    observe bernoulli(p) == f
  p
```

**MOCK grammar:**

```
model     = "model" NAME [tparams] "(" [params] ")" "->" type "uses" caps "="
            NEWLINE INDENT {pstmt} expr NEWLINE DEDENT
pstmt     = "let" pattern "=" "sample" expr ["as" STRING] NEWLINE   -- name for the trace
          | "observe" expr "==" expr NEWLINE
          | "factor" expr NEWLINE                                   -- add log weight
          | yan-stmt
```

Inference stays a call: `infer(model(args), algorithm(options))`.

**MOCK example:**

```
model linreg(xs: List[f64], ys: List[f64]) -> (f64, f64) uses prob =
  let a = sample normal(0.0, 10.0) as "intercept"
  let b = sample normal(0.0, 10.0) as "slope"
  let s = sample half_normal(1.0)
  for (x, y) in xs.zip(ys)
    observe normal(a + b * x, s) == y
  (a, b)
```

## 16. Yan Protocol (`.ypc`) — Partial (research)

Session-typed protocols: each role and step is a `@linear` state type, so skipping or reordering a
message is a compile error. One example in the RFC.

RFC 0002 D12:

```
protocol Checkout between Client, Shop
  Client -> Shop: Start(cart: Cart)
  Shop -> Client: Quote(total: Money)
  choice by Client
    accept:
      Client -> Shop: Pay(token: Token)
      Shop -> Client: Receipt or Declined
    cancel:
      Client -> Shop: Cancel
```

**MOCK grammar** (adds recursion, which the RFC lists as open):

```
protocol  = "protocol" TYPENAME "between" TYPENAME {"," TYPENAME} NEWLINE INDENT {pstep} DEDENT
pstep     = TYPENAME "->" TYPENAME ":" msg {"or" msg} NEWLINE
          | "choice" "by" TYPENAME NEWLINE INDENT branch {branch} DEDENT
          | "rec" NAME NEWLINE INDENT {pstep} DEDENT
          | "again" NAME NEWLINE
branch    = NAME ":" NEWLINE INDENT {pstep} DEDENT
msg       = TYPENAME ["(" [field {"," field}] ")"]
```

**MOCK example:**

```
protocol Download between Client, Server
  Client -> Server: Open(path: str)
  rec chunks
    choice by Server
      more:
        Server -> Client: Chunk(data: Bytes)
        again chunks
      done:
        Server -> Client: Eof(sha256: Digest)
```

## 17. Yan Spec (`.ysp`) — Partial (research)

Specs in the TLA+ style, explored with `std.sim` and checked against actors by trace refinement. One
example in the RFC.

RFC 0002 D13:

```
spec Lease
  var holder: Node? = None
  var expires: Instant = Instant.min
  var now: Instant = Instant.min

  action acquire(n: Node) when holder == None or now >= expires
    holder <- n
    expires <- now + 10s
  action tick(d: Duration) when d > 0s
    now <- now + d

  always one_holder: holder == None or now < expires + 1s

refines actor LeaseServer
  holder = state.owner
  expires = state.deadline
```

**MOCK grammar:**

```
item      = spec | refines | yan-item
spec      = "spec" TYPENAME [tparams] NEWLINE INDENT {svar} {action} {prop} DEDENT
svar      = "var" NAME ":" type "=" expr NEWLINE
action    = "action" NAME "(" [params] ")" ["when" expr] NEWLINE INDENT {assign} DEDENT
assign    = NAME "<-" expr NEWLINE
prop      = ("always" | "eventually_within" expr) NAME ":" expr NEWLINE
refines   = "refines" "actor" TYPENAME NEWLINE INDENT {NAME "=" expr NEWLINE}
            {"on" TYPENAME "=>" NAME "(" [args] ")" NEWLINE} DEDENT   -- message to action
```

`<-` inside an action reads as Yan Meta's assignment, chosen to set spec state apart from program
code.

**MOCK example:**

```
spec Counter
  var n: int = 0
  var max: int = 3
  action inc() when n < max
    n <- n + 1
  action reset()
    n <- 0
  always bounded: n >= 0 and n <= max

refines actor CounterActor
  n = state.count
  on Increment => inc()
  on Reset => reset()
```

## 18. Yan Law (`.ylaw`) — Partial (research)

Statute-shaped rules with default logic, in the Catala style. Lowers to `uses none` functions over
`Money`. One example in the RFC.

RFC 0002 D14:

```
scope IncomeTax
  input income: Money
  input dependents: int
  output tax: Money

  definition allowance equals 12000 EUR plus 2000 EUR times dependents
  definition taxable equals max(income minus allowance, 0 EUR)

  rule tax equals taxable times 20pct
  exception when taxable is more than 50000 EUR
    rule tax equals 10000 EUR plus (taxable minus 50000 EUR) times 40pct
```

**MOCK grammar:**

```
scope     = "scope" TYPENAME NEWLINE INDENT {decl} {rulepart} DEDENT
decl      = ("input" | "output") NAME ":" type NEWLINE
          | "source" STRING NEWLINE                     -- statute citation, kept on outputs
rulepart  = "definition" NAME "equals" lexpr NEWLINE
          | "rule" NAME "equals" lexpr ["rounded" mode "to" lexpr] NEWLINE
          | "exception" ["to" NAME] "when" lcond NEWLINE INDENT rulepart {rulepart} DEDENT
lexpr     = yan-expr with word operators: "plus" "minus" "times" "divided by"
lcond     = lexpr ("is more than" | "is less than" | "is at least" | "is at most" | "is") lexpr
          | lcond ("and" | "or") lcond
mode      = "half_even" | "half_up" | "down" | "up"
```

**MOCK example:**

```
scope ChildBenefit
  source "Example Act 2026, s. 4"
  input children: int
  input income: Money
  output benefit: Money

  definition base equals 250 EUR times children
  rule benefit equals base
  exception when income is more than 80000 EUR
    rule benefit equals base times 50pct rounded half_even to 1 EUR
  exception when children is 0
    rule benefit equals 0 EUR
```
