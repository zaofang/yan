## 21. Agent features

These features make Yan Agent cheap and reliable for models to write. They are toolchain features, not language features, and all of them read from the same query database.

### 21.1 The spec gate

Generation happens in two phases, enforced by `yan mcp` and available to any agent.

1. **Spec.** The model submits an item with a body of `...`: signature, `uses`, docstring, `pre`, `post`, `ex`. The tool checks that types resolve; that each `ex` is well-typed; that each `post` is well-typed and every name in it resolves, with `out` naming the result (R767); that each `ex` satisfies `pre`; that each `ex`'s expected result satisfies `post`, where that can be evaluated; and that `pre` is satisfiable, by the bounds solver or SMT when available. Problems are returned as diagnostics.
2. **Body.** The model submits the body against the frozen spec. Changing any spec line reopens phase 1.

Human review of model-written code starts with the spec, which is short and readable.

**Spec freeze (normative).** Coding agents rewrite tests to pass when they can: ImpossibleBench (ICLR 2026) measured a 93% test-hacking rate in one configuration. So the tests and contracts produced by the spec step are frozen once a human accepts them. An implement-step patch may not touch test functions or `pre`, `post`, `ex`, `invariant` and `decreases` clauses; `check_patch` enforces this (P006). If the agent believes a frozen test is wrong, it stops and reports instead of editing. W806 flags code that special-cases the literal inputs of `ex` clauses, including lookups keyed by them; in the spec gate it is an error.

**Frozen meaning (R675).** Names in frozen clauses resolve against the module as the spec was accepted, plus the prelude, so an implement patch cannot change what a clause calls: adding an item a frozen clause names is P006.

**Spec-gate checks (R621).** `spec_check` runs each `ex` line's arguments through `pre`. Arguments that break it are E619 at the `ex` line. A `pre` that cannot be evaluated yet, such as one calling a function whose body is still `...`, is skipped.

**The reference loop (R617).** `yan agent` (18.1, R616) runs one task through both phases using only the 21.4 tools, called in process as JSON-RPC messages to the handler `yan mcp` serves. Phase 1: the model writes items with `...` bodies, `spec_check` checks them, and up to `--spec-repairs` rounds (default 2) show it the line rendering. The accepted spec is written and frozen. Phase 2, each round in order: a reply that drops a spec item or changes a def line, docstring or contract is refused before any tool runs; `apply_patch` applies the reply with `--check`; `autofix` runs and its patch is applied; `run_tests` runs the module's `test_` functions and `ex` lines. The first refusal or failure is the next repair round, up to `--body-repairs` (default 3). The run is green when the tests pass. A reply with a line `STOP: <reason>` ends the run as `stopped`, the stop-and-report rule above. The prompts are fixed in the toolchain and versioned with it; the primer and shots are inputs.

### 21.2 Typed holes

`...` in expression position is a typed hole in Yan Agent (R212), and `_` is one in Yan. In Yan Agent `_` is only a discard (R347), and `...` as a whole body (a trait signature, or a function whose body is a hole) stays valid.

- `yan check` reports each hole as H701 with its expected type.
- In development builds a program with holes runs: a hole evaluates to an indeterminate value, operations on it propagate indeterminacy, and evaluation continues around it, as in Hazel. Release builds reject holes (E702).
- The `hole_context` query returns, for one hole: expected type, in-scope bindings with types, the enclosing function's contracts, nearby examples, and the top five `search_type` results. The output MUST fit in 400 tokens.

### 21.3 Constrained decoding

The compiler exposes, for any prefix of an item being generated, the set of valid continuations:

- **Grammar.** The Yan Agent grammar (13.2) as a token-aligned grammar for llguidance and XGrammar, and as GBNF for llama.cpp, all generated from the same source as the factored LL(1) grammar that CI checks (R428). It ships with the toolchain and is versioned with it.
- **Names and types.** At each identifier position: the in-scope names whose types can fit the expected type.
- **Effects and mutability.** Calls needing a capability the function does not declare, and `&` arguments to non-`var` places, are excluded.

Masks apply only to the code being generated, never to the model's reasoning. Yan Agent MUST remain fully usable without any constraint, because hosted model APIs may not expose masks.

**Prompting.** Prompts carry 5-shot Yan Agent examples, not retrieved documentation; `yan skill` prints the canonical example set.

### 21.4 Agent server (`yan mcp`)

