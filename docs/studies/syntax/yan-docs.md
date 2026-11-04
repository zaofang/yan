# Yan Docs (`.ydoc`): a syntax study

Status: brainstorm study, 2026-11-04. Not normative. No spec rows, no diagnostic codes. Items are
labelled D1, D2 for a later RFC. Where this page and `spec/` differ, `spec/` wins.

**One-line answer.** A `.ydoc` page should be *a module turned inside out*: prose is the body, code
is embedded, and three laws hold. Everything that looks like code is checked; the author writes
inputs and the toolchain writes outputs; a page declares its capabilities once and is otherwise
pure. Almost no new syntax is needed, because the one-core design already gives Yan what other doc
systems bolt on: surface tabs (R313), hash-pinned links (R311) and deterministic, cacheable runs
(R315).

## 1. Methodology and process

1. **Read the current state, narrowly.** `docs/surfaces.md` section 5 (status Partial, with a MOCK
   grammar), spec 18.9 (R310 to R319), the docs gate in 23, the watch-expression rule in 13, handler
   syntax in 15, and the naming rows R774 and R775. Nothing in `conformance/` or `tests/` was read
   or changed.
2. **Survey peers.** About thirty systems in five families (section 2), from primary docs where
   reachable. Fetched this session: Verso (Lean), Markdoc FAQ, Unison docs, Djot reports, llms.txt
   adoption data. The rest is from prior knowledge, marked with confidence.
3. **Survey research.** Documentation-failure studies (IBM survey, ICSE taxonomy), notebook studies
   (CHI 2018, MSR 2019), literate programming, prompt-format sensitivity in LLMs. Confidence marked
   per claim: **H** (read the primary source this session), **M** (well known, not re-read), **L**
   (secondary source or vendor blog).
4. **Fix the tasks.** Four tasks every proposal must express (section 4), so proposals are compared
   on the same text, not on their best case.
5. **Generate five proposals**, each pushed hard along one axis: brevity, safety, power, agent token
   cost, tooling and round trip. Each is allowed to be extreme; its costs are stated.
6. **Score and mix.** A trade-off table (section 6), then a consolidated design that keeps what won
   on each axis and drops what cost too much (section 7).
7. **Experiment on paper.** Every sample was checked for CommonMark behaviour by hand (how GitHub
   and a plain Markdown viewer would show it), since "degrades to readable Markdown" is a hard
   requirement found in section 3. No parser was built.

What was not done: no user study, no token measurement on the eval harness (R312 asks for one; see
open questions), no prototype.

## 2. Findings: peers and competitors

### 2.1 The landscape

| Family | Systems | What they get right | What goes wrong |
| --- | --- | --- | --- |
| Lightweight markup | CommonMark/GFM, Djot, AsciiDoc, reStructuredText, MyST | Markdown won: GitHub, every editor, every model reads it | CommonMark's emphasis, list and raw-HTML rules are hard to parse and surprise people; extensions fork it (H, Djot reports) |
| Declarative doc DSLs | Markdoc (Stripe), MyST directives, Verso roles | Tags are data: validated, transformable, no code runs in content | Another syntax to learn; `{% %}` noise (H, Markdoc FAQ) |
| Docs as programs | MDX, Scribble, Pollen, Typst, Verso | Real abstraction, computed content, one language | Content becomes code review; arbitrary code in docs is a security and churn problem (H, Markdoc FAQ) |
| Checked examples | Python doctest, Rust doctests, Elixir doctests, Go `Example` functions, Unison docs, OCaml `ppx_expect`, Rust `insta` | Examples that cannot rot | Hidden lines and per-block flags (Rust `ignore`, `no_run`, `#` lines) leak tooling into prose (M) |
| Notebooks and literate | Jupyter, marimo, Pluto.jl, Observable Framework, Livebook, Quarto, Org Babel, Knuth WEB, noweb, Literate Haskell, Glamorous Toolkit | Narrative plus live results | Hidden state, out-of-order runs, JSON files that do not diff; literate programming never spread beyond niches (H for notebooks, M for literate) |

