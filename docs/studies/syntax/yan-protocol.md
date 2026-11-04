# Yan Protocol 言礼 (`.ypc`): syntax and semantics study

- Date: 2026-11-04
- Status: brainstorm study. Not a spec change, not an RFC. Every item below is labelled D1, D2 and so
  on; none has a row number or a diagnostic code. Existing rows are cited only as references.
- Surface: Yan Protocol 言礼, extension `.ypc`, research track (spec 16.7). Names are frozen.
- Inputs: `docs/surfaces.md` section 16, RFC 0002 D12, RFC 0018 and spec 9.5 (rows R633, R634),
  spec 16.7 (R536, R547), spec 10 (`@linear`, R443), spec 9.4 (deadlines).

礼 is rite, etiquette, the agreed order of a ceremony. A protocol is exactly that: who speaks, to
whom, in what order, and what each party may say next. The study keeps that image throughout.

## 1. Summary

1. Yan already has the semantics. R633 (global protocols projected onto roles) and R634 (linear
   endpoints, deadlock freedom within a session, `SessionClosed` on failure) are normative. What is
   missing is a first-class notation: today a protocol is a stage-0 string, and every role is written
   by threading a linear endpoint through `send` and `recv` by hand.
2. So `.ypc` can be a pure lowering surface (N1, no engine). Most of what the study proposes lowers
   to R633 and R634 with no core change. Four ideas need the core, and they are marked.
3. The recommended framework has three layers in one file: a **protocol** (the global type), a
   **`plays` clause** that turns an ordinary Yan function into one role with the endpoint hidden, and
   an optional **`play`** (a choreography: the whole interaction written once, projected into one
   function per role).
4. The core notation is the directed choice of the theory, written as an indented sequence diagram:
   `A -> B: Label(fields)` for a message and `A -> B` followed by indented labels for a choice. It
   is as short as the terse sigil proposal in tokens (shorter on two of four tasks), with none of its
   sigils.
5. A prototype projector (section 4) checked the notation on the four tasks. It caught a realistic
   agent-orchestration bug, and it showed that local types must be shown as state machines, never as
   trees, because a coordinator over n parties grows as 2 to the n as a tree.

## 2. Methodology and process

Steps, in order:

1. **Read narrowly.** `docs/surfaces.md` section 16 (the RFC example and the mock grammar), RFC 0002
   D12, RFC 0018, the spec text of R633, R634, R547 and the admission rules N1 to N9 (R536), the
   `@linear` row (R443) and the deadline rules of 9.4. Nothing else in `spec/` or `conformance/`.
2. **Survey peers and literature.** From memory of the field, then targeted web searches to check
   recent work (2023 to 2026) and to find what I did not know. Each source below carries a confidence
   mark: **high** (read or well known, checked), **medium** (known, details from memory), **low**
   (title and abstract only).
3. **Fix five tasks** that stress different features, written in every proposal:
   - T1 Checkout: the RFC 0002 example. Two roles, a choice, a nested choice.
   - T2 Download: unbounded recursion, a per-message deadline, a failing peer.
   - T3 Two-phase commit: a coordinator and n participants (a role family).
   - T4 Agent review loop: Lead, Coder and Critic LLM agents, at most three rounds.
   - T5 Implement one role: the Shop side of T1, as runnable code.
4. **Write five proposals**, each optimizing one axis as hard as it can, so its costs show.
5. **Measure.** Lexical tokens and non-blank lines per task (section 4.2). No model tokenizer was
   reachable from the sandbox, so lexical tokens (identifiers, numbers, operators, punctuation) are a
   proxy; they over-count sigil syntax relative to a BPE tokenizer.
6. **Prototype.** A 210-line Python projector for the mix's protocol syntax: parse, unroll families
   and bounded loops, project onto each role with full merging, report unprojectable roles. It ran
   on T1 to T4 plus one deliberately broken variant of T4.
7. **Mix.** Take each proposal's best idea that survives the others' criticism, check it against the
   one-core rules (N1 to N9, law 4), and sort each into lowering-only or core change.

Artifacts (project files, not in the repo): `ypc-study/project.py` (the projector), `ypc-study/tasks.ypc`
(T1 to T4 in the mix syntax), `ypc-study/projections.txt` (its output), `ypc-study/samples/` (every code
sample in this document, one file per proposal) and `ypc-study/count.py` (the counter).

## 3. Findings

### 3.1 Peers and competitors

| Family | Examples | What they get right | What goes wrong |
| --- | --- | --- | --- |
| Global protocol languages | Scribble, nuScr, Session* (F*), STScript (TypeScript), Teatrino (Scala 3) | One global description, projected per role; checked once | Separate toolchain and code generation step; generated APIs feel foreign; adoption stayed academic |
| Session types embedded in a host type system | Rust `session-types`, Ferrite, mpstthree, Rumpsteak; Haskell, Scala Effpi | Real compile-time checking in a mainstream language | Type-level encodings give unreadable types and errors; endpoint threading is noisy |
| Choreographic programming | Choral (Java), HasChor, MultiChor (Haskell), ChoRus (Rust), ChoreoTS, Pirouette, Klor (Clojure) | Write the interaction once with local computation; deadlock freedom by construction; no realizability check | Hard to talk to code you do not own; knowledge-of-choice messages can be hidden broadcasts |
| Model checkers and executable specs | P (AWS, Microsoft), TLA+, Promela | Find protocol bugs in design; used in production at scale | A model, not the code; drift between model and implementation |
| State machine frameworks | Erlang `gen_statem`, XState statecharts, Rust typestate pattern | Familiar; named states; good runtime tooling and visualizers | No global view; nothing checks that two machines agree |
| Interface description languages | gRPC and protobuf, AsyncAPI, Smithy, TypeSpec | Wire schemas, codegen, breaking-change checks | Describe messages, not order; streaming is unordered beyond "a stream of T" |
| Diagram notations | Mermaid and PlantUML sequence diagrams, UML interactions, message sequence charts | Everybody can read them; `A -> B: msg` is near universal | Not checked; `alt` guards are prose |