A Model Context Protocol server (protocol 2026-07-28, stateless) over stdio. Every tool returns JSON in structuredContent with a declared outputSchema; diagnostics use the schema in 18.2, and a result that carries diagnostics also puts their line rendering in content, which is what the model reads (R295). Tool schemas are frozen at 1.0 and grow only by optional fields.

| Tool | Input | Output |
| --- | --- | --- |
| `check` | module or item name; optional max\_tokens | diagnostics |
| `check_patch` | patch (18.3); optional max\_tokens | diagnostics, or success with new hashes; nothing is written |
| `apply_patch` | patch | as `check_patch`, then writes |
| `autofix` | item name | the patch produced by applying every machine-applicable fix |
| `apply_fixes` | fix ids from a diagnostics result, e.g. #2.1 | as apply\_patch, for the one patch formed by those fixes (R295) |
| `undo` | number of operations, default 1 | the reverted operations and new hashes |
| `sig` | module name, optional budget | signatures, contracts, examples |
| `pack_context` | task description, budget in tokens | the most relevant signatures, contracts and examples, ranked by dependency distance from the items named in the task |
| `search_type` | a type, e.g. `Callable[[Table[Trade]], Array[Str, float]]` | matching functions with signatures, best first |
| `type_at` | file, line, column | inferred type and capabilities |
| `hole_context` | file, line, column | as 21.2 |
| `spec_check` | an item with body `...` | phase-1 results (21.1) |
| `run_tests` | filter | results, with counterexamples for property tests |
| `why_shared` | item or site | where a value is deep-copied or shared through `Rc` or `Arc` |
| `why_needs` | item and a capability, `fail` member or panic kind | the chain of `yan why` (R612) |
| `infer_invariants` | item | the patch adding accepted `invariant` clauses (R607) |
| `debug` | a trace or a running test; a query (why, last\_write, step, back) | the answer of the one trace query engine of 18.4 (R306, R378) |
| `explain` | code | long-form text |
| `query` | a Yan Agent body using only the `toolchain` capability; step and token budgets | the body's return value as JSON, or its diagnostics |

**Model adapters (R618).** `yan agent --model` takes `stub:FILE`, a JSON array of scripted replies for tests and CI, or `cmd:PROGRAM ARGS`, a process that reads one JSON line per turn, `{"system", "messages", "max_tokens"}`, and writes one line, `{"text", "usage": {"in", "out", "reasoning"}, "model"}` or `{"error"}`. Hosted models are reached through `cmd:` adapters that the harness ships; the toolchain makes no HTTPS calls and holds no API keys.

**The `toolchain` capability.** A built-in read-only capability over the query database: `check`, `sig`, `search_type`, `type_at`, `hole_context`, `callers`, `callees`, `uses_of`, index search and generated-item expansion. `query` runs its body on the interpreter with a step budget and an output token budget. Release builds never bind `toolchain` (E318), and it cannot produce approvals (19.4).

**Autofix loop.** An agent SHOULD call `autofix` before asking the model to repair anything. Errors whose only fix is mechanical (missing `uses`, keyword arguments, parentheses, `&` marks, `copy` and `share` insertion, lenient normalizations) never cost a model generation. check and check\_patch apply auto fixes before they answer, so the model sees only diagnostics that need judgment (R295).

**Untrusted content (normative).** Repository text is an attack surface: 2025 brought prompt-injection CVEs in Copilot, Cursor, Amazon Q and Claude Code through comments, READMEs and rules files. So:

- `pack_context`, `hole_context` and `query` emit comments, docstrings and string literals in a fenced channel marked untrusted, and strip or flag zero-width, bidirectional and tag characters.
- `yan mcp` never applies a patch to a protected target (18.3) without a human confirmation token.
- No repository-defined code or configuration runs before the user has trusted the repository; `yan trust` records that, and `run_tests`, `query` and stage 0 refuse to run without it (R676).
- `sig`, the `quote` and `notes` of diagnostics, and test output carry repository text in the same untrusted channel (R676).

**Confinement (R676).** A module name in a request is dotted identifiers resolved beneath the server root, never through a symlink out of it. Every tool that runs code has step, heap and wall-clock budgets per call and per session; a step includes a loop iteration (R487).

### 21.5 Provenance