### 2.2 Systems that matter most for Yan

- **Unison** (H). Docs are a `Doc` value in the language, written `{{ ... }}`. Inline code in double
  backticks is typechecked and follows renames; a fenced block shows source and its evaluated
  result; `@source{f}` and `@signature{f}` embed an item. Closest peer: Unison is content-addressed,
  as Yan's item hashes are. Lesson: embedding items by reference, not by copy, is what stops rot.
- **Verso** (H, partial). Lean's doc tool: one markup close to Markdown, many *genres* (manual, blog,
  website, literate program), roles written `{name args}[content]`, Lean code elaborated by the
  compiler, accurate highlighting from the elaborator rather than regexes. Lesson: one markup, many
  genres; extension through the host language, checked like code.
- **Markdoc** (H). Stripe chose "docs as data" over MDX's "docs as code": a declarative tag syntax,
  no JavaScript in content, validation and static analysis. Lesson: for a capability-safe language,
  docs must not be a back door for code that runs unchecked.
- **Djot** (H). MacFarlane's redesign of Markdown: easier parse, attributes on any element, native
  divs. It has not displaced Markdown; users report the stricter list rules as a non-starter.
  Lesson: a stricter Markdown buys parse sanity at a real adoption cost.
- **Go examples and OCaml expect tests** (M). The expected output sits next to the code, written by
  a tool (`dune promote`, `cargo insta review`) and checked in CI. Lesson: "generated, never
  authored" (R315) does not have to mean "absent from the source".
- **marimo and Pluto** (M). Reactive notebooks stored as plain source, re-run as a dependency graph,
  so no hidden state. Lesson: the notebook problems are interactive problems; a batch build that
  always runs top to bottom from fresh state does not have them.
- **Typst** (M). Markup, code and math modes with a single `#` escape. Lesson: one escape character
  is enough for computed content.
- **Diataxis** (M). Four kinds of page: tutorial, how-to, reference, explanation. Widely adopted
  (Django, Canonical, NumPy). Lesson: page kind predicts what checks make sense.
- **llms.txt** (L). A Markdown index for agents. Adoption is wide but reads by big-lab crawlers are
  rare; coding agents are the real consumers. Lesson: the agent view matters for agents already
  pointed at the docs, not for discovery. R312 already emits it.

### 2.3 What users want, and what the research says

| Finding | Source | Conf. | Consequence for `.ydoc` |
| --- | --- | --- | --- |
| The three most severe API-doc problems are ambiguity, incompleteness and incorrectness; obsolete and unexplained examples also block work | Uddin and Robillard, IEEE Software 2015, 323 IBM developers | H | Embed items and constants by reference; show outputs; check every example |
| Doc issues span content (incorrect, incomplete, outdated) and process; 878 artifacts analysed | Aghajani et al., ICSE 2019 | M | Lints matter as much as syntax |
| 1.16M notebooks: 24% ran without error, 4% reproduced their results; top causes missing imports, NameError from out-of-order state, missing files | Pimentel et al., MSR 2019 | H | Declared capabilities, seeded fixtures, fresh top-to-bottom runs |
| 27.6% of 1.23M notebooks had no text; notebooks are "scripts with loose notes", and cleaning them for an audience is tedious | Rule, Tabard and Hollan, CHI 2018 | H | Separate the exploring tool (playground) from the explaining artifact (`.ydoc`) |
| Prompt format alone moved GPT-3.5 results by up to 40%; GPT-4 far less sensitive | He et al., 2024 (arXiv 2411.10541) | H | Keep the agent view in plain Markdown, the format models see most; measure, do not guess |
| Literate programming produces excellent artifacts (TeX) but did not spread; tangle and weave tooling is the cost | Knuth 1984; Ramsey 1994 (noweb) | M | Get literate modules from projection (one core), not from a tangle step |
| Raw HTML in Markdown is the main XSS vector for hosted docs | common practice (GitHub sanitises) | M | The registry hosts every package's docs (R318): ban raw HTML |