### 3.2 What users want, and what works

Evidence-weighted, strongest first:

1. **Message-passing bugs are real and common.** Tu et al. (ASPLOS 2019) studied concurrency bugs in
   large Go projects and found message passing caused more blocking bugs than shared memory. This is
   the core motivation: channels alone do not order conversations. (high)
2. **People already think in sequence diagrams.** `A -> B: msg` is the shared notation of Mermaid,
   PlantUML and MSCs. A protocol that looks like one needs almost no teaching. (high that it is
   widespread; medium that it transfers to checked syntax)
3. **Session types help when the tooling hides the encoding.** Voinea and Gay (PLATEAU 2016) report
   developer benefit from session types in a small study; Coblenz et al. (Obsidian usability study, OOPSLA 2020)
   found typestate usable when states are named and the tool explains transitions. (medium)
4. **Choreographies remove a whole class of errors.** Choral (TOPLAS 2024), HasChor (ICFP 2023) and
   the census-polymorphic libraries (Bates et al., 2025: MultiChor, ChoRus, ChoreoTS) show that
   writing the interaction once gives deadlock freedom by construction, and that "conclaves" avoid
   HasChor's broadcast of every choice to every party. (high for the claims; low for performance,
   which the 2025 paper states qualitatively)
5. **Real systems need time and failure.** Timed multiparty session protocols (Hou, Lagaillardie,
   Yoshida, ECOOP 2024, a Rust implementation) and crash-stop MPST (Barwell, Hou, Yoshida, Zhou;
   ECOOP 2023, and a journal version in LMCS) both exist because deadlines and lost peers are the first thing a
   production protocol meets. (high)
6. **Payloads depend on earlier payloads.** Session* (Zhou et al., OOPSLA 2020) adds refinements
   such as "the amount paid equals the quoted total". Without them, T3's "commit only if everybody
   voted yes" cannot be stated. (high)
7. **Parties are often counted, not named.** Two-phase commit, map-reduce, a swarm of agents.
   Parameterised MPST and census polymorphism (2025) address this. (high)
8. **LLM agents need coordination protocols.** Bollig, Függer and Nowak (2026) propose message
   sequence charts as a DSL for LLM multi-agent workflows, projected to deadlock-free local
   programs, with LLM calls as opaque typed functions (ZipperGen, Python). Li, Hou and Yoshida (2026)
   report that monitored decoding against MPST specifications raised semantic validity of
   LLM-generated protocol refinements from 60.4% to 99.2% on a 7B model. Both match Yan's bet:
   agents write code, and a checker keeps them honest. (low: abstracts and summaries only)
