# Yan Schema 言契 (`.ysd`): syntax study

Status: brainstorm study, 2026-11-04. Not a spec change. No rows, no diagnostic codes. Every syntax
block below is a mock unless it is quoted from RFC 0002 D2.

**One line.** A Yan Schema file should read like plain Yan's own `type` declarations, carry its version
history as short trailing marks printed from the pinned history, stay closed and tag-free by default,
and export the same declarations to protobuf, OpenAPI, JSON Schema and model-output grammars.

## 1. Methodology and process

1. **Read the current definition narrowly.** `docs/surfaces.md` section 6 (RFC sketch and the mock
   grammar), RFC 0002 D1 (admission rules N1 to N9) and D2 (Yan Schema), spec 16.7 row R537, spec 9.6.1
   (R405 encoding, R406 schema evolution, R407 `@remote`), the `Wire` decoding paragraph in spec 17,
   the compatibility table in spec 20, and R555 (streams over `Remote`) and R568 (niche surfaces may own
   semantics). Plain Yan's syntax came from `docs/surfaces.md` section 3. Nothing else in `spec/` was read.
2. **Surveyed peers.** 20 schema and interface languages, grouped by job (wire IDLs, API description
   languages, data-validation languages, LLM output schemas, migration systems). Recent facts were checked
   by web search this session (protobuf editions, TypeSpec 1.0, Typical, Cambria, panproto, Stripe
   versioning, BAML). Older facts come from prior knowledge and are marked with a confidence level in
   section 10.
3. **Read the research.** Schema evolution (Avro resolution, PRISM schema modification operators,
   bidirectional and edit lenses, Cambria), protocol robustness (RFC 9413), asymmetric field presence
   (Typical, and a draft essay generalizing it), refinement types, and grammar-constrained decoding.
4. **Fixed five tasks** (section 4) that every proposal must express, chosen to hit each hard part:
   refinements and defaults, sums, three versions of history, a service with failures and a stream, and
   generics with a cross-field rule.
5. **Wrote five proposals**, each pushed to the extreme of one axis: brevity, evolution, safety, power,
   agent and tool cost. Each is written as if it were the only goal, to see what that axis buys and
   what it costs.
6. **Measured.** The five tasks were also written in protobuf (editions 2023 with protovalidate),
   TypeSpec 1.0 (with `@typespec/versioning`) and the RFC 0002 sketch. Each version was counted in
   characters without whitespace and in lexemes (identifiers, numbers, single punctuation). A real
   tokenizer was not reachable from this container (the proxy blocks the tokenizer downloads), so
   lexemes stand in for tokens. The figures are a proxy and are labelled as such.
7. **Mixed.** Kept what each proposal did best, checked each kept feature against N1 to N9, and sorted
   features into "lowers to existing core", "needs a row" and "declined".

## 2. What `.ysd` is today

- **Job (RFC 0002 D2).** Declare wire types and remote interfaces once, with their version history.
  Users are API and platform teams on protobuf, OpenAPI or JSON Schema.
- **Subset.** Record and sum types deriving `Wire`, refinement fields, and `@remote` traits whose
  methods have `Wire` parameters, results and failures. No bodies.
- **Identity.** No field tags. A `Wire` type's identity is its structural hash (R406). Equal hashes
  decode directly; unequal hashes go through a converter generated at stage 0 from a version pinned in
  `.yan/schemas/`. With no converter, the call fails `SchemaMismatch` before user code runs.
- **Decoding is already strict** (spec 17). Only statically known types are produced, sum tags never
  select an arbitrary type, sizes and depths are bounded, and unknown fields and duplicate keys are
  rejected unless the type opts in. Capability values are never `Wire`.
- **Recovery (N3).** Version annotations are printed back from the pinned history, so printing needs
  `.yan/schemas/` beside the source.
- **Five facts (N5).** Only failure occurs (in each method signature). A stream result adds
  concurrency.
- **Semantics.** R568 lets a niche surface own semantics behind a boundary signature. This study
  avoids that except where marked: every feature in the mix lowers to existing core or to a row that
  would be needed anyway.

The RFC sketch is already good. It has no tags, it states history inline (`since`, `was`), refinements
use `where`, and failures sit in method signatures. Its gaps are that it does not say how removals,
type changes, open types, generics, cross-field rules, documentation or exports are written, and
that it uses `record`, `enum` and `union` where plain Yan uses one `type` form.

## 3. Findings

### 3.1 Landscape