**Summary of findings.** Rot and ambiguity are the enemies, not missing features. Every system that
beats rot does it by *reference* (Unison, Verso) or by *generated output checked in CI* (Go, expect
tests). Every system that adds power by running code in content pays in review and security
(MDX). Every stricter Markdown pays in adoption (Djot). Yan's one core already supplies surface
tabs, hash-pinned links and deterministic runs, so the design problem is mostly subtraction.

## 3. Requirements distilled

1. Degrades to readable CommonMark on GitHub and in any editor (adoption; Djot lesson).
2. No code runs except fences and pure constants; no raw HTML (capability safety; Markdoc lesson).
3. Every code-looking thing is checked or explicitly `text` (rot; Uddin and Robillard).
4. Outputs are generated, never authored (R315), and visible to a reader of the raw file.
5. A page is reproducible: same inputs, same bytes (R318, determinism).
6. One mechanism per job; no per-fence flags or hidden lines (R310 already says so).
7. Cheap for agents to read and write (R312, R567).

## 4. The four shared tasks

- **T1 Tutorial step.** Read `app.log` from fixtures, count its lines, show what it printed.
- **T2 Handler.** Fetch a quote under a fake network handler and show the deterministic result.
- **T3 Reference fragment.** Embed the signature of `std.fs.lines`, state a constant that must not
  rot (the default line limit), add a warning callout and a small table.
- **T4 Expected failure.** Show that calling the network from a page that did not declare `net`
  fails to compile, as a lesson in capability safety.

Names such as `fs.DEFAULT_MAX_LINE`, `std.fs.Err.LineTooLong` and `quote` are illustrative, not std items.

## 5. Five proposals

Samples below show `.ydoc` source. Inner fences use `~~~` so this page never nests backtick fences.

### P1 Brevity: "Just Markdown"

Axis: fewest new things. A `.ydoc` is an R316 guide page with a different extension. No header
fence: capabilities are inferred as the union of what the fences use, and reported on the page.
Outputs appear only in the rendered site.

```
# Reading files

Open a rooted handle with [std.fs.root] and count lines with [std.fs.lines].

~~~
let logs = fs.root("./logs")
io.print(fs.lines(logs / "app.log")?.count())
~~~

## With a fake network

~~~
with net = fake_net({"/A": "10.0"})
  io.print(quote("A")?)
~~~

## Reference

[std.fs.lines] reads one line at a time. Lines longer than 64 KiB fail.

> **Warning.** A rooted handle cannot reach `..`.

| Call | Reads |
| --- | --- |
| `fs.lines` | one line |
| `fs.read_text` | whole file |
```

- T4 cannot be expressed: with inferred capabilities there is no undeclared one to violate, and
  without flags there is no way to say "this must fail".
- The "64 KiB" in prose will rot.
- Gains: zero learning, perfect GitHub rendering. Costs: requirements 3, 4 and 5 are weak; inferred
  capabilities hide what a page touches, which is the opposite of the capability model.

### P2 Safety: "Sealed Page"

Axis: explicitness and least authority. A mandatory header fence declares capabilities, fixtures and
a seed. Each fence may narrow capabilities further. Links that do not resolve are errors, not lints.
Raw HTML is an error. Outputs are written into the source by the toolchain and locked by hash.

```
~~~page
uses fs, io, net
fixtures "docs/fixtures/logs"
seed 7
~~~

# Reading files

~~~yn uses fs, io
let logs = fs.root("./logs")
io.print(fs.lines(logs / "app.log")?.count())
~~~
~~~out 3f9a1c
3
~~~

~~~yn uses net, io
with net = fake_net({"/A": "10.0"})
  io.print(quote("A")?)
~~~
~~~out 77b2e0
Quote(sym: "A", px: 10.0)
~~~

~~~yn uses io expect-error
http.get_text("https://px.example/A")?
~~~
```