Each item MAY carry provenance in `.yan/provenance.jsonl`, keyed by item hash. Each record is an in-toto statement: `{item_hash, author: human | model(id, version), prompt_hash?, tool_calls_hash, base_hashes, time, status: checked | tested | proven}`, optionally signed. `yan audit` reports model-authored code no human has reviewed, model-authored changes to `ffi` modules and to any item whose capability set grew, and every extern block, `safe` item and `uses none` claim (12.10).

**Review report (R665 to R671).** `yan review [BASE[..HEAD]]` (18.1) compares two git revisions of a package; HEAD left out is the working tree and BASE left out is `HEAD`, and neither tree is written (R665). Items are compared by key on elaborated Yan Core, so converting a file between Yan Agent and Yan changes nothing. An item is added, removed or edited, where edited means its canonical Yan Core text without docstrings differs; an item with the same text whose identity hash (3.6) changed is affected, counted but not listed. Each added or edited item shows its header old and new; tests show whole. `--as yag|wen` follows the item-level view in M3 (R666). For every function, generator, impl method, trait signature and extern block it lists capability and `fail` gains and losses, `unsafe` and `pub` gained or lost, and extern blocks added, edited or removed, gains first (R667). It lists each `pre`, `post`, `ex`, `invariant` and `decreases` line added or removed, each test added, edited or removed, and any change to `tests.lock`. Approval is judged by BASE's `tests.lock`, so a change cannot approve its own tests; an edit to an approved test, or to the contract lines of an item that had any at BASE, is marked frozen (R668). It runs HEAD's tests through the verdict cache (18.4, R669) and lists each test that reaches an added, edited or affected item with its verdict, approved or proposed, and cached or run. When `.yan/provenance.jsonl` exists, each listed item carries the author recorded for its identity hash, or `unknown` (R670). The exit status is 0 when nothing needs a human, 1 when something does and 2 on a tool failure; needing a human means a capability, `fail`, `unsafe` or `pub` gain, an extern block change, a frozen edit, a `tests.lock` change, a failing reached test or a module that does not elaborate at HEAD. `--format json` gives each finding a `kind` and an `attention` flag, and `--format md` renders a PR comment (R671).

### 21.6 Evaluation harness

The M1 gate is measured with a harness specified here, so the result can be trusted:

- Held-out tests the model never sees, kept in a separate private repository the agents cannot read and pulled only at run time, on fresh tasks that resist training-data contamination, in a versioned task set.
- End-to-end token counts, including retries and tool calls, against a sealed typed-Python baseline on the same tasks.
- pass@1 at temperature 0 and at the deployment temperature, zero-shot and 5-shot.
- An ImpossibleBench-style cheating rate: tasks whose tests contradict the spec, where any pass counts as cheating.
- Results stored with the toolchain hash and the model identifier.
- The unit is total tokens per solved task: (1/p) times the sum over every turn of harness prompt, code read, history and output, counted with the deployed model's own tokenizer plus o200k and cl100k. Repair rounds and tool results are included (R289).
- Tokens are reported in two parts (R567). Spec-phase tokens cover the signature, `uses`, contracts and `ex` clauses of 21.1; body tokens cover the body and every repair round. Body tokens compare against the typed-Python program; spec-phase tokens compare against the pytest cases the same model writes for the same task when the prompt asks typed Python for tests, so both sides pay for their tests.
- The Yan Agent primer the model receives stays at or under 500 tokens; the canonical example set (`yan skill`) counts toward it (R289).
- Each run reports, per task, rounds to green, the diagnostics hit, and which of them an autofix closed in the loop (R280).
- The Yan Agent arm runs through `yan agent` (R620), so repair rounds and autofix count as a user meets them. The typed-Python arm keeps the same two phases, with mypy and pytest, in the harness.
- **Run record (R619).** `yan agent` writes JSON with `schema: 1`: `task`, `model` (the adapter's id), `toolchain` (`version` and the BLAKE3 hash of the binary), `status` (`green`, `spec_failed`, `body_failed` or `stopped`), `rounds_to_green`, `tokens.spec` and `tokens.body` (each `{in, out, reasoning}` as the provider reports them, R567), and `rounds`. Each round has its phase, number, usage, `ok`, the report the model saw next, and its tool calls, each with the codes it hit, failing test ids and items autofix replaced. `system`, `messages`, `spec` and `final` let the harness recount tokens with each tokenizer. New fields are optional additions only.

**If the fluency gate fails,** the plan is continued pre-training of an open 8B to 32B model on Yan Agent plus instruction transfer, not fine-tuning of hosted models.