| Family | Tool | Identity of a field | Evolution model | What people praise | What people curse |
| --- | --- | --- | --- | --- | --- |
| Wire IDL | Protobuf (proto3, editions 2023/2024) | integer tag | additive; tags never reused; `reserved` | speed, codegen in every language, `buf breaking` | tag bookkeeping, `reserved` graveyards, zero-value ambiguity (fixed late by editions' `field_presence`), no validation in core |
| Wire IDL | gRPC | method name | as protobuf | streaming, deadlines, status codes | HTTP/2 only on the web, opaque errors |
| Wire IDL | Cap'n Proto | ordinal `@n` | additive; groups; unions | zero-copy, interfaces as capabilities, promise pipelining | small ecosystem, ordinals by hand |
| Wire IDL | FlatBuffers | field order or `id` | append-only | zero-copy reads | awkward builders, append-only discipline |
| Wire IDL | Thrift, Bond | integer id | additive | multi-language RPC | fragmented ecosystems |
| Wire IDL | Avro | field name + schema | writer schema and reader schema resolved at read time; defaults; aliases | no tags; real schema resolution; schema registries | needs the writer schema at read time; JSON-in-JSON schema syntax |
| Wire IDL | Typical | integer index | `optional`, required and `asymmetric` fields | asymmetric fields make required-field changes safe in two deploys | small, single author |
| API DL | OpenAPI 3.1 + JSON Schema 2020-12 | JSON name | none built in (path versioning) | universal tooling, docs, SDK generators | verbose YAML, weak sums (`oneOf`), validation and type mixed, no history |
| API DL | TypeSpec 1.0 (Microsoft) | name | `@added`, `@removed`, `@renamedFrom`, `@typeChangedFrom` per version enum | TypeScript-like brevity, templates, emits OpenAPI, JSON Schema, protobuf | decorator noise, history scattered across decorators, HTTP shape mixed with data shape |
| API DL | Smithy 2 (AWS) | name | traits, resources with lifecycle operations | resources, mixins, protocol-agnostic, linting | heavy model, Java-centric tooling |
| API DL | GraphQL SDL | name | `@deprecated`, never remove | one evolving graph, nullability on every edge | no versions; deprecation lives forever |
| API DL | AsyncAPI | name | as OpenAPI | event and channel description | YAML; little type power |
| Data | JSON Type Definition (RFC 8927) | JSON name | none | tiny, codegen-friendly, discriminated unions | too small for real constraints |
| Data | CDDL (RFC 8610) | name | none | very compact grammar-like notation for CBOR and JSON | unfamiliar to most developers |
| Data | CUE, Pkl, Dhall | name | unification or functions | types and constraints are one lattice (CUE); real config power | evaluation semantics; hard to predict; not a wire IDL |
| Data | Zod, Valibot, ArkType, Effect Schema | name | code | in-language, inference, ArkType's string DSL, Effect's bidirectional encode/decode | tied to TypeScript; not a source of truth for other languages |
| LLM | BAML, JSON Schema subsets for structured output | name | none | prompts typed by schemas; field descriptions guide the model; schema-aligned parsing | schema dialects differ per vendor |
| Migration | Stripe API versions | date | per-version "version change" modules transform responses back to the caller's pinned version | callers never break; dated versions | internal; costs a module per change |
| Migration | Cambria (Ink & Switch), panproto | name | lenses between schema versions, both directions | old and new peers interoperate; history is data | research grade |

### 3.2 What users want

From issue trackers, design docs and the tools that won (confidence medium: pattern across sources, not a
survey):

1. **One source of truth** that generates types, validators, docs and clients in many languages. The
   reason OpenAPI and protobuf persist despite their syntax.
2. **Breaking-change detection in CI** that is automatic and explains itself (`buf breaking`, Confluent
   compatibility modes, `yan schema check`). Users want the check to name the field and the reason.
3. **Validation next to the type.** Protovalidate (CEL), JSON Schema keywords and Zod all exist because
   the type alone is not the contract.
4. **Real sum types** with clean discriminators. `oneOf` and `oneof` are the most complained-about parts
   of their languages.
5. **Explicit presence.** "Was it absent or zero?" is the oldest protobuf complaint; editions added
   explicit presence in 2023.
6. **Readable diffs and short files.** TypeSpec's adoption over hand-written OpenAPI is mostly about
   size relative to the OpenAPI it emits (confidence medium).
7. **Docs as part of the schema.** Field descriptions flow to OpenAPI, SDK doc comments and, now, to
   models that fill the schema.

### 3.3 What works and what does not

- **Works: no tags, if identity is structural and history is recorded.** Avro proves name-based
  evolution works when readers have the writer's schema. Yan already pins schemas and generates
  converters, which is the same idea with the resolution done at compile time.
- **Fails: required fields as a permanent state.** Protobuf removed `required` in proto3 because adding or
  removing one breaks mixed fleets. Typical's fix is the middle state `asymmetric`: required for writers,
  optional for readers. A required field then changes in two deploys without a break (confidence high,
  verified on the Typical README).
- **Fails: history as decorators scattered over the current shape.** TypeSpec's `@added(v2)` and
  `@renamedFrom(v3, "owner")` work but grow without bound and dominate the file. GraphQL's "never remove"
  is the same failure in another form.
- **Works: history as operators.** PRISM's schema modification operators (Curino et al., 2008) and
  Stripe's version changes both model evolution as a sequence of small named operations, each with a
  known data mapping. That makes converters derivable and reviewable.
- **Works: lenses for both directions.** Cambria and panproto show that a change is safe for mixed
  fleets only if it has both a forward and a backward mapping, and that the backward mapping may need
  a complement (data the forward mapping dropped).
- **Fails: liberal acceptance.** RFC 9413 (2023) argues that Postel's "be liberal in what you accept"
  causes protocol decay. Yan's closed-by-default `Wire` decoding is on the right side of this.
- **Fails: HTTP shape mixed into the data schema.** OpenAPI and TypeSpec spend much of their syntax on
  paths, verbs and status codes. Yan services call by signature hash over `Remote` (R407), so `.ysd` can
  leave HTTP to an export mapping.
- **Works: compact notation for experts.** CDDL and ArkType show that a grammar-like notation can be
  very short. But both are read mostly by their authors; the one-off reader pays.

### 3.4 Research

- **Avro schema resolution** (Apache Avro specification): writer and reader schemas are matched by
  field name; missing fields take reader defaults; aliases rename. The model behind R406.
- **Schema modification operators** (Curino, Moon, Zaniolo, "Graceful database schema evolution: the
  PRISM workbench", VLDB 2008): a small set of operators (add, drop, rename, copy, merge, split and so on), each
  with an inverse where one exists, from which query rewriting is derived.
- **Lenses** (Foster, Greenwald, Moore, Pierce, Schmitt, "Combinators for bidirectional tree
  transformations", TOPLAS 2007) and **edit lenses** (Hofmann, Pierce, Wagner, POPL 2012): get and put
  with round-trip laws. Cambria (Litt, van Hardenberg, Henry, Ink & Switch 2020, and an MIT thesis) applies
  them to schema evolution between live peers. panproto-lens (Rust) packages lens combinators with
  law checks (GetPut, PutGet).
- **Asymmetric presence** (Typical, Boyer) and a draft essay "Interface description languages
  should support more asymmetric types", which generalizes the idea: the constructor check must imply
  the decoder check, giving asymmetric labels, field fallbacks, variants that can be decoded but not
  produced, and separate construction and decoding predicates. The essay suggests an SMT solver to check
  the implication across versions. Confidence: the essay is marked draft by its author.
- **Robustness** (RFC 9413, Thomson and Schinazi, 2023): strictness plus active maintenance beats
  tolerance.
- **Refinement types** (Rondon, Kawaguchi, Jhala, "Liquid types", PLDI 2008; Liquid Haskell): decidable
  predicates on base types, the theory under `where`.
- **Grammar-constrained decoding** (Geng et al., "Grammar-constrained decoding for structured NLP tasks
  without finetuning", EMNLP 2023; XGrammar, 2024; llguidance): a schema compiled to a grammar makes a
  model's output parse by construction. Plan A's harness already plans GBNF and llguidance export.
- **Session types** (Honda 1993; Honda, Yoshida, Carbone 2008): protocols as types. Relevant to service
  ordering, but that is Yan Protocol's job (RFC 0002 D12), not this surface's.

### 3.5 Implications for Yan Schema

- F1. Yan's structural hash plus pinned history is a stronger position than tags. The syntax should make
  the history pleasant, not re-add tags.
- F2. The history should be readable inline (where the reader looks) but must not grow forever. A
  support window that lets old marks fall away fixes TypeSpec's and GraphQL's failure.
- F3. Changes need both directions in a mixed fleet. The syntax should make the backward direction
  visible when it is not automatic.
- F4. Closed by default is right. Openness is a per-type decision and should be one word.
- F5. The surface should look like plain Yan's `type` declarations. A schema author is usually also a
  Yan author, and models trained on one transfer to the other.
- F6. Docs are part of the contract, and exports to OpenAPI, JSON Schema, protobuf and model grammars
  are a large share of the value.

## 4. The five tasks

- **T1 Record.** `Order` with an id, a ticker symbol of 1 to 12 characters, a side, a positive
  quantity, an optional limit price, a time-in-force defaulting to `Day`, and an account; doc comments
  on the type and on `qty`.
- **T2 Sums.** `Side` (two nullary variants) and `Payment` (`Card` with a four-character `last4`,
  `Bank`, `Voucher`), where unknown future payment kinds must decode.
- **T3 History.** v1 has `owner` and `note: str?` and `qty: i32`. v2 adds `tif` with a default, the
  `Voucher` variant and the `watch` method. v3 renames `owner` to `account`, widens `qty` to `i64`,
  removes `note`, and adds `Page`, `Window` and `list`.
- **T4 Service.** `Book` with `place` (fails `Rejected`), `cancel` (fails `NotFound`), `watch` (a
  stream of quotes) and `list`.
- **T5 Power.** A generic `Page[T]` and a `Window` whose `start` is not after `end`.

Baseline, the RFC 0002 sketch (it cannot write T2's `open`, T3's removal and widening, or T5):

```
package acme.orders

record Order since v1
  id: OrderId
  sym: str where len(sym) >= 1 and len(sym) <= 12
  side: Side
  qty: i64 where qty > 0
  limit: Money?
  tif: Tif = Tif.day since v2
  account: AccountId was owner since v3

enum Side
  buy
  sell

union Payment since v1
  Card(token: str, last4: str)
  Bank(iban: str)
  Voucher(code: str) since v2

service Book
  place(o: Order) -> Ack fail Rejected
  cancel(id: OrderId) -> Ack fail NotFound
  watch(sym: str) -> stream Quote since v2
```

Baseline, protobuf editions 2023 with protovalidate (excerpt; the full version is 61 lines):

```
message Order {
  reserved 7;
  reserved "note";
  string id = 1;
  string sym = 2 [(buf.validate.field).string = {min_len: 1, max_len: 12}];
  Side side = 3;
  // Shares. Never zero.
  int64 qty = 4 [(buf.validate.field).int64.gt = 0];
  acme.common.Money limit = 5;
  Tif tif = 6;
  string account = 8;
}

enum Side {
  SIDE_UNSPECIFIED = 0;
  SIDE_BUY = 1;
  SIDE_SELL = 2;
}
```

## 5. Five proposals

### P1 Terse: brevity first

**Idea.** A schema is a grammar; write it like one (CDDL, ArkType). Fields are `name Type` without a
colon, sizes are `str[1..12]`, `!` is failure, `*T` is a stream, and history is a changelog at the bottom.

```
package acme.orders

Order = {id OrderId, sym str[1..12], side Side, qty i64>0, limit Money?, tif Tif=Day, account AccountId}
Side = Buy | Sell
Payment = Card(token str, last4 str[4]) | Bank(iban str) | Voucher(code str)
Page[T] = {items [T], next Cursor?}
Window = {start Date, end Date} & start <= end

Book ::
  place Order -> Ack ! Rejected
  cancel OrderId -> Ack ! NotFound
  watch str -> *Quote
  list Window, Cursor? -> Page[Order]

v2: +Order.tif +Payment.Voucher +Book.watch
v3: Order.owner>account Order.qty:i32>i64 -Order.note +Window +Page +Book.list
```

- **Buys.** The shortest file by far (section 6). The changelog line per version is a strong idea: it
  is the whole history in one glance.
- **Costs.** Unnamed parameters cannot carry docs or show up in generated clients. `!`, `*`, `::`, `&`
  and `>` are new meanings a Yan reader must learn; `qty i64>0` reads as a comparison. No room for doc
  comments on fields. Fails F5.
- **Keep.** Single-line `type X = { ... }` and `A | B` (already plain Yan), and a one-line per-version
  summary as a tool view.

### P2 Ledger: evolution first

**Idea.** The file is the history. Each version block lists operations (PRISM's operators, Stripe's
version changes); the current shape is the fold of the ledger. Converters fall out of the operators.

```
package acme.orders

v1 2026-03-01
  type Order
    id: OrderId
    sym: str where .len() in 1..12
    side: Side
    qty: i32 where > 0
    limit: Money?
    owner: AccountId
    note: str?
  type Side = Buy | Sell
  type Payment = Card(token: str, last4: str where .len() == 4) | Bank(iban: str)
  service Book
    place(o: Order) -> Ack fail Rejected
    cancel(id: OrderId) -> Ack fail NotFound

v2 2026-06-10
  add Order.tif: Tif = Day
  add Payment.Voucher(code: str)
  add Book.watch(sym: str) -> stream Quote

v3 2026-11-04
  rename Order.owner to account
  widen Order.qty to i64
  drop Order.note
  add type Page[T] = { items: List[T], next: Cursor? }
  add type Window = { start: PlainDate, end: PlainDate } where start <= end
  add Book.list(w: Window, after: Cursor?) -> Page[Order]
```

- **Buys.** History is exact, reviewable and append-only; every operator has a known converter, and the
  inverse is known for most (rename, widen with a check, add with default). Code review of an API change
  is reading one block. Dated versions sort and tell their age.
- **Costs.** The current shape of `Order` is not written anywhere; a reader needs a tool view. After
  thirty versions the file is a changelog. It duplicates `.yan/schemas/`, which already holds the
  history (N3 says recovery comes from the pins).
- **Keep.** The operator vocabulary (`add`, `drop`, `rename`, `widen`, `retype via`) as the language
  of `yan schema log` and of diagnostics. Dated version labels as an option.

### P3 Contract: safety and explicitness first

**Idea.** Nothing is implied. Every field states its presence for writers and readers, every type says
open or closed, the file states the window of versions it accepts, and every method states its effect,
retry rule and deadline.

```
package acme.orders
accepts v1..v3

record Order since v1 closed
  id: OrderId          required
  sym: str             required   where len(sym) in 1..12
  side: Side           required
  qty: i64             required   where qty > 0         widened from i32 at v3
  limit: Money         optional
  tif: Tif             defaulted  Tif.Day              since v2
  account: AccountId   required   renamed from owner at v3
  note: str            removed at v3

enum Side since v1 closed
  Buy
  Sell

union Payment since v1 open
  Card(token: str required, last4: str required where len(last4) == 4)
  Bank(iban: str required)
  Voucher(code: str required)  since v2

record Window since v3 closed
  start: PlainDate     required
  end: PlainDate       required
  invariant start <= end

service Book since v1
  place(o: Order) -> Ack fail Rejected
    effect write  retry never  deadline 2s
  cancel(id: OrderId) -> Ack fail NotFound
    effect write  retry idempotent
  watch(sym: str) -> stream Quote fail NotFound  since v2
    effect read  credit 64
```

- **Buys.** Nothing to guess. The `accepts` window is the key idea: it tells the checker which peer
  versions are in service, so it can demand converters for exactly those pairs and let older ones go.
  Presence words make Typical's asymmetric state writable (`write-required read-optional`).
- **Costs.** The longest Yan-like file (section 6). `required` on almost every line is noise; the
  default should be the common case. Effects, retries and deadlines are runtime policy that belongs to
  the cluster layer and its handlers, not to the wire type; putting them here adds semantics (R568
  territory) for little gain.
- **Keep.** The window, `open` as one word on a type, and presence asymmetry as a concept (spelling
  left open, Q2).

### P4 Algebra: expressive power first

**Idea.** Schemas are values in a type algebra (TypeScript, TypeSpec templates, Smithy mixins and
resources, CUE). Spread fields, derive types by subtraction, declare resources with lifecycle
operations, and state call order as a protocol.

```
package acme.orders

type Sym = str where matches(/[A-Z]{1,12}/)
type Audited = { created: Instant, by: AccountId }

type Order = {
  ..Audited
  id: OrderId
  sym: Sym
  side: Side
  qty: i64 where > 0           was i32 since v3
  limit: Money?
  tif: Tif = Day               since v2
  account: AccountId           was owner since v3
}
type Draft = Order without id, ..Audited
type Patch = partial Draft

type Side = Buy | Sell
type Payment = Card(token: str, last4: str where .len() == 4) | Bank(iban: str) | Voucher(code: str) since v2

type Page[T] = { items: List[T], next: Cursor? }
type Window = { start: PlainDate, end: PlainDate } where start <= end

resource Order by id in Book
  create place(Draft) fail Rejected
  read get fail NotFound
  update amend(Patch) fail Rejected
  delete cancel fail NotFound
  list(w: Window) paged
  watch(sym: Sym) -> stream Quote since v2

protocol Book.session = place ; (watch | amend | cancel)*
```

- **Buys.** Large APIs shrink: `Draft` and `Patch` are one line each, and a resource writes CRUD,
  listing and paging in six lines. Named refinement aliases (`Sym`) are reusable.
- **Costs.** `without` and `partial` create types whose hash depends on a computation; under N3 the
  printer must recover the operator from stage-0 metadata, and a change to `Order` silently changes
  `Patch` (a breaking change at a distance). Resources hide method names and paging shape behind
  conventions (Smithy's lesson: powerful, but a second language to learn). Protocols are Yan Protocol's
  job.
- **Keep.** Generics, named refinement aliases, type-level `where`, and `..T` spread of a record's
  fields (with care, Q5). Decline `without`, `partial`, resources and protocols for now.

### P5 Facts: agent and tool cost first

**Idea.** One line is one fact, addressed by its full path. The file is a sorted table: no indentation,
no nesting, order-independent. Every line matches one regular pattern, so a model can be constrained to
it with a tiny grammar, a patch touches one line, and merges almost never conflict.

```
package acme.orders

Order              type                              v1
Order.id           OrderId                           v1
Order.sym          str | .len() in 1..12             v1
Order.side         Side                              v1
Order.qty          i64 | > 0                         v1  i32 until v3
Order.limit        Money?                            v1
Order.tif          Tif = Day                         v2
Order.account      AccountId                         v1  owner until v3
Order.note         gone                              v1  until v3
Side               Buy | Sell                        v1
Payment.Card       (token: str, last4: str | .len() == 4)  v1
Payment.Bank       (iban: str)                       v1
Payment.Voucher    (code: str)                       v2
Page[T].items      List[T]                           v3
Page[T].next       Cursor?                           v3
Window             | start <= end                    v3
Window.start       PlainDate                         v3
Window.end         PlainDate                         v3
Book.place         (o: Order) -> Ack ! Rejected      v1
Book.cancel        (id: OrderId) -> Ack ! NotFound   v1
Book.watch         (sym: str) -> stream Quote        v2
Book.list          (w: Window, after: Cursor?) -> Page[Order]  v3
```

- **Buys.** Every member has a stable path, which is what diagnostics, history, CLI queries and agent
  patches want to name. `grep Order.` is the type. Few lexemes per fact (section 6). The grammar is
  regular per line.
- **Costs.** Paths repeat on every line, so it has the most characters of the Yan-like proposals. There
  is no overview of a type's shape at a glance unless an editor folds by prefix. Doc comments have no
  natural place. It looks like nothing else in Yan (fails F5).
- **Keep.** Member paths (`Order.qty`) as the addressing scheme everywhere tools talk about a schema,
  and this exact table as a machine output (`yan schema facts`), not as a source syntax.

## 6. Experiment: size of the same five tasks

Counts over the same five tasks. "Full" means all five tasks are expressed. Lexemes approximate tokens
(identifiers and numbers count as one, each punctuation character as one); a real BPE tokenizer was not
reachable, so treat ratios, not absolute values, as the result.

| Version | Full | Lines | Chars without whitespace | Lexemes |
| --- | --- | --- | --- | --- |
| Protobuf editions + protovalidate | yes, history only via `reserved` | 61 | 1178 | 397 |
| TypeSpec 1.0 + versioning | no cross-field rule, no stream | 42 | 985 | 346 |
| RFC 0002 sketch | no (T2 open, T3 drop and widen, T5) | 29 | 477 | 154 |
| P1 Terse | yes, no docs | 16 | 468 | 172 |
| P2 Ledger | yes, no docs | 29 | 611 | 222 |
| P3 Contract | yes, no docs | 40 | 871 | 240 |
| P4 Algebra | yes, plus Draft, Patch, resource | 33 | 722 | 237 |
| P5 Facts | yes, no docs | 24 | 585 | 213 |
| Mix (section 7) | yes, with two doc comments | 31 | 623 | 220 |

Observations:

- The Yan-like proposals take 40% (P1) to 74% (P3) of protobuf's characters; the mix takes 53% of
  protobuf's and 63% of TypeSpec's. The savings come from dropping tags, request wrapper messages,
  decorator syntax and import boilerplate.
- P1 is only 22% smaller than the mix in lexemes. Most of P1's savings come from dropping parameter
  names and colons, the parts a generated client and a reader need most. Brevity beyond the mix has
  poor returns.
- P5 has fewer lexemes than P3 but more characters than any other Yan-like version: path repetition is
  cheap in tokens (common prefixes merge in BPE) but costly to the eye.
- The mix costs about 43% more lexemes than the RFC sketch and covers the three tasks the sketch cannot
  write, plus docs.

## 7. The mix: a consolidated proposal

### 7.1 Principles

1. **It is plain Yan's `type`, nothing new to learn.** One keyword `type` for records and sums, block or
   one-line form, `name: Type` fields, PascalCase variants, `?` for optional, `= default`, `where`
   refinements, `---` doc comments, `--` comments (from P1 and F5). `record`, `enum` and `union` go.
2. **History is trailing marks, printed from the pins.** `since v`, `was old`, `drop`, `via f back g`.
   The printer aligns marks into one column per block, so the current shape reads first and the history
   reads second (from P2, P4).
3. **A window keeps the file clean.** `window v2..v3` names the oldest version still in service and the
   current one. The checker requires converters, in both directions, for every version in the window,
   and `yan fmt` removes marks and dropped members older than the floor. The history stays in
   `.yan/schemas/`; the source never becomes a graveyard (from P3, against TypeSpec and GraphQL).
4. **Closed and tag-free by default.** Unknown fields and variants are rejected; `open` on a type opts in
   to unknown variants (decoded to a reserved `Unknown` case) and unknown fields (skipped). No field
   numbers; `@proto(n)` exists only for export.
5. **Every member has a path.** `Order.qty`, `Payment.Voucher`, `Book.watch`. Diagnostics, `yan schema
   log`, `yan schema show`, exports and agent patches all use it (from P5).
6. **Exports are part of the surface's job.** OpenAPI 3.1, JSON Schema 2020-12, protobuf, and model
   output grammars (GBNF, llguidance), with doc comments as descriptions (F6).

### 7.2 The five tasks in the mix

```
package acme.orders
window v2..v3

--- A limit or market order.
type Order
  id: OrderId
  sym: Sym
  side: Side
  --- Shares. Never zero.
  qty: i64 where > 0               was i32 since v3
  limit: Money?
  tif: Tif = Day                   since v2
  account: AccountId               was owner since v3
  drop note: str?                  since v3

type Sym = str where .len() in 1..12
type Side = Buy | Sell

type Payment open
  Card(token: str, last4: str where .len() == 4)
  Bank(iban: str)
  Voucher(code: str)               since v2

type Page[T] = { items: List[T], next: Cursor? }
type Window = { start: PlainDate, end: PlainDate } where start <= end

service Book
  place(o: Order) -> Ack fail Rejected
  cancel(id: OrderId) -> Ack fail NotFound
  watch(sym: Sym) -> stream Quote fail NotFound   since v2
  list(w: Window, after: Cursor?) -> Page[Order]  since v3
```

Reading notes:

- `where > 0` and `where .len() in 1..12` elide the subject. A predicate that starts with an operator
  or with a leading-dot call (plain Yan's `.sym` section form) applies to the field or the aliased
  type. Anything else names the field explicitly, as in `where start <= end`.
- `was i32` on a field is a type change; `was owner` is a rename (a lowercase name versus a type).
  Widening that the core can prove lossless (`i32` to `i64`) needs no function. The backward direction
  (an old reader receiving `i64`) is partial, so the checker reports that a v2 reader fails
  `SchemaMismatch` for values above `i32` range, unless a `back` function is given.
- `drop note: str? since v3` keeps the removed field visible while v2 peers are in the window. When the
  window floor moves to v3, `yan fmt` deletes the line.
- `= Day` resolves the variant from the field's type (Q6).
- `type Payment open`: an old reader that meets a future variant gets `Payment.Unknown`; a new
  variant without `open` is a breaking change, matching spec 20.

### 7.3 Evolution in depth

The same type two versions later, with a non-trivial change and the window moved:

```
package acme.orders
window v3..v5

type Order
  id: OrderId
  sym: Sym
  side: Side
  qty: i64 where > 0
  --- Limit price, now with currency.
  limit: Price?                    was Money via price_of back money_of since v5
  tif: Tif = Day
  account: AccountId
  ..Audited                        since v4
```

- `via price_of back money_of` names two plain Yan functions (cross-surface calls by name, N9). Both
  must be total and `uses none`, or the checker reports the direction that cannot be generated.
- `..Audited` spreads the fields of a record type defined elsewhere (from P4). It elaborates to the
  fields themselves, so the hash is the same as writing them out; N3 recovery uses stage-0 metadata
  (Q5).
- The `since v2` and `was owner` marks from section 7.2 are gone: v2 left the window. `yan schema log
  Order` still prints them, using the P2 operator vocabulary:

```
v2  add Order.tif = Day
v3  rename Order.owner to account; widen Order.qty to i64; drop Order.note
v4  add Order.created, Order.by (spread Audited)
v5  retype Order.limit Money to Price via price_of back money_of
```

### 7.4 Mock grammar

Non-normative. It extends the mock in `docs/surfaces.md`; lexis is plain Yan's (N9).

```
module   = "package" path NEWLINE ["window" VERSION ".." VERSION NEWLINE] {use} {item}
item     = {doc} {attr} (type | service)
type     = "type" TYPENAME [tparams] ["open"]
           ( "=" texpr [where] [marks] NEWLINE
           | [where] [marks] NEWLINE INDENT member {member} DEDENT )
member   = {doc} {attr} ["drop"] (field | variant | spread) [marks] NEWLINE
field    = NAME ":" texpr ["=" const] [where]
variant  = TYPENAME ["(" field {"," field} ")"]
spread   = ".." TYPENAME [targs]
texpr    = tref ["?"]
         | "{" field {"," field} "}"
         | variant {"|" variant}
where    = "where" (expr | binop expr | "." call {binop expr})
marks    = ["was" (NAME | tref) ["via" path ["back" path]]] ["since" VERSION]
service  = "service" TYPENAME [marks] NEWLINE INDENT method {method} DEDENT
method   = {doc} {attr} ["drop"] NAME "(" [params] ")" ["->" ["stream"] texpr] ["fail" texpr]
           [marks] NEWLINE
```

### 7.5 Lowering and what it needs

| Construct | Lowers to | Status |
| --- | --- | --- |
| `type` with fields or variants | `type T = {...}` or a sum, with `derive(Wire)` | existing (R537 lowering) |
| field and type `where` | refinement, or type invariant checked after construction | existing (3.5, 8.5, spec 17 decoding) |
| subject-elided `where` | the same refinement with the subject filled in | lowering only; stage-0 metadata for printing |
| `open` | `@non_exhaustive` plus the `Wire` opt-in for unknown fields and variants | existing pieces; the `Unknown` case needs a fixed generated-name rule (N4) |
| `since`, `was name`, `drop` | R406 converters from the pins | existing |
| `was T` (widen) | generated converter, partial in the backward direction | existing mechanism; partiality report is new tooling |
| `via f back g` | user-supplied converter functions in the R406 converter | likely needs a row: R406 says converters are generated |
| `window` | which pinned versions need converters; what `fmt` prunes | needs a row (new semantics for the checker, none at runtime) |
| `..T` spread | the fields of `T` written out | lowering only (N3 via stage-0 metadata) |
| `service`, `stream` | `@remote` trait and `Remote` handle; stream per R555 | existing (R401, R407, R555) |
| exports | `yan schema export` to OpenAPI, JSON Schema, protobuf, GBNF, llguidance | tooling; protobuf through `@yan/protobuf` already planned |

Five facts (N5): failure is `fail` on methods; concurrency is `stream`; mutation, compile time and
capabilities never occur (capabilities never travel, R407).

### 7.6 What each proposal gave

| From | Kept | Left out |
| --- | --- | --- |
| P1 Terse | one-line `type X = {...}` and `A \| B`; a one-line-per-version log view | unnamed parameters, new sigils |
| P2 Ledger | operator vocabulary for `log` and diagnostics; history as data | the ledger as the source |
| P3 Contract | `window`; one-word `open`; presence asymmetry as an open question | `required` everywhere; effect, retry, deadline lines |
| P4 Algebra | generics, refinement aliases, type-level `where`, `..T` spread | `without`, `partial`, resources, protocols |
| P5 Facts | member paths; `yan schema facts` as machine output | the flat table as source |

## 8. Open questions

- **Q1 Version labels.** `v1, v2` (RFC) or dates (`2026-11-04`, Stripe, P2)? Dates sort, show age and
  need no numbering decision; Yan already has date literals. Recommendation: allow a date as an optional
  label on `yan schema pin`, keep `vN` in source.
- **Q2 Asymmetric presence.** Typical's middle state (required to write, optional to read) needs the
  constructor check to differ from the decoder check, which the core does not have today (decoding runs
  the same checks as construction). Candidate spellings: `name: T?write`, `name: T since v4 soft`, or a
  field-level `asym`. Needs a core row if accepted; it is the one feature here with real semantics.
- **Q3 Window semantics.** Is the window a source declaration (as in the mix) or a property of the
  deployment (pins plus a cluster setting)? Source is reviewable; deployment is truer.
- **Q4 Call policy.** Do idempotency, retry and deadline belong anywhere in `.ysd`? If the cluster layer
  grows them, an attribute on the method (`@idempotent`) is the natural place. Not in the mix.
- **Q5 Spread.** Is `..T` worth its N3 recovery cost, and does a change to `T` count as a change to
  every type that spreads it (it must, for the hash)? The checker should say so in the report.
- **Q6 Bare variant defaults.** `= Day` instead of `= Tif.Day`. Fine in `.ysd`; should plain Yan allow it
  for consistency?
- **Q7 Subject-elided `where`.** Same question: adopt `where > 0` in plain Yan too, or keep it
  `.ysd`-only?
- **Q8 Model-output schemas.** Yan Prompt declares output types such as `Triage`. Should those live in
  `.ysd`, so one surface defines both wire types and model-output grammars? It would make the GBNF and
  llguidance exports the Plan A harness needs a schema-surface feature.
- **Q9 Imports.** OpenAPI and JSON Schema import (RFC 0002 "later") will meet constructs with no Yan
  equivalent (`anyOf` without a discriminator, `patternProperties`). Map to the nearest type and report,
  or refuse?

## 9. Not proposed

- Field tags in source. The structural hash is the identity (R406).
- HTTP routes, verbs and status codes in the schema. That is an export mapping.
- Evaluation of any kind (CUE, Dhall, Pkl). A schema stays decidable (N2).
- Semantics behind a boundary signature (R568). Nothing in the mix needs an engine.

## 10. Sources

Confidence: **high** means checked this session; **medium** means stable prior knowledge, not
re-checked.

- Protobuf editions feature settings, including `field_presence` (high): https://protobuf.dev/editions/features/
- Protobuf editions overview (high): https://protobuf.dev/editions/overview/
- TypeSpec 1.0 GA release notes (high): https://typespec.io/blog/typespec-1-0-GA-release/
- Typical (high): https://github.com/stepchowfun/typical
- "Interface description languages should support more asymmetric types", draft essay (high that it
  exists; the author marks it draft): https://maxbo.neocities.org/interface-description-languages-should-support-more-asymmetric-types/index.html
- Project Cambria, Ink & Switch (high): https://inkandswitch.com/cambria
- Cambria: schema evolution in distributed systems with edit lenses, MIT (high): https://dspace.mit.edu/handle/1721.1/145983
- panproto-lens (high): https://docs.rs/crate/panproto-lens/latest/source/src/lib.rs
- Stripe, "APIs as infrastructure: future-proofing Stripe with versioning" (high): https://stripe.com/ae/blog/api-versioning
- Stripe API versions (high): https://docs.stripe.com/api-versions
- BAML, why BAML and schema-aligned parsing (high): https://docs.boundaryml.com/guide/introduction/why-baml
- Apache Avro specification, schema resolution (medium): https://avro.apache.org/docs/
- Cap'n Proto language and evolution rules (medium): https://capnproto.org/language.html
- Smithy 2 resources and mixins (medium): https://smithy.io/2.0/
- JSON Schema 2020-12 (medium): https://json-schema.org/specification
- OpenAPI 3.1 (medium): https://spec.openapis.org/oas/v3.1.0
- RFC 8610 CDDL (medium): https://www.rfc-editor.org/rfc/rfc8610
- RFC 8927 JSON Type Definition (medium): https://www.rfc-editor.org/rfc/rfc8927
- RFC 9413 Maintaining robust protocols (medium): https://www.rfc-editor.org/rfc/rfc9413
- Curino, Moon, Zaniolo, PRISM, VLDB 2008 (medium)
- Foster et al., bidirectional tree transformations, TOPLAS 2007; Hofmann, Pierce, Wagner, edit lenses,
  POPL 2012 (medium)
- Rondon, Kawaguchi, Jhala, Liquid types, PLDI 2008 (medium)
- Geng et al., grammar-constrained decoding, EMNLP 2023; XGrammar 2024 (medium)
- Kleppmann, Designing Data-Intensive Applications, chapter 4, encoding and evolution (medium)