- T3 as in P1, but the constant still rots.
- Gains: auditability; every capability visible per fence; outputs visible in raw source.
- Costs: per-fence `uses` and `expect-error` are exactly the per-fence flags R310 rejects; hashes in
  the source are noise and churn on every toolchain bump; the header repeats what the build could
  enforce. Obnoxious for a reader of the raw file.

### P3 Power: "Live Page"

Axis: expressiveness. Prose can compute. One escape, Yan's own interpolation `\(expr)`, works in
prose; `![item]` embeds an item card (as an image embeds a picture); directives are Yan calls
written `:: name(args)` on their own line, resolved against a doc library (`std.doc`), like Verso
roles or Typst functions.

```
~~~page
uses fs, io, net
~~~

# Reading files

~~~
let logs = fs.root("./logs")
let n = fs.lines(logs / "app.log")?.count()
~~~

The log has \(n) lines.

## Reference

![std.fs.lines]

Lines longer than \(fs.DEFAULT_MAX_LINE.human()) fail.

:: warning("A rooted handle cannot reach `..`.")

:: table(std.fs.readers(), cols: [.name, .reads])

:: tabs(yn: "fs.lines(p)", yag: "fs.lines(p)")
```

- T2 as in P2 without hashes. T4: `:: must_fail(...)` directive wrapping a fence.
- Gains: nothing rots; tables come from data; the doc library is ordinary Yan, so extensions are
  checked code.
- Costs: prose interpolation of runtime values (`\(n)`) ties prose to execution and makes review
  harder (the MDX problem, in a safer form). `:: tabs` duplicates what R313 already gives for free.
  Directive calls are a second language inside the first. On GitHub, `\(n)` shows as `(n)`.

### P4 Agent token cost: "Dense Page"

Axis: cheapest for a model to read and write. The page is an outline of claims. Rules: the first
sentence of every section is its summary; claims are backed by `ex` lines inline in prose (the same
`ex` syntax items use); the agent view is the headings, the first sentences, the `ex` lines and the
fences, nothing else.

```
~~~page
uses fs, io, net
~~~

# Reading files

[std.fs.lines] streams a file one line at a time.
ex fs.lines(fs.root("./logs") / "app.log")?.count() == 3

## Fake network

A handler replaces `net` for one block.
ex (with net = fake_net({"/A": "10.0"}) quote("A")?.px) == 10.0

## Limits

Lines over the limit fail.
ex fs.DEFAULT_MAX_LINE == 64KiB

> [!WARNING]
> A rooted handle cannot reach `..`.
```

- T4: `ex fails compile(...)` is not expressible as an `ex` line without a new form.
- Gains: tiny agent view; every claim checked; `ex` reuses contract syntax people already know.
- Costs: `ex` lines in prose read as code mixed into sentences on GitHub; tutorials need printed
  output and multi-line steps, which `ex` lines do not give; "first sentence is the summary" is good
  discipline but turns into a lint war if enforced hard.

### P5 Tooling and round trip: "Inverted Module"

Axis: one artifact, every view. A `.ydoc` that defines items *is a module*: its fences are the
module's items in order, its prose is their `Doc` trivia (hash-neutral, R310). So
`yan view --as ydoc mod.yn` weaves any module into a page, and a page compiles as a module with
the same item hashes. Literate programming with no tangle step, because tangle and weave are just
two surfaces of one core. Fences run top to bottom from fresh state; the playground re-runs from
the edited fence down (marimo behaviour, no hidden state).

```
~~~page
uses fs, io
~~~

# Counting log lines

A log is a rooted directory; nothing outside it is reachable.

~~~
pub count_lines(dir: fs.Root, name: str) -> int fail fs.Err uses fs =
  fs.lines(dir / name)?.count()
~~~

Run it on the fixture log:

~~~
io.print(count_lines(fs.root("./logs"), "app.log")?)
~~~
```