9. **What does not work.** A separate code generator (Scribble's adoption problem); errors phrased in
   the encoding instead of the protocol (embedded Rust and Haskell libraries); a model that drifts
   from the code (TLA+ and P without refinement checks); hidden messages inserted by the compiler,
   which break performance reasoning and audits. (medium, practitioner consensus)

### 3.3 What Yan already has

- R633: a global protocol as a stage-0 string, projected per role, unprojectable is a compile error.
- R634: one `@linear` endpoint per role; `send` and `recv` consume it; choice arrives as a sum;
  `close` only at the end; dropping early is an error; a failing branch cancels the session and the
  peers' `recv` fails with `SessionClosed`; deterministic; asynchronous subtyping MAY reorder sends.
- 9.4: `with timeout(d)` and `with deadline(t)` cancel scopes that reach every `recv`.
- R400 and E720 rules: `uses cluster` and `Wire` payloads for sessions over `Remote`.
- R547: the `.ypc` surface, lowering to `@linear` state types over `std.chan`, research track.

So `.ypc` does not need its own semantics to be useful. It needs a notation for the global type and
a way to write a role without threading the endpoint by hand.

## 4. Experiments

### 4.1 Prototype projector

`ypc-study/project.py` implements the mix's protocol notation (section 6, D1 to D3) and the classic
projection with full merging (Honda, Yoshida, Carbone 2008). It unrolls role families with a fixed n,
`-> all` broadcasts, and `loop max K`.

Results:

1. T1 to T4 project onto every role. The Shop role of T1, as the tool prints it:

```
Shop:
  recv Start from Client
  send Quote to Client
  offer from Client
    Pay
      select to Client
        Receipt
          end
        Declined
          end
    Cancel
      end
```

2. **It caught a realistic agent bug.** In a variant of T4, the Critic sends its verdict to the
   Coder only, and the Coder later reports a final patch to the Lead. The Lead never learns whether
   a round ended, so it cannot know whether to wait for `Final`. The tool reports
   `role Lead cannot tell branches Approve, Revise apart`. This is the exact mistake a planner agent
   makes when it wires a review loop, and the error names the role and the branches.
3. **Bounded loops are free.** `loop max 3` unrolls to a finite protocol (the third `again` falls
   through), so it lowers to R633 with no new construct.
4. **Trees explode; machines do not.** T3's coordinator, printed as a tree, has these line counts:
   n = 1: 17, n = 2: 51, n = 3: 135, n = 4: 335, n = 5: 799, n = 6: 1855. Each vote is a branch whose
   continuations are identical, so a tree repeats the tail 2 to the n times. A state machine with
   shared states stays linear in n. Every view of a local type (`yan view`, IDE hover, error text)
   must therefore print states, not trees.
5. **A protocol cannot say "commit only if all voted yes".** The projection is happy with a
   coordinator that commits after a No, because a protocol types messages, not decisions. Either the
   protocol gets refinements over earlier choices (D4, a core change) or the decision lives in code
   (D6, `play`, lowering only). That is the strongest argument for keeping a choreography layer.

### 4.2 Size of the same tasks in each notation

Lexical tokens / non-blank lines. P0 is today's R633 string form and R634 endpoint code; T3 in P0 is
written out for n = 2 because R633 has no families. P5 writes every role; the others write one
global description.

| Task | P0 | P1 | P2 | P3 | P4 | P5 | Mix |
| --- | --- | --- | --- | --- | --- | --- | --- |
| T1 | 72 / 1 | 69 / 12 | 52 / 3 | 154 / 17 | 120 / 14 | 80 / 10 | 61 / 7 |
| T2 | 48 / 1 | 53 / 10 | 37 / 3 | 109 / 14 | 90 / 12 | 50 / 6 | 35 / 7 |
| T3 | 121 / 1 | 59 / 15 | 55 / 4 | 108 / 10 | 93 / 15 | 107 / 16 | 45 / 9 |
| T4 | 74 / 1 | 73 / 12 | 61 / 3 | 127 / 16 | 119 / 15 | 154 / 18 | 62 / 10 |
| T5 | 123 / 10 | - | - | - | - | 82 / 7 | 85 / 10 |
| T3 as play | - | - | - | - | - | - | 113 / 12 |
| T4 as play | - | - | - | - | - | - | 130 / 17 |
| Running a session | - | - | - | - | - | - | 55 / 4 |

Reading: the mix is the shortest global notation on T2 and T3, one token behind the sigil proposal
on T4 and nine behind on T1. Sigils save characters (P2 is 84 to 123 characters per task against the mix's 133
to 212), not tokens, and a BPE tokenizer narrows that gap further because `->` and keywords are
single tokens. The local-first P5 is shortest on T5 but pays on T3 and T4, where it writes every role.

## 5. Five proposals

Each proposal pushes one axis to its limit. The point is to see what each axis costs.

### P1 Sequence: readability and learnability

Bet: a protocol should be a checked sequence diagram that anyone who has drawn one in Mermaid or
PlantUML can read without a manual. Keywords come from UML interactions: `participant`, `alt`,
`else`, `loop`, `break`, `note`.

T1 Checkout:

```
protocol Checkout
  participant Client, Shop
  Client -> Shop: Start(cart: Cart)
  Shop -> Client: Quote(total: Money)
  alt accept
    Client -> Shop: Pay(token: Token)
    alt paid
      Shop -> Client: Receipt(id: OrderId)
    else declined
      Shop -> Client: Declined(reason: str)
  else cancel
    Client -> Shop: Cancel
```

T2 Download:

```
protocol Download
  participant Client, Server
  Client -> Server: Open(path: Path)
  loop
    alt more
      Server -> Client: Chunk(data: Bytes)
    else done
      Server -> Client: Eof(sha256: Digest)
      break
  note over Client: waits at most 30s for each message
```

T3 Two-phase commit:

```
protocol Commit
  participant Coord
  participants Part
  loop each Part as p
    Coord -> p: Prepare(tx: TxId)
    alt yes
      p -> Coord: Yes
    else no
      p -> Coord: No
  alt commit
    Coord -> Part: Commit
  else abort
    Coord -> Part: Abort
  loop each Part as p
    p -> Coord: Ack
```

T4 Agent review:

```
protocol Review
  participant Lead, Coder, Critic
  Lead -> Coder: Task(spec: Spec)
  loop at most 3 times
    Coder -> Critic: Draft(patch: Patch)
    alt approve
      Critic -> Lead: Approve(patch: Patch)
      Lead -> Coder: Done
      break
    else revise
      Critic -> Lead: Revise(notes: str)
      Lead -> Coder: Fix(notes: str)
```

T5 uses the R634 endpoint API as in P0 (section 5.6).

- Gains: zero learning for diagram users; renders directly to Mermaid; `alt` names read as prose.
- Costs: the chooser is implicit (inferred from the first sender of each branch); branch names and
  message labels say the same thing twice (`alt paid` then `Receipt`); `loop` repeats by default and
  needs `break`, which reads well in diagrams but inverts what a type needs; `note` puts deadlines in
  prose, unchecked.
- Keep: the arrow line, `participant`-free headers aside, and the Mermaid view.

### P2 Terse: brevity and agent token cost

Bet: an agent writes protocols all day, so every character costs. One-letter role aliases, `>` for a
message, braces and `|` for choice, `;` for sequence, `*` for recursion, `^` to loop back, `~` for a
deadline, positional payloads.

T1 Checkout:

```
protocol Checkout(C: Client, S: Shop)
  C>S Start(Cart) ; S>C Quote(Money)
  C>S {Pay(Token) ; S>C {Receipt(OrderId) | Declined(str)} | Cancel}
```

T2 Download:

```
protocol Download(C: Client, S: Server)
  C>S Open(Path)
  * S>C~30s {Chunk(Bytes) ; ^ | Eof(Digest)}
```

T3 Two-phase commit:

```
protocol Commit(C: Coord, P: Part[n])
  each p:P (C>p Prepare(TxId) ; p>C {Yes | No})
  C>P* {Commit | Abort}
  each p:P (p>C Ack)
```

T4 Agent review:

```
protocol Review(L: Lead, D: Coder, R: Critic)
  L>D Task(Spec)
  *3 D>R Draft(Patch) ; R>L {Approve(Patch) ; L>D Done | Revise(str) ; L>D Fix(str) ; ^}
```

- Gains: fewest characters by far; a whole protocol fits on a line or two; close to the formal
  global type, so the projection is easy to see for an expert.
- Costs: not fewer tokens than the mix (section 4.2); positional payloads lose field names, which
  agents and readers rely on; nested braces are where agents make bracket errors; aliases add a
  lookup; it reads as code golf, which the brief calls obnoxious.
- Keep: one-line alternatives (`Yes | No`), `loop max K` as a number, `-> all` for a family.

### P3 Choreography: expressiveness and power

Bet: do not write a protocol at all. Write the interaction once as a program, with each value and
statement located at a role (`@Client`), and let the compiler project one function per role. The
protocol is inferred and printed by `yan view --as protocol`.

T1 Checkout:

```
play checkout(cart: Cart @Client, wallet: Wallet @Client, db: Db @Shop) -> Outcome @Client
    fail ShopErr uses io @Client, db @Shop
  Client -> Shop: Start(cart)
  let total @Shop = db.price(cart)?
  Shop -> Client: Quote(total)
  if io.confirm(t"pay \(total)?") @Client
    Client -> Shop: Pay(token: wallet.token())
    match db.charge(token, total) @Shop
      Ok(id) ->
        Shop -> Client: Receipt(id)
        Paid(id)
      Err(e) ->
        Shop -> Client: Declined(e.text())
        Refused(e.text())
  else
    Client -> Shop: Cancel
    Cancelled
```

T2 Download:

```
play download(path: Path @Client, fs: Fs @Server) -> Bytes @Client
    fail DownloadErr uses fs @Server
  Client -> Server: Open(path)
  let file @Server = fs.open(path)?
  var got @Client = Bytes.empty()
  loop
    match file.read(64kb)? @Server
      Some(data) ->
        Server -> Client: Chunk(data) within 30s
        got.append(data) @Client
      None ->
        Server -> Client: Eof(sha256: file.sha256()) within 30s
        break
  got
```

T3 Two-phase commit:

```
play commit(tx: TxId @Coord, store: Store @Part[n]) -> Decision @Coord uses db @Part
  let votes @Coord = for p in Part
    Coord -> p: Prepare(tx)
    p -> Coord: Vote(ok: store.prepare(tx).is_ok() @p)
  let d @Coord = if votes.all(.ok) then Commit else Abort
  Coord -> all Part: d
  for p in Part
    store.finish(tx, d)? @p
    p -> Coord: Ack
  d
```

T4 Agent review:

```
play review(spec: Spec @Lead, coder: Model @Coder, critic: Model @Critic) -> Patch? @Lead
    uses llm @Coder, llm @Critic
  Lead -> Coder: Task(spec)
  var notes @Coder = ""
  loop max 3
    Coder -> Critic: Draft(patch: coder.write(spec, notes)?)
    match critic.review(patch)? @Critic
      Pass ->
        Critic -> Lead: Approve(patch)
        Lead -> Coder: Done
        return Some(patch)
      Notes(n) ->
        Critic -> Lead: Revise(n)
        Lead -> Coder: Fix(n)
        notes = n @Coder
  None
```

T5 is not needed: the play is the implementation of every role.

- Gains: the most powerful; decisions live next to messages, so "commit only if all voted yes" is
  plain code; deadlock freedom by construction; census polymorphism comes naturally (`for p in Part`,
  `@Part`); for agent swarms the whole workflow is one readable unit.
- Costs: largest text, because it carries the computation too; it cannot describe a peer you do not
  own (a payment gateway, a browser); every value needs a location, which is noise in simple cases;
  an inferred protocol changes silently when code changes, so it is a poor contract between teams.
- Keep: as an optional layer on top of a declared protocol, never instead of one.

### P4 Contract: safety and explicitness

Bet: every protocol that crosses a trust boundary will be attacked or will fail, so the notation
makes the failure cases mandatory. Each receive has a deadline, each payload a refinement, each role a
loss handler, and each untrusted role a runtime monitor.

T1 Checkout:

```
protocol Checkout between Client, Shop
  trust Client untrusted
  Client -> Shop: Start(cart: Cart where cart.len() > 0) within 10s
  Shop -> Client: Quote(total: Money where total > 0) within 5s
  choice by Client within 15m
    accept:
      Client -> Shop: Pay(token: Token, amount: Money where amount == total)
      choice by Shop within 30s
        paid: Shop -> Client: Receipt(id: OrderId)
        declined: Shop -> Client: Declined(reason: str)
    cancel:
      Client -> Shop: Cancel
  on lost Shop: Client fails ShopGone
  on lost Client: Shop releases cart
```

T2 Download:

```
protocol Download between Client, Server
  trust Server untrusted
  Client -> Server: Open(path: Path where path.is_relative()) within 10s
  rec chunks
    choice by Server within 30s
      more:
        Server -> Client: Chunk(data: Bytes where data.len() <= 1mb)
        again chunks
      done:
        Server -> Client: Eof(sha256: Digest where sha256 == sha256_of(chunks.data))
  on lost Server: Client fails ServerGone
  on lost Client: Server closes
```

T3 Two-phase commit:

```
protocol Commit between Coord, Part[n]
  each p in Part
    Coord -> p: Prepare(tx: TxId) within 5s
    choice by p within 5s
      yes: p -> Coord: Yes
      no: p -> Coord: No
  choice by Coord
    commit when every p voted yes:
      Coord -> all Part: Commit
    abort:
      Coord -> all Part: Abort
  each p in Part
    p -> Coord: Ack within 5s
  on lost Coord: Part blocks
  on lost p in Part: Coord aborts
```

T4 Agent review:

```
protocol Review between Lead, Coder, Critic
  trust Coder, Critic untrusted
  budget rounds 3, tokens 200k
  Lead -> Coder: Task(spec: Spec)
  rec round
    Coder -> Critic: Draft(patch: Patch where patch.applies_to(spec)) within 10m
    choice by Critic within 5m
      approve:
        Critic -> Lead: Approve(patch: Patch where patch == round.patch)
        Lead -> Coder: Done
      revise:
        Critic -> Lead: Revise(notes: str where notes.len() <= 4kb)
        Lead -> Coder: Fix(notes: str)
        again round
  on lost Coder or Critic: Lead fails AgentGone
```

T5 uses the R634 endpoint API as in P0.

- Gains: the most honest; 2PC's blocking weakness is written down (`on lost Coord: Part blocks`);
  untrusted agents and servers are monitored; budgets for agent loops (rounds, tokens) are part of
  the contract.
- Costs: twice the size of the mix; most of it is boilerplate in the common case; refinements over
  history (`sha256_of(chunks.data)`) and `on lost` need core changes; mandatory deadlines everywhere
  would be obnoxious.
- Keep: `within d` (optional, lowering only), `where` refinements (core change, D4), `on lost` (core
  change, D7), monitors at trust boundaries (D8).

### P5 Parts: tooling, interop and local-first

Bet: most teams own one side of a conversation. Let them write the side they own as a named state
machine, check it against the other parts, and get state names that tools, error messages and
dashboards can use. A two-party protocol writes one part and gets the other with `dual`.

T1 Checkout:

```
part Client of Checkout
  start: send Start(cart: Cart) to Shop -> quoting
  quoting: recv Quote(total: Money) from Shop -> deciding
  deciding:
    send Pay(token: Token) to Shop -> paying
    send Cancel to Shop -> end
  paying:
    recv Receipt(id: OrderId) from Shop -> end
    recv Declined(reason: str) from Shop -> end

part Shop of Checkout = dual Client
```

T2 Download:

```
part Client of Download
  start: send Open(path: Path) to Server -> streaming
  streaming within 30s:
    recv Chunk(data: Bytes) from Server -> streaming
    recv Eof(sha256: Digest) from Server -> end

part Server of Download = dual Client
```

T3 Two-phase commit:

```
part Coord of Commit
  start: send Prepare(tx: TxId) to each Part -> collecting
  collecting: recv Yes | No from each Part -> deciding
  deciding:
    send Commit to each Part -> acking
    send Abort to each Part -> acking
  acking: recv Ack from each Part -> end

part Part of Commit
  start: recv Prepare(tx: TxId) from Coord -> voting
  voting:
    send Yes to Coord -> deciding
    send No to Coord -> deciding
  deciding:
    recv Commit from Coord -> acking
    recv Abort from Coord -> acking
  acking: send Ack to Coord -> end
```

T4 Agent review:

```
part Lead of Review
  start: send Task(spec: Spec) to Coder -> waiting
  waiting:
    recv Approve(patch: Patch) from Critic -> approving
    recv Revise(notes: str) from Critic -> revising
  approving: send Done to Coder -> end
  revising: send Fix(notes: str) to Coder -> waiting

part Coder of Review
  start: recv Task(spec: Spec) from Lead -> drafting
  drafting: send Draft(patch: Patch) to Critic -> waiting
  waiting:
    recv Done from Lead -> end
    recv Fix(notes: str) from Lead -> drafting

part Critic of Review
  start: recv Draft(patch: Patch) from Coder -> judging
  judging:
    send Approve(patch: Patch) to Lead -> end
    send Revise(notes: str) to Lead -> start max 3
```

T5 Implement Shop, handler style:

```
shop(db: Db) runs Checkout.Shop fail ShopErr uses db =
  on Start(cart) -> send Quote(total: db.price(cart)?)
  on Pay(token, amount) ->
    match db.charge(token, amount)
      Ok(id) -> send Receipt(id:)
      Err(e) -> send Declined(reason: e.text())
  on Cancel -> end
```

- Gains: named states are the best hooks for errors ("in state `waiting` you may receive `Approve`
  or `Revise`"), IDE hovers, logs and version diffs; familiar to `gen_statem` and XState users; works
  when the other side is foreign; `dual` makes two-party protocols short.
- Costs: no global view, so multiparty protocols are written n times and checked by composition
  (bottom-up MPST, which is harder to explain when it fails); the handler style in T5 loses locals
  between states (`total` is gone by the time `Pay` arrives, so the code above charges the client's
  `amount`, not the quoted `total`), which forces explicit state records.
- Keep: generated state names in every view; implementation in direct style, not handler style.

### P0 Baseline: what Yan has today

For comparison, R633 strings and R634 endpoints, as the spec text describes them (the exact R633
spelling of a recursion jump is my reading of "named recursion").

T1 to T4:

```
type Checkout = session.protocol("Client -> Shop: Start(Cart); Shop -> Client: Quote(Money); choice Client { Client -> Shop: Pay(Token); choice Shop { Shop -> Client: Receipt(OrderId) | Shop -> Client: Declined(str) } | Client -> Shop: Cancel() }; end")
```

```
type Download = session.protocol("Client -> Server: Open(Path); rec Chunks { choice Server { Server -> Client: Chunk(Bytes); Chunks | Server -> Client: Eof(Digest) } }; end")
```

```
type Commit2 = session.protocol("Coord -> P1: Prepare(TxId); choice P1 { P1 -> Coord: Yes() | P1 -> Coord: No() }; Coord -> P2: Prepare(TxId); choice P2 { P2 -> Coord: Yes() | P2 -> Coord: No() }; choice Coord { Coord -> P1: Commit(); Coord -> P2: Commit() | Coord -> P1: Abort(); Coord -> P2: Abort() }; P1 -> Coord: Ack(); P2 -> Coord: Ack(); end")
```

```
type Review = session.protocol("Lead -> Coder: Task(Spec); rec Round { Coder -> Critic: Draft(Patch); choice Critic { Critic -> Lead: Approve(Patch); Lead -> Coder: Done() | Critic -> Lead: Revise(str); Lead -> Coder: Fix(str); Round } }; end")
```

T5:

```
shop(ep: Checkout.Shop, db: Db) fail ShopErr uses db =
  let (cart, ep) = recv(ep)?
  let total = db.price(cart)?
  let ep = send(ep, Quote(total))?
  match recv(ep)?
    Pay(token, ep) ->
      match db.charge(token, total)
        Ok(id) -> close(send(ep, Receipt(id))?)
        Err(e) -> close(send(ep, Declined(e.text()))?)
    Cancel(ep) -> close(ep)
```

## 6. The mix: a consolidated proposal

Name for the framework: **rite, part, play**. A `protocol` is the rite (the agreed order); a function
that `plays` a role is one part; a `play` is the whole performance written once. One file can hold
all three, plus ordinary Yan.

### 6.1 Protocols (D1 to D4)

**D1 Directed choice as an indented sequence diagram.** A message line is `A -> B: Label(fields)`.
A choice is always made by the sender of the message that carries it, so a choice is a message line
without a colon, followed by one indented line per label, each with its own continuation. The
one-line form `A -> B: X | Y` is a choice whose branches all continue with what follows. `loop`
opens a recursion, `again` jumps back to the innermost loop, and a branch that ends without `again`
falls through to after the loop. `loop name` and `again name` name nested loops. `|` is the separator
because Yan sums use `|` (RFC 0002 wrote `or`). Fields are named, as in Yan records, and puns work
(`Quote(total:)`).

This removes the RFC's `choice by Client` plus branch names (`accept:` then `Pay`): the label is
the branch name, and the sender is the chooser, exactly as in the theory's `p -> q: {l_i(S_i). G_i}`.

**D2 Role families.** `between Coord, Part[n]` declares a family; `each p in Part` repeats a block
per member; `A -> all Part` sends one label to every member, in a fixed order, as a choice when
indented labels follow. With n known at stage 0 this unrolls (lowering only). With n chosen at run
time it needs parameterised protocols in the core (core change).

**D3 Deadlines.** `within d` after a message line or a choice header puts a deadline on the
receiver's wait. It lowers to `with timeout(d)` around the projected `recv`, so expiry is the
ordinary `fail Timeout` and cancels the session (lowering only). It is a local deadline, not the
clock constraints of timed MPST; those are an open question.

**D4 Payload refinements.** `where` after a message states a predicate over its fields and earlier
fields in the same session (`amount == total`). Checked statically where Yan's refinements can, and
at the receiving boundary otherwise. Needs protocol state that carries values (core change).

T1 Checkout:

```
protocol Checkout between Client, Shop
  Client -> Shop: Start(cart: Cart)
  Shop -> Client: Quote(total: Money)
  Client -> Shop
    Pay(token: Token, amount: Money) where amount == total
      Shop -> Client: Receipt(id: OrderId) | Declined(reason: str)
    Cancel
```

T2 Download:

```
protocol Download between Client, Server
  Client -> Server: Open(path: Path)
  loop
    Server -> Client within 30s
      Chunk(data: Bytes)
        again
      Eof(sha256: Digest)
```

T3 Two-phase commit:

```
protocol Commit between Coord, Part[n]
  each p in Part
    Coord -> p: Prepare(tx: TxId)
    p -> Coord: Yes | No
  Coord -> all Part
    Commit
    Abort
  each p in Part
    p -> Coord: Ack
```

T4 Agent review:

```
protocol Review between Lead, Coder, Critic
  Lead -> Coder: Task(spec: Spec)
  loop max 3
    Coder -> Critic: Draft(patch: Patch)
    Critic -> Lead
      Approve(patch: Patch)
        Lead -> Coder: Done
      Revise(notes: str)
        Lead -> Coder: Fix(notes: str)
        again
```

### 6.2 Parts: implementing one role (D5)

**D5 The `plays` clause.** A role is an ordinary Yan function with one more signature clause,
`plays P.Role`, next to `fail` and `uses`. The endpoint is implicit: `send Label(...)`,
`recv Label(binds)` and `match recv` act on it, and the checker tracks its state at every program
point (the same typestate check R634 already requires, applied to a hidden variable). The function
must reach the end state on every path that does not fail. `to Role` and `from Role` are needed only
where the current state allows more than one peer. Locals live across states, because this is
direct-style code, not handlers. It lowers to a function that takes the endpoint by `sink` and
threads it, with the generated state names of RFC 0002 (`CheckoutShop2`).

T5 Implement Shop:

```
shop(db: Db) plays Checkout.Shop fail ShopErr uses db =
  recv Start(cart)
  let total = db.price(cart)?
  send Quote(total:)
  match recv
    Pay(token, amount) ->
      match db.charge(token, amount)
        Ok(id) -> send Receipt(id:)
        Err(e) -> send Declined(reason: e.text())
    Cancel -> ()
```

Running a session (lowers to `session.connect` and one `par` branch per role):

```
pub main() fail AppErr uses io, db =
  let cart = io.read_cart()?
  let out = Checkout.run(Client: buy(cart), Shop: shop(db))?
  io.print("\(out.Client)")
```

### 6.3 Plays: writing the whole interaction once (D6)

**D6 `play`.** A choreography item that names the protocol it performs (`in Commit`). Statements and
`let` bindings are located with `@Role`; a message `A -> B: Label(name: expr)` evaluates `expr` at
A and binds `name` at B. A `match`, `if` or loop condition located at a role is a choice by that
role, and must be followed, in each branch, by messages that tell every role whose behaviour
differs. The compiler never inserts a hidden message: if a role cannot tell, the play is rejected
with the role and branches named, as in section 4.1. The play must conform to its declared
protocol; `yan view --as protocol` prints the protocol it implies. Each role projects to a `plays`
function with a generated name (N4). Lowering only.

T3 as a play (the decision rule now lives in code):

```
play commit(tx: TxId @Coord, store: Store @Part[n]) -> Decision @Coord
    fail StoreErr uses db @Part
  in Commit
  let votes @Coord = each p in Part
    Coord -> p: Prepare(tx:)
    p -> Coord: if store.prepare(tx).is_ok() @p then Yes else No
  let d @Coord = if votes.all(== Yes) then Commit else Abort
  Coord -> all Part: d
  each p in Part
    store.finish(tx, d)? @p
    p -> Coord: Ack
  d
```

T4 as a play, the agent loop:

```
play review(spec: Spec @Lead, coder: Model @Coder, critic: Model @Critic) -> Patch? @Lead
    uses llm @Coder, llm @Critic
  in Review
  Lead -> Coder: Task(spec:)
  var notes @Coder = ""
  loop max 3
    Coder -> Critic: Draft(patch: coder.write(spec, notes)?)
    match critic.review(patch)? @Critic
      Pass ->
        Critic -> Lead: Approve(patch:)
        Lead -> Coder: Done
        return Some(patch)
      Notes(n) ->
        Critic -> Lead: Revise(notes: n)
        Lead -> Coder: Fix(notes:)
        again
  None
```

### 6.4 Failure, trust and tools (D7 to D9)

**D7 Lost peers.** Today a lost peer is `SessionClosed` at the next `recv`, handled with Yan's
`else` or `?` in a `plays` function (lowering only). A protocol-level `on lost Role` block that
continues the session with the survivors, as crash-stop MPST allows, is a core change.

**D8 Monitors at trust boundaries.** When a role is bound to a peer that is not checked Yan code (a
socket, a browser, an LLM tool call), the endpoint is wrapped in a generated runtime monitor that
checks labels, order, deadlines and D4 refinements, and fails the session on the first violation.
This keeps capability safety honest at the edge. Std package, lowering only.

**D9 Views.** `yan view --as diagram` prints a Mermaid `sequenceDiagram` (D1 maps one to one:
arrow lines, `alt` for choices, `loop`); `yan view --as part Shop` prints the local type as a state
machine with generated or chosen state names, never as a tree (section 4.1, result 4). Error
messages name the state and the labels it allows, as R634's existing error already does.

### 6.5 How it stays inside the one core

| Item | Lowers to | Core change |
| --- | --- | --- |
| D1 protocol notation | `session.protocol` stage-0 type (R633) | no |
| D2 families, n at stage 0 | unrolled protocol | no |
| D2 families, n at run time | parameterised protocols | yes |
| D3 `within` | `with timeout` around projected `recv` (9.4) | no |
| D4 `where` | protocol state carrying values | yes |
| D5 `plays` | endpoint threaded by `sink` (R634) | no |
| D6 `play` | one `plays` function per role | no |
| D7 `on lost` blocks | crash-stop sessions | yes |
| D8 monitors | std wrapper over an endpoint | no |
| D9 views | printers | no |

Five facts (law 4, N5): `fail` and `uses` sit on `plays` functions and on `play` headers, per role
(`uses db @Part`); concurrency is the role list (`between`), and each role runs in its own `par`
branch; compile time is the protocol itself, a stage-0 type; mutation is plain Yan. Lexis is Yan's.

Opinions the mix takes, deliberately:

- The sender chooses. There is no separate choice syntax.
- No hidden messages. A broadcast is written `-> all`.
- Named fields, not positions.
- Trees never appear in tools; states do.
- A choreography never replaces a declared protocol between parties that do not share code.

## 7. Open questions

1. Is directed choice (D1) the right default, or should the RFC's `choice by Role` with branch
   names stay for readers who think in branches? Recommendation: D1, with branch names recovered in
   views from labels.
2. Keywords: `play` and `plays` read well and fit 礼; `rite` for the protocol item is more
   distinctive but less obvious. Recommendation: keep `protocol`.
3. Family syntax: `Part[n]` (chosen here, echoes shapes) or `many Part`.
4. Should state names be writable in the protocol (for example `as quoted` at the end of a line) so
   that logs, metrics and errors use human names instead of generated ones?
5. Clock constraints (timed MPST) beyond local deadlines: worth a core change, or is D3 enough?
6. Versioning: a `yan protocol diff` that reports breaking changes needs a decidable compatibility
   check; asynchronous subtyping is undecidable in general. Bounded checks, or a syntactic rule?
7. Monitors (D8) on LLM tool calls: should an agent's tool-use loop be declared as a protocol, so
   the reference agent loop gets the same checks?
8. Census polymorphism with run-time n (D2) and crash-stop (D7) both need core work; which first?
9. Overlap with Yan Workflow (`.ywf`) and Yan Delegate (`.ydl`): T4 is half workflow, half
   delegation. Where does the line sit?

## 8. Sources

- Honda, Yoshida, Carbone. Multiparty asynchronous session types. POPL 2008. (high)
- Scalas, Yoshida. Less is more: multiparty session types revisited. POPL 2019. (high)
- Lange, Tuosto, Yoshida. From communicating machines to graphical choreographies. POPL 2015. (medium)
- Lagaillardie, Neykova, Yoshida. Stay safe under panic: affine Rust programming with multiparty
  session types. ECOOP 2022. (medium)
- Cutner, Yoshida, Vassor. Deadlock-free asynchronous message reordering in Rust with multiparty
  session types (Rumpsteak). PPoPP 2022. (high)
- Zhou, Ferreira, Hu, Neykova, Yoshida. Statically verified refinements for multiparty protocols
  (Session*). OOPSLA 2020. (high)
- Hou, Lagaillardie, Yoshida. Fearless asynchronous communications with timed multiparty session
  protocols. ECOOP 2024. https://arxiv.org/abs/2406.19541 (high)
- Barwell, Hou, Yoshida, Zhou. Designing asynchronous multiparty protocols with crash-stop failures.
  ECOOP 2023. https://drops.dagstuhl.de/entities/document/10.4230/LIPIcs.ECOOP.2023.1 ; and
  Crash-stop failures in asynchronous multiparty session types, LMCS.
  https://lmcs.episciences.org/15526 (high)
- Giallorenzo, Montesi, Peressotti. Choral: object-oriented choreographic programming. TOPLAS 2024.
  https://arxiv.org/abs/2005.09520 (high)
- Shen, Kashiwa, Kuper. HasChor: functional choreographic programming for all. ICFP 2023. (high)
- Bates, Kashiwa, Jafri, Shen, Kuper, Near. Efficient, portable, census-polymorphic choreographic
  programming. 2025. https://www.arxiv.org/pdf/2412.02107 (high for claims, low for performance)
- Hirsch, Garg. Pirouette: higher-order typed functional choreographies. POPL 2022. (medium)
- Montesi. Introduction to Choreographies. Cambridge University Press, 2023. (medium)
- Tu, Liu, Song, Zhang. Understanding real-world concurrency bugs in Go. ASPLOS 2019.
  https://songlh.github.io/paper/go-study.pdf (high)
- Voinea, Gay. Benefits of session types for software development. PLATEAU 2016.
  https://bibtex.github.io/PLATEAU-2016-VoineaG.html (medium)
- Coblenz et al. Obsidian: typestate and assets for safer blockchain programming. TOPLAS 2020; and
  Can advanced type systems be usable? An empirical study of ownership, assets, and typestate in
  Obsidian. OOPSLA 2020. (medium)
- Desai et al. P: safe asynchronous event-driven programming. PLDI 2013; P at AWS.
  https://sos-vo.org/node/83860/ (medium)
- Newcombe et al. How Amazon Web Services uses formal methods. CACM 2015. (high)
- Ancona et al. Behavioral types in programming languages. Foundations and Trends in PL, 2016. (high)
- Bollig, Függer, Nowak. Provable coordination for LLM agents via message sequence charts. 2026.
  https://arxiv.org/pdf/2604.17612 (low)
- Li, Hou, Yoshida. Specification-guided synthesis of deadlock-free communication protocol
  refinements with large language models. 2026. https://arxiv.org/pdf/2607.27964 (low)
- Mermaid sequence diagrams, PlantUML, AsyncAPI, gRPC, Erlang `gen_statem`, XState: product
  documentation, for notation only. (high)
