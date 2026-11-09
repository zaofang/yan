# 0061: Serve a package's public functions as MCP tools

- Status: draft
- Research: none; polish item N4 (#866, tracked by #862). Builds on 21.4, R676 and the json wire
  forms of R874 to R877.
- Rows: filled in when accepted

## Summary

`yan mcp` (21.4) serves the toolchain to an agent. A package written to be an agent tool (the
`agent-tool` template of `yan new`) has no way to be served itself: today an author wraps it by
hand in another language's MCP SDK, and the capability story stops at that wrapper. This RFC adds a
second server, `yan mcp package`, that serves the public functions of a package as MCP tools.
Each tool's input and output schemas come from its signature, its description carries its `uses`
clause, and a call gets only the authority the operator granted. The interpreter implements the
recommended option of each item; native serving is a later gap item.

## Decisions

### D1: the command

`yan mcp package [DIR] [--allow CAP,...] [--list]` serves the package at `DIR` (default `.`) over
stdio with the 21.4 transport and protocol version. It serves only the package's tools, never the
toolchain tools of 21.4. `--list` prints the `tools/list` result as JSON and exits, so the
contract can be kept as a golden.

Options: (a) a `package` subcommand beside `yan mcp schema`; (b) a `--package DIR` flag that
switches `yan mcp` into a second mode; (c) a new top-level `yan serve`. (a) keeps the two servers,
whose authority differs, apart while grouping both under `mcp`; (b) gives one command two
unrelated tool sets; (c) separates it from the MCP rows it shares transport and budgets with.

- Decision: (pending, card 2026-11-09)
- Row:

### D2: which functions are tools

Every `pub def` of the package's entry module (`src/main.yag`, else `src/lib.yag`) is a tool,
except `main` and `test_` functions. The tool's name is the function's name and its description is
the docstring followed by a line `uses: fs, net` (or `uses: none`). A public function that cannot
be served stops the server with E201 naming it and why: it is generic, it has an `inout`
parameter, it uses `io` (stdio carries the protocol), or a parameter or its result has no json
form (R875). Making the function private keeps it out.

Options: (a) public functions, as #866 and the `agent-tool` template state; (b) an opt-in `@tool`
decorator. (b) adds a decorator for what visibility already says; (a) fails loud instead of
silently skipping a function the author meant to serve (law 13).

- Decision: (a), inconsequential, as the issue states
- Row:

### D3: arguments, results and failures

The arguments are one JSON object whose members are the parameters, decoded as a record with the
shapes of R875: an absent member takes the parameter's literal default, an optional parameter may
be absent, unknown members are rejected (R876). The result is
`structuredContent: {"result": <value>}` in the R875 shape, a unit result being `null`, with the
same JSON as text in `content`. The `inputSchema` and `outputSchema` are JSON Schema derived from
the same wire schema elaboration computes for `json.parse[T]`, so the schema and the decoder cannot
disagree. A failure value, an argument that does not decode (with its R877 path), a panic and a
budget stop each return `isError: true` with the reason as text.

- Decision: (a), inconsequential, follows R874 to R877
- Row:

### D4: authority

The operator grants capabilities with `--allow`; the default grant is none. A call whose function
`uses` a capability outside the grant is refused before it runs, naming the missing capabilities.
A granted `fs` is `fs.sub(root)` (R674): paths resolve beneath the served package and never above
it. Other granted capabilities bind the host handlers.

Options: (a) the operator's grant, default none; (b) every capability the function's `uses`
names, with `fs` confined; (c) as (a) with the manifest's `capabilities` as the default grant. (a)
puts the person running the server, not the package author, in charge of authority, while each
call stays bounded by its signature; (b) lets any package that is installed exercise everything
it declares; (c) is (b) with an opt-out.

- Decision: (pending, card 2026-11-09)
- Row:

### D5: trust and budgets

Serving runs the package's code, so the server refuses to start in a directory `yan trust` has
not recorded (R676). Each call runs on the interpreter under the per-call and per-session step,
heap and wall-clock budgets of 21.4 (R676).

- Decision: inconsequential, follows R676
- Row:

## Spec changes

- 18.1: a row for `yan mcp package [DIR] [--allow CAP,...] [--list]`.
- 21.4: a paragraph "Serving a package" with D2 to D5.