- The `pub` item is a real item of module `docs.counting_log_lines`, importable by tests.
- Gains: the doc and the code cannot drift; any module gets a literate view for free; formatter,
  language server and hashing are reused unchanged.
- Costs: "is this file a guide or a module?" needs one rule; pages that only illustrate (most
  tutorials) gain nothing; a guide becoming a dependency of code inverts the usual direction.

## 6. Trade-off table

Scores 1 to 5, higher is better, judged against section 3.

| Axis | P1 Brevity | P2 Safety | P3 Power | P4 Dense | P5 Inverted |
| --- | --- | --- | --- | --- | --- |
| Readable as plain Markdown | 5 | 2 | 3 | 3 | 5 |
| Resists rot | 2 | 3 | 5 | 4 | 5 |
| Capability visibility | 1 | 5 | 3 | 3 | 3 |
| New syntax to learn (less is better) | 5 | 2 | 1 | 3 | 4 |
| Agent read and write cost | 4 | 2 | 2 | 5 | 4 |
| Expresses all four tasks | 2 | 4 | 4 | 3 | 3 |
| Fits R310's "no flags" | 5 | 1 | 3 | 4 | 5 |

Nothing wins outright. P1 and P5 win on fit and readability, P3 on rot, P2 on visibility, P4 on agent
cost. The mix takes one idea from each where its cost is low.

## 7. The mix: consolidated proposal

### 7.1 Three laws

1. **Checked page.** Every fence is compiled and run (R315); every item link resolves (R311);
   anything code-like that should not be checked is tagged `text`.
2. **Generated output.** The author writes inputs and the *kind* of an expected result; the
   toolchain writes the result into the file and CI fails if it is stale or hand-edited.
3. **Bounded page.** A page declares its capabilities once; a page without a declaration is pure.
   Pages run top to bottom from fresh state under the R315 handlers, seeded from `docs/fixtures/`.

### 7.2 Syntax, in full

A `.ydoc` file is CommonMark plus GFM tables with the R310 additions, and these rules:

```
page     = [header] {block}
header   = "~~~page" NL ["uses" cap {"," cap} NL] ["surface" EXT NL] ["kind" KIND NL] "~~~" NL
block    = commonmark-block | fence [result] | embed | alert | tagged
fence    = "~~~" [EXT ["!"] | "text"] NL code "~~~"
result   = "~~~" ("out" | "error") NL generated "~~~"
embed    = "![" path "]"                          -- alone in a paragraph
alert    = "> [!" ("NOTE" | "TIP" | "WARNING") "]" NL {"> " line}
inline   = ... | "\(" const-expr ")"              -- constant expressions only
KIND     = "tutorial" | "howto" | "reference" | "explanation"
```

Taken from each proposal:

- **From P1:** no new block syntax beyond what GitHub already renders. Title is the H1 (no `title`
  field); order comes from `docs/index.ydoc` (R316), not from each page. Untagged fences take the
  header's surface, default `yn`.
- **From P2:** capabilities are declared, not inferred, and the header is the only place they
  appear. Raw HTML is not a block: it renders as text, so registry-hosted docs (R318) cannot carry
  script. Outputs live in the source, but without hashes: the build cache (R315) already knows
  whether a result is stale.
- **From P3:** `![item]` embeds an item card (signature, `uses`, `fail`, summary, `ex`), rendered
  from the check database, so it cannot rot. `\(expr)` in prose is allowed for *constant*
  expressions only (compile-time evaluable, `uses none`), so prose can state a limit without
  copying it. No directive language: callouts use GitHub alerts, tabs come from R313.
- **From P4:** the first sentence of a page and of each section is its summary; the agent view of a
  page is headings, those sentences, fences and results. A lint, off by default, flags long first
  sentences.
- **From P5:** `pub` items in a page make it a literate module, with the same hashes as the woven
  `.yn` (later phase, D8). Playground edits re-run from the edited fence down.

