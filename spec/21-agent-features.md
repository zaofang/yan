## 21. Agent features

These features make Xin cheap and reliable for models to write. They are toolchain features, not language features, and all of them read from the same query database.

### 21.1 The spec gate

Generation happens in two phases, enforced by `yong mcp` and available to any agent.

1. **Spec.** The model submits an item with a body of `...`: signature, `uses`, docstring, `pre`, `post`, `ex`. The tool checks that types resolve; that each `ex` is well-typed; that each `ex` satisfies `pre`; that each `ex`'s expected result satisfies `post`, where that can be evaluated; and that `pre` is satisfiable, by the bounds solver or SMT when available. Problems are returned as diagnostics.
2. **Body.** The model submits the body against the frozen spec. Changing any spec line reopens phase 1.

Human review of model-written code starts with the spec, which is short and readable.

**Spec freeze (normative).** Coding agents rewrite tests to pass when they can: ImpossibleBench (ICLR 2026) measured a 93% test-hacking rate in one configuration. So the tests and contracts produced by the spec step are frozen once a human accepts them. An implement-step patch may not touch test functions or `pre`, `post`, `ex`, `invariant` and `decreases` clauses; `check_patch` enforces this (P006). If the agent believes a frozen test is wrong, it stops and reports instead of editing. W806 flags code that special-cases the literal inputs of `ex` clauses.

### 21.2 Typed holes

`...` in expression position is a typed hole in Xin (R212), and `_` is one in Wen. In Xin `_` is only a discard (R347), and `...` as a whole body (a trait signature, or a function whose body is a hole) stays valid.

- `yong check` reports each hole as H701 with its expected type.
- In development builds a program with holes runs: a hole evaluates to an indeterminate value, operations on it propagate indeterminacy, and evaluation continues around it, as in Hazel. Release builds reject holes (E702).
- The `hole_context` query returns, for one hole: expected type, in-scope bindings with types, the enclosing function's contracts, nearby examples, and the top five `search_type` results. The output MUST fit in 400 tokens.

### 21.3 Constrained decoding

The compiler exposes, for any prefix of an item being generated, the set of valid continuations:

- **Grammar.** The Xin grammar (13.2) as a token-aligned grammar for llguidance and XGrammar, and as GBNF for llama.cpp, all generated from the same source as the factored LL(1) grammar that CI checks (R428). It ships with the toolchain and is versioned with it.
- **Names and types.** At each identifier position: the in-scope names whose types can fit the expected type.
- **Effects and mutability.** Calls needing a capability the function does not declare, and `&` arguments to non-`var` places, are excluded.

Masks apply only to the code being generated, never to the model's reasoning. Xin MUST remain fully usable without any constraint, because hosted model APIs may not expose masks.

**Prompting.** Prompts carry 5-shot Xin examples, not retrieved documentation; `yong skill` prints the canonical example set.

### 21.4 Agent server (`yong mcp`)

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
| `debug` | a trace or a running test; a query (why, last\_write, step, back) | the answer of the one trace query engine of 18.4 (R306, R378) |
| `explain` | code | long-form text |
| `query` | a Xin body using only the `toolchain` capability; step and token budgets | the body's return value as JSON, or its diagnostics |

**The `toolchain` capability.** A built-in read-only capability over the query database: `check`, `sig`, `search_type`, `type_at`, `hole_context`, `callers`, `callees`, `uses_of`, index search and generated-item expansion. `query` runs its body on the interpreter with a step budget and an output token budget. Release builds never bind `toolchain` (E318), and it cannot produce approvals (19.4).

**Autofix loop.** An agent SHOULD call `autofix` before asking the model to repair anything. Errors whose only fix is mechanical (missing `uses`, keyword arguments, parentheses, `&` marks, `copy` and `share` insertion, lenient normalizations) never cost a model generation. check and check\_patch apply auto fixes before they answer, so the model sees only diagnostics that need judgment (R295).

**Untrusted content (normative).** Repository text is an attack surface: 2025 brought prompt-injection CVEs in Copilot, Cursor, Amazon Q and Claude Code through comments, READMEs and rules files. So:

- `pack_context`, `hole_context` and `query` emit comments, docstrings and string literals in a fenced channel marked untrusted, and strip or flag zero-width, bidirectional and tag characters.
- `yong mcp` never applies a patch to a protected target (18.3) without a human confirmation token.
- No repository-defined code or configuration runs before the user has trusted the repository.

### 21.5 Provenance

Each item MAY carry provenance in `.yong/provenance.jsonl`, keyed by item hash. Each record is an in-toto statement: `{item_hash, author: human | model(id, version), prompt_hash?, tool_calls_hash, base_hashes, time, status: checked | tested | proven}`, optionally signed. `yong audit` reports model-authored code no human has reviewed, model-authored changes to `ffi` modules and to any item whose capability set grew, and every extern block, `safe` item and `uses none` claim (12.10).

### 21.6 Evaluation harness

The M1 gate is measured with a harness specified here, so the result can be trusted:

- Held-out tests the model never sees, kept in a separate private repository the agents cannot read and pulled only at run time, on fresh tasks that resist training-data contamination, in a versioned task set.
- End-to-end token counts, including retries and tool calls, against a sealed typed-Python baseline on the same tasks.
- pass@1 at temperature 0 and at the deployment temperature, zero-shot and 5-shot.
- An ImpossibleBench-style cheating rate: tasks whose tests contradict the spec, where any pass counts as cheating.
- Results stored with the toolchain hash and the model identifier.
- The unit is total tokens per solved task: (1/p) times the sum over every turn of harness prompt, code read, history and output, counted with the deployed model's own tokenizer plus o200k and cl100k. Repair rounds and tool results are included (R289).
- Tokens are reported in two parts (R567). Spec-phase tokens cover the signature, `uses`, contracts and `ex` clauses of 21.1; body tokens cover the body and every repair round. Body tokens compare against the typed-Python program; spec-phase tokens compare against the pytest cases the same model writes for the same task when the prompt asks typed Python for tests, so both sides pay for their tests.
- The Xin primer the model receives stays at or under 500 tokens; the canonical example set (`yong skill`) counts toward it (R289).
- Each run reports, per task, rounds to green, the diagnostics hit, and which of them an autofix closed in the loop (R280).

**If the fluency gate fails,** the plan is continued pre-training of an open 8B to 32B model on Xin plus instruction transfer, not fine-tuning of hosted models.
