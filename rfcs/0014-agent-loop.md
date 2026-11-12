# 0014: The reference agent loop

- Status: accepted (the lead chose "Accept all (D1 to D6)" on 2026-11-01)
- Research: Yan product study, shortlist item 1
- Rows: R616 to R621

## Summary

Sections 21.1, 21.4 and 21.6 describe a spec gate, an agent server and an evaluation harness. Nothing
yet runs a model through them. Without that, the M1 gate harness and the Yan Agent probe each write their
own loop, and neither one uses the gate or autofix. This RFC adds `yan agent`. It runs one task
through both phases of 21.1 using only the `yan mcp` tools, and it writes a run record that 21.6
reads. Both harnesses then drive the Yan Agent arm through it.

The loop also needs things the spec already asks for but main lacks. These are implementation work,
not decisions, and they land in the same PR:
- holes in statement position (21.2), so a spec with `...` bodies checks and runs;
- `ex` lines run as tests (R309);
- the `spec_check` and `run_tests` MCP tools (21.4).

## Decisions

### D1: `yan agent`, an M1 command

`yan agent <task.json> --model M [--primer f] [--shots dir] [--dir d] [--module m]
[--spec-repairs 2] [--body-repairs 3] [--out run.json]` runs one task and prints its run record. The
exit status is 0 when the task ends green, 1 when it does not, and 2 when the tool itself fails. It
goes into the 18.1 table at M1.

The loop calls the MCP tools in process, as JSON-RPC messages to the same handler that `yan mcp`
serves. Every step therefore goes through the tool schemas that 1.0 freezes. An external agent that
does the same steps over stdio gets the same results.

Options weighed:
- A library, with no command. The harnesses would then link Rust or write their own loops, which is
  the problem this RFC fixes.
- A loop inside the harness (Python). It would drift from the toolchain and could not ship with it.

Recommendation: the command.

- Decision: accepted, 2026-11-01
- Row: R616

### D2: the loop

Phase 1, the spec:
1. The model writes items whose bodies are `...`.
2. `spec_check` checks them.
3. The model gets up to `--spec-repairs` repair rounds, and each round shows it the line rendering.
4. The accepted spec is written to the module and frozen. In the harness the loop accepts it.
   Elsewhere, a person can review it before phase 2 (a later flag; see open questions).

Phase 2, the bodies. Each round runs these steps in order:
1. **Freeze.** If the reply drops an item of the spec, or changes a def line, docstring or contract,
   it is refused before any tool runs. The model is told which items differ. `check_patch` still
   enforces P006 behind this.
2. `apply_patch` applies the reply as a plain patch, with `--check`.
3. `autofix` runs, and its patch is applied when it is not empty. No model turn is spent on a
   mechanical fix (21.4).
4. `run_tests` runs the module's `test_` functions and `ex` lines.

The first refusal or test failure is shown to the model as the next repair round, up to
`--body-repairs` rounds. The run is green when the tests pass.

**Stop and report.** A reply with a line `STOP: <reason>` ends the run with status `stopped`. This
is the 21.1 rule that an agent which believes a frozen test is wrong stops instead of editing it.

The prompts are fixed in the toolchain and versioned with it. The primer and the shots are inputs.

- Decision: accepted, 2026-11-01
- Row: R617

### D3: model adapters

`--model` takes one of two forms:
- `stub:FILE` replays scripted replies. It is used for tests and for CI.
- `cmd:PROGRAM ARGS` starts a process. The loop writes one JSON line per turn,
  `{"system", "messages", "max_tokens"}`, and reads back one line,
  `{"text", "usage": {"in", "out", "reasoning"}, "model"}`.

Hosted APIs (Anthropic, OpenAI, local servers) are reached through `cmd:` adapters, which the
harness ships. The toolchain makes no HTTPS calls and holds no API keys.

Option weighed: built-in `anthropic:` and `openai:` clients. These would need a TLS stack and
proxy handling in the toolchain binary. They would also tie a toolchain release to vendor API
changes.

Recommendation: stub and cmd only.

- Decision: accepted, 2026-11-01
- Row: R618

### D4: the run record

The run record is JSON with `schema: 1`. Its fields:
- `task`, `model` (the adapter's reported id) and `toolchain` (`version`, and the BLAKE3 hash of the
  binary);
- `status`, which is `green`, `spec_failed`, `body_failed` or `stopped`, and `rounds_to_green`;
- `tokens.spec` and `tokens.body`, each `{in, out, reasoning}` as the provider reports them (R567);
- `rounds`. Each round has its phase, its number, its usage, and every tool call it made. A tool call
  records the diagnostic codes it hit, the failing test ids, and how many items autofix replaced
  (R280). A round also has `ok` and the report the model saw next;
- `system`, `messages`, `spec` and `final`, so the harness can recount tokens with o200k, cl100k and
  the model's own tokenizer (21.6).

New fields are added only as optional fields, as for diagnostics (18.2).

- Decision: accepted, 2026-11-01
- Row: R619

### D5: the harnesses drive Yan Agent through `yan agent`

21.6 gains one rule: the Yan Agent arm of the M1 gate harness, and of any probe that reports against it,
runs through `yan agent`, so repair rounds and autofix are counted the way a user would meet them.
The typed-Python arm keeps the same shape: a spec phase with pytest tests, then a body phase with
mypy and pytest repairs. That arm stays in the harness.

- Decision: accepted, 2026-11-01
- Row: R620

### D6: a code for an `ex` line that breaks `pre`

21.1 has the spec gate check that each `ex` line satisfies `pre`, but no code exists for a failure.
This adds an error code in 25.8: "the arguments of an `ex` line break the function's `pre`". It is
reported by `spec_check`, at the `ex` line. A `pre` that cannot be evaluated yet is skipped, as 21.1
allows. One example is a `pre` that calls a function whose body is still `...`.

- Decision: accepted, 2026-11-01
- Row: R621

## Spec changes

- 18.1: a `yan agent` row at M1 (D1).
- 21.1: one paragraph on the reference loop, its order of steps and stop-and-report (D2), and the
  new code (D6).
- 21.4: the model adapter protocol (D3), as a short subsection after the tool table.
- 21.6: the run record (D4) and the rule that the Yan Agent arm runs through `yan agent` (D5).
- 25.8: the code from D6.

## Open questions (not for this RFC)

- A `--review` pause between the phases, so a person accepts the spec. It needs a confirmation token
  format (P006), which is not specified yet.
- `post` checks in the spec gate. They wait for `post` lowering.
- Satisfiability of `pre` by the bounds solver.