Result blocks remove the need for flags. An empty `~~~error` block after a fence is the author
saying "this must fail to compile or must panic"; the toolchain fills in the diagnostic. An `~~~out`
block is always written by the toolchain; the author never types one. A fence whose body contains
`...` is an exercise, graded by its `ex` clauses (R319). No other block kinds.

### 7.3 The four tasks in the mix

```
~~~page
uses fs, io
kind tutorial
~~~

# Reading files

A rooted handle reads files under one directory and nowhere else.

Open the fixture directory with [std.fs.root] and count lines with [std.fs.lines]:

~~~
let logs = fs.root("./logs")
io.print(fs.lines(logs / "app.log")?.count())
~~~
~~~out
3
~~~

## Faking the network

A handler replaces `net` for one block, so the page stays deterministic.

~~~
with net = fake_net({"/A": "10.0"})
  io.print(quote("A")?.px)
~~~
~~~out
10.0
~~~

This page did not declare `net`, so the real network is out of reach:

~~~
http.get_text("https://px.example/A")?
~~~
~~~error
capability `net` is not available here: this page declares `uses fs, io`
~~~

## Reference

![std.fs.lines]

Lines longer than \(fs.DEFAULT_MAX_LINE) fail with [std.fs.Err.LineTooLong].

> [!WARNING]
> A rooted handle cannot reach `..`, even through a symbolic link.

| Call | Reads | Fails on |
| --- | --- | --- |
| [std.fs.lines] | one line at a time | long lines |
| [std.fs.read_text] | the whole file | invalid UTF-8 |

Uses:
- `fs` under `./logs` only.
```

On GitHub this renders as an ordinary page: code blocks, a real warning alert, a table, a literal
`![std.fs.lines]` and `(fs.DEFAULT_MAX_LINE)`. On the site, the card and the constant are filled in,
every fence switches surface with the R313 toggle, and the result blocks are marked checked.

### 7.4 Why this is elegant, not just small

- **Subtraction through the core.** Tabs per language, link checking, reproducible outputs and
  cached builds cost nothing new: they are R311, R313 and R315 seen from a page.
- **One escape, already known.** `\(expr)` is Yan's string interpolation; inside a page it means
  the same thing, limited to constants.
- **Results, not flags.** What a fence should do is stated by what follows it, and the machine
  fills it in. The author's intent is visible in the raw file; the output is never typed.
- **Embedding is to items what images are to pictures.** `![x]` already means "embed x" in
  Markdown.
- **Literate for free.** Weave and tangle are views, so a literate module is a surface choice, not
  a toolchain.

### 7.5 Proposed items for a later RFC

| Item | Proposal | Depends on |
| --- | --- | --- |
| D1 | `.ydoc` replaces `docs/*.md` for guide pages; `docs/index.ydoc` gives order | R316 |
| D2 | Header fence `~~~page` with `uses`, `surface`, `kind`; title from H1 | R316 |
| D3 | Result blocks `~~~out` (tool-written) and `~~~error` (author marks, tool fills); `yan doc --bless` writes them, `yan test` fails when stale | R315 |
| D4 | `![path]` item card embed | R311, R314 |
| D5 | `\(expr)` in prose for constant expressions only | const evaluation in 10 |
| D6 | No raw HTML in doc comments or pages; it renders as text | R310, R318 |
| D7 | GitHub alerts (`NOTE`, `TIP`, `WARNING`) as the only callouts | R310 |
| D8 | Literate modules: `pub` items in a page form a module with identical hashes; `yan view --as ydoc` | R310, 16 (projection) |
| D9 | Agent view of pages: headings, first sentences, fences, results | R312 |
| D10 | `kind` lints: tutorial fences share scope and end in a complete program; explanation pages may be fence-free | Diataxis |

D1 to D7 are small and fit M1's doc work. D8 is the vanguard item and should wait for M1. D9 needs
the R312 token measurement. D10 is optional.

## 8. Ideas tried and dropped

- **Reactive DAG scope instead of ordered scope** (marimo). Dropped for the batch build: fresh
  top-to-bottom runs already have no hidden state. Kept only as playground behaviour.
- **Per-fence capabilities** (P2). More authority detail than readers use; the page header is the
  right unit, and splitting a page is the fix when one fence needs more.
- **A directive language** (P3, Markdoc, MyST). Every needed directive was either already free
  (tabs) or a GitHub alert. Add one only when a real page needs it.
- **Hashes beside outputs** (P2). The cache knows; the file does not need to.
- **YAML front matter.** A second data language in a Yan file, with its own quoting traps. The
  header fence uses Yan's own `uses` spelling.
- **Runtime values in prose** (`\(n)` in P3). Ties prose to a run; kept for constants only.
- **Stricter Markdown** (Djot-style). Parse sanity is real, but GitHub rendering is the adoption
  path; lint the bad cases instead.

## 9. Open questions

1. **Stale fence tags in R310.** 18.9 still lists `xin`, `wen`, `yan` and `wen!`; after R774 they
   should be `yag`, `yn` (and `yir`, `ymt`). This is a spec fix needed regardless of this study.
2. **Fence character.** This study uses `~~~` so pages can quote Markdown with backticks (the
   problem PRs #180 and #181 hit). Should `yan fmt` make `~~~` canonical in `.ydoc` while doc
   comments keep backticks?
3. **Output in source** (D3). R315 says outputs are generated, never authored; writing them into the
   file by tool meets that reading, but the lead should confirm it is the intended one.
4. **Constant interpolation** (D5) changes how a GitHub viewer reads the prose (`\(x)` shows as
   `(x)`). Acceptable, or should constants only appear in item cards?
5. **Math.** GitHub renders `$...$`; scientific users (units, grids in 11) may want it. In or out?
6. **Diagrams.** Mermaid fences are popular and render on GitHub; they are `text` today. Worth a
   tag?
7. **Untrusted prose.** Dependency docs reach agents through the 21.4 untrusted channel; should a
   page's own prose also be marked untrusted when it comes from a third-party package? (Open item
   "untrusted-text marking" in project state.)
8. **Measurement.** The agent-view layout (D9) should be chosen by the R312 token measurement, not
   by this study.

## Sources

- Uddin, G. and Robillard, M. P. "How API Documentation Fails." IEEE Software, 2015.
  https://www.cs.mcgill.ca/~martin/papers/ieeesw2015.pdf
- Aghajani, E. et al. "Software Documentation Issues Unveiled." ICSE 2019.
  https://neverworkintheory.org/2021/10/06/software-documentation-issues-unveiled.html
- Pimentel, J. F. et al. "A Large-scale Study about Quality and Reproducibility of Jupyter
  Notebooks." MSR 2019. https://leomurta.github.io/papers/pimentel2019a.pdf
- Rule, A., Tabard, A. and Hollan, J. "Exploration and Explanation in Computational Notebooks." CHI
  2018. https://par.nsf.gov/servlets/purl/10064423
- He, J. et al. "Does Prompt Formatting Have Any Impact on LLM Performance?" 2024.
  https://arxiv.org/abs/2411.10541
- Markdoc FAQ. https://markdoc.dev/docs/faq
- Unison documentation. https://unison-lang.org/docs/usage-topics/documentation
- Verso genres. https://verso.lean-lang.org/doc/latest/Genres/ and
  https://github.com/leanprover/verso
- Djot in practice. https://www.jonashietala.se/blog/2024/02/02/blogging_in_djot_instead_of_markdown/
- llms.txt adoption (secondary, L). https://codersera.com/blog/llms-txt-complete-guide-2026/
- Knuth, D. E. "Literate Programming." The Computer Journal, 1984. Ramsey, N. "Literate
  Programming Simplified." IEEE Software, 1994. (M, not re-read)
- Diataxis. https://diataxis.fr (M, not re-read)
