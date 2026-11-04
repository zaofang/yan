# Yan Shell 言行 (`.ynsh`): syntax and semantics study

Date: 2026-11-04. Status: brainstorm study. Not a spec change: no rows, no diagnostic codes. Every
grammar and example below is **MOCK** and non-normative. Proposal items are labelled D1, D2, ...

## Summary

Yan Shell should be the shell that bash users can read on day one and that a reviewer, a CI system
or an agent sandbox can trust without running it. The consolidated proposal (section 8) rests on four
ideas:

1. **The header is the policy.** One `uses` line names the programs, paths and hosts the script may
   touch, and it lowers to the existing attenuators (`proc.only`, `fs.only`, `net.only`, R674).
2. **One glyph runs a program.** A line starting with `$` is a command, exactly as in every README.
   `$` is the mark for both process use and failure: a non-zero exit fails the script. `$?` gives the
   exit status back as data.
3. **Argv is the syntax tree.** Words are literal, `{x}` is exactly one argument, `{xs...}` splats a
   list, and nothing is ever re-scanned, split or globbed. What a reviewer or guard reads is what
   runs, which closes the inspection-execution gap behind the 2026 GuardFall bypasses.
4. **Bare word is a program, dot is Yan.** In a pipeline `| sort` runs the program, `| .sorted()`
   runs a Yan function on the stream through Yan's implicit subject (spec 15). Both worlds share one
   pipe.

On the four shared tasks the mix is 10% larger than bash by a lexical token count (398 against 361),
while closing the quoting, word-splitting, error-handling and ambient-authority classes that cause
most real bash bugs. The most terse proposal was 19% smaller than bash; the most explicit was twice
its size. The biggest open item is not syntax: a child process today keeps the user's full OS
authority, so an honest header needs OS sandboxing of children (D9).

## 1. Methodology and process

**Brief.** Antonio's 2026-11-04 request: research peers, users' wants, what works and what does not,
old and new research; brainstorm; write five proposals each optimizing one axis; mix into one
consolidated proposal; document the method. Target qualities: elegant, concise, powerful,
expressive, opinionated without being obnoxious.

**What I read in the repo (narrowly).** `docs/surfaces.md` section 9 (status, RFC example, mock
grammar); RFC 0002 D5 and the admission rules N1 to N9; spec 16.7 row R540; spec 7 capability table
(`proc`, `fs`, `net`, `env`, sharing classes); spec 17 `std.proc` (R691 to R693), `@derive(Cli)`
(R694, R695) and attenuators (R674); spec 15.1 lexical rules and 15.2 statement and expression
grammar; spec 9.4 deadlines; `examples/tally/g5_tally.yn` for house style.

**What I searched.** Oils YSH reference and tour; Nushell design notes on byte streams; SHILL (OSDI
2014) abstract and full paper; "Unix shell programming: the next 50 years" (HotOS 2021); "Bash in
the Wild" (TOSEM 2022); BashFAQ 105 on `set -e`; PaSh (EuroSys 2021); Bun Shell and zx; PEP 750
and PEP 787 (t-strings for subprocess); the GuardFall research note (Cloud Security Alliance, July
2026); a 2025 paper on transactional sandboxing for coding agents; Landlock documentation; Amber
(FOSDEM 2026); `just` and task-runner comparisons. Facts I did not re-check this session are marked
*(from memory)*.

**How I experimented.**

1. Picked four tasks that cover the shell's real jobs: a CI gate, file housekeeping with a remote
   copy, parallel fan-out with timeouts, and a git-driven check over a file list (section 4).
2. Wrote a bash baseline for each, idiomatic and `set -euo pipefail`-hardened.
3. Wrote the same four tasks in each of five proposals, each pushed to one extreme axis.
4. Measured every sample: bytes and a lexical token count (identifiers, numbers and single
   punctuation marks; shebang and doc-comment lines excluded). The real BPE tokenizers were
   unreachable from the container (the encoding downloads are blocked by the proxy), so this is a
   proxy; the R567 harness should replace it with model-billed tokens.
5. Scored each proposal against the bug classes from the research (section 7).
6. Checked each candidate rule against Yan's grammar for collisions (`|` is bitwise or in Yan, `>`
   is comparison, braces are literal in Yan strings) and against law 4 (five facts visible) and N3
   (one printing).
7. Mixed: kept each proposal's best idea only where it did not cost another axis much, then rewrote
   the four tasks in the mix and re-measured.

## 2. Where Yan Shell stands today

Six findings from the repo, each a tension the study had to resolve:

- **F1. `|` already means bitwise or.** Spec 15.2 has `bitor` in the expression grammar. The RFC
  sketch's `lines(f) | grep("ERROR")` is a type error in Yan today. The surface must own `|`.
- **F2. There are no t-strings in the core.** RFC D5 uses `t"rsync -a {out.path} ..."`, but Yan
  strings interpolate with `\(x)` and treat braces as literal (15.1). The argv split has to happen in
  the surface's lowering, at stage 0.
- **F3. Exit codes disagree.** RFC D5 says "a non-zero exit is a typed `ProcError`". R692 says "a
  non-zero exit is data, not a failure", and `ProcError` covers only a child that could not start.
  The surface must pick a default and offer the other.
- **F4. The header overstates its authority.** `uses proc, fs, net` constrains Yan code. A child
  started by `proc.run` keeps the user's full OS authority; `proc.only` (R674) limits only *which*
  programs start. `rsync` under `net.only(...)` can still reach any host. SHILL solved this with a
  MAC sandbox per exec.
- **F5. Process-to-process pipes are not specified yet.** `spawn` and piped stdio wait for `Reader`
  and `Writer` (R693). Until then only Yan-side line pipelines are implementable.
- **F6. The core already gives a shell things no shell has.** Deterministic `par` with output
  committed in source order at the join (spec 7 sharing classes), deadlines as scopes (9.4), a
  `fake_proc` test handler (R692), typed CLI parsing (R694), rooted paths with no `..` escape (R674).

## 3. The market: peers and competitors

| Tool | Model | What works | What does not |
| --- | --- | --- | --- |
| bash, POSIX sh | strings, re-scanned words | everywhere; terse; pipes scale; the tool universe | quoting, word splitting, `set -e` holes, ambient authority, no types |
| zsh, fish | interactive-first shells | fish's no-word-splitting rule and readable syntax; great completion | still strings; fish not POSIX, few scripts in it |
| PowerShell | .NET objects in pipes | structured pipeline, verb-noun discoverability, `-WhatIf` dry runs *(from memory)* | verbose; slow start; externals are second-class text |
| Nushell | typed tables in pipes | `where`, `sort-by`, built-in JSON and CSV, clear errors | external output has unknown encoding; frequent breaking changes; stderr and redirect rules still moving (core-team notes) |
| Oils YSH | bash-compatible upgrade path | words vs expressions mode, `@arr` splice, errexit fixed by default, J8 JSON with bytes | two syntaxes in one file; adoption still small |
| Elvish, Murex | structured values, exceptions | non-zero exit raises *(from memory)*; Murex types pipes | niche |
| xonsh | Python plus shell mode | Python libraries in scripts | mode detection is heuristic, surprises users *(from memory)* |
| zx, Bun Shell, dax | JS template literal `$` | `$` tag is short and auto-escapes interpolations; zx throws on non-zero exit; Bun ships cross-platform builtins | zx still runs a real shell underneath; JS async noise |
| Amber | typed language compiled to bash | types and errors handled before runtime; FOSDEM 2026 talk | output is still bash with bash's runtime gaps |
| execline, Plan 9 rc | no re-scanning | rc: lists are values, one parse, no splitting *(from memory)*; execline: argv chain-loading, no interpreter at run time | unfamiliar; tiny ecosystems |
| SHILL | capabilities and contracts for scripts | least authority per script and per exec; contracts are documentation | research prototype (Racket, FreeBSD); exec sandbox up to 6x slower in worst cases |
| just, make, Taskfile | named recipes | the "commands with names" job most people actually have | not a language; bash inside each recipe |
| Deno | runtime permission flags | `--allow-run=git` style grants are understood by users | grants live on the command line, not in the file |

**What users want, as the sources and these tools' adoption show:**

- Scripts that do not break on a space in a file name.
- Failing loudly on the first error, with the failing command and its stderr.
- Structured data when it helps (JSON from `cargo metadata`, `gh`, `kubectl`) and plain text when it
  does not.
- Parallelism that does not garble output.
- The same script on macOS, Linux and CI.
- A way to see what a script will touch before running it, now urgent for agent-run commands.
- Familiarity: `|`, `>`, `$ cmd` are muscle memory. Proposals that abandon them lose.

## 4. Research, old and new

- **Bugs in real bash (TOSEM 2022, high confidence).** Over 80% of general scripts had at least one
  ShellCheck report; quoting and word splitting, and error handling, dominate. Of manually reviewed
  semantic bugs: quoting 16%, file and path handling 14%, command options 11%, permissions 11%,
  error handling 11%. ShellCheck catches quoting but few resource, permission or option bugs.
- **`set -e` is not error handling (BashFAQ 105, high).** errexit is suppressed in conditions, in
  `&&` chains, in functions called from conditions and in command substitutions; `pipefail` is
  separate. The fix is a language where failure is a value with a defined path, not a flag.
- **The next 50 years (HotOS 2021, high).** The shell's strengths are universal composition, stream
  processing and terseness; its weaknesses are that it is too arbitrary, too dynamic and too obscure
  to analyze. Directions: annotated commands, distribution, incremental re-execution, formal
  semantics (Smoosh).
- **PaSh (EuroSys 2021, high).** Annotating 47 commands with four parallelizability classes
  (stateless, parallelizable pure, non-parallelizable pure, side-effectful) gave an average 6.7x
  speedup on 44 unmodified scripts. In Yan the classes come free: a Yan stage's `uses` set and
  purity are known to the checker, so no annotation file is needed for Yan stages.
- **SHILL (OSDI 2014, high).** Capability-safe scripts receive authority only as arguments; an
  ambient launcher creates capabilities from paths; each exec of a native binary runs in a sandbox
  that sees only the capabilities passed. Privileges are fine-grained (`+lookup`, `+append`, ...).
  Lesson: language-level capabilities without a child sandbox are only half the story (F4).
- **Object capabilities (Miller, 2006, from memory, high).** Authority by reference, attenuation by
  wrapping. Yan already follows this; the shell surface should make attenuation literal and short.
- **Injection by re-scanning (PEP 750 accepted, PEP 787 deferred; high).** Python added template
  strings so that interpolations stay separate from literal parts; PEP 787 would build safe
  subprocess calls on them and notes that escaping is not enough on Windows. Separation by
  construction beats escaping.
- **GuardFall (CSA, July 2026, high).** Eleven open-source coding agents were tested; ten had
  command guards that could be bypassed by quote removal, `$IFS` splitting, command substitution,
  base64-to-shell and destructive flags on allowed binaries. Root cause: the guard checks the string
  the model wrote, the shell runs a transformed one. Only a parse-then-evaluate guard holds. A
  language whose argv is its syntax tree has no gap to exploit, except the last class (destructive
  flags), which needs per-program policy.
- **Transactional sandboxing for agents (arXiv 2512.12806, 2025, medium).** Snapshots plus policy
  interception caught every high-risk command in the prototype at about 14.5% overhead. Supports an
  `atomic` block idea (D11).
- **OS confinement without root (high).** Linux Landlock restricts filesystem access and, in newer
  ABIs, TCP bind and connect, for an unprivileged process and its children; OpenBSD has
  `pledge` and `unveil`; FreeBSD has Capsicum; macOS has the Seatbelt sandbox *(from memory)*. These
  are enough to make a child see only granted paths and hosts on the main platforms.

## 5. Frameworks: ideas worth building on

These are the vanguard ideas the proposals draw from. Each lowers to the one core unless noted.

1. **Authority as namespace.** The programs named in the header are the only names a command line
   may start. The header is both the import list and the policy.
2. **Argv is the AST.** No string is ever re-parsed into commands. Interpolation creates argv
   entries, never text. Guards, reviewers and `ynsh plan` read the same tree that runs.
3. **Two worlds, one pipe.** A bare word stage is a process; a dot stage is a Yan function over the
   stream, through Yan's existing implicit subject. Text, lines, bytes and typed values flow in one
   pipeline, converted at stage boundaries by type.
4. **Failure is a mark, status is data.** `$` fails on non-zero exit (bash users want this, zx and
   Elvish do it); `$?` returns R692's `Output`. Both are visible at the call site (law 4).
5. **Determinism dividends.** Parallel jobs whose logs never interleave (output committed in source
   order at the join); scripts testable with `fake_proc` and a temp `fs`; replay of a recorded run.
6. **Free PaSh.** Pure Yan stages with known `uses` are data-parallel by type; `par` makes the rest
   explicit.
7. **Plan before run.** Because programs and grants are literal, `ynsh plan` can print the full
   authority manifest (programs, paths with modes, hosts) and every literal command, without running
   anything. This is what agent harnesses and reviewers need.
8. **Honest children.** A child sees only what the header grants it, enforced by the OS where
   possible, and the header says when it is not.

## 6. The shared tasks and the bash baseline

- **T1 CI gate.** Build, keep the log, fail when warnings exceed `--max` (default 0).
- **T2 Housekeeping.** For each `logs/*.log` older than a day: write sorted counts of `ERROR` lines
  to `out/STEM.errors`, gzip the log; then rsync `out/` to a backup host.
- **T3 Fan-out.** Run `cargo test` for every workspace package, at most 8 at once, 10 minutes each,
  one log per package; fail listing the failing packages.
  The Yan samples assume a `Metadata` record type for `cargo metadata` output, declared elsewhere
  (bash uses `jq` instead).
- **T4 Changed files.** Format the Rust files changed since `origin/main` and fail if formatting
  changed anything.

Bash baseline (hardened; each still has subtle issues noted after it):

```bash
#!/usr/bin/env bash
set -euo pipefail
max=${1:-0}
cargo build --release > build.log 2>&1
n=$(grep -c 'warning:' build.log || true)
if (( n > max )); then
  echo "$n warnings" >&2
  exit 1
fi
```

```bash
#!/usr/bin/env bash
set -euo pipefail
mkdir -p out
find logs -maxdepth 1 -name '*.log' -mtime +0 -print0 |
while IFS= read -r -d '' f; do
  stem=$(basename "$f" .log)
  { grep 'ERROR' "$f" || true; } | sort | uniq -c > "out/$stem.errors"
  gzip "$f"
done
rsync -a out/ backup.example.com:/srv/errors/
```

```bash
#!/usr/bin/env bash
set -uo pipefail
mkdir -p out
cargo metadata --no-deps --format-version 1 | jq -r '.packages[].name' |
  xargs -P 8 -I{} sh -c 'timeout 600 cargo test -q -p "$1" > "out/$1.log" 2>&1 || echo "$1"' _ {} > out/failed
if [ -s out/failed ]; then
  sort out/failed >&2
  exit 1
fi
```

```bash
#!/usr/bin/env bash
set -euo pipefail
mapfile -t files < <(git diff --name-only --diff-filter=d origin/main -- '*.rs')
(( ${#files[@]} )) || exit 0
rustfmt --edition 2024 -- "${files[@]}"
git diff --exit-code -- "${files[@]}"
```

Issues even in careful bash: T1's `|| true` hides a missing log; T2 needs `-print0` and `IFS= read
-r -d ''` to survive spaces, and its `grep || true` group exists only to stop `pipefail` from killing
the loop on a log without errors; T3 re-enters `sh -c` to quote one argument, and the parallel jobs
write `out/failed` in completion order; T4's process substitution hides `git diff`'s exit status from
`set -e`. None of the scripts states what it may touch.

## 7. Five proposals

Each proposal optimizes one axis on purpose, past the point of good taste, so the trade-off is
visible.

### P1 Terse: the expert's shell (axis: brevity)

Principles: a line whose first word is a program granted in `uses` is a command, with no sigil
(authority as namespace, taken all the way); paths are bare words; zero-argument stages drop
parentheses; failure is implicit; `<path` reads lines.

```
#!/usr/bin/env -S ynsh
uses cargo, fs.write(.)
arg max = 0
cargo build --release &> build.log
let n = <build.log | .grep("warning:") | .count
if n > max
  exit 1, "\(n) warnings"
```

```
#!/usr/bin/env -S ynsh
uses gzip, rsync, fs.write(logs, out), net(backup.example.com:22)
for f in logs/*.log where .age > 1d
  <f | .grep("ERROR") | .sort | .uniq_count > out/\(f.stem).errors
  gzip {f}
rsync -a out/ backup.example.com:/srv/errors/
```

```
#!/usr/bin/env -S ynsh
uses cargo, fs.write(out)
let pkgs = cargo metadata --no-deps --format-version 1 | .json.packages.name
let bad = par(8) for p in pkgs
  timeout 10min: cargo test -q -p {p} &> out/\(p).log else p
if bad
  exit 1, bad.sorted.join("\n")
```

```
#!/usr/bin/env -S ynsh
uses git, rustfmt, fs.write(.)
let files = git diff --name-only --diff-filter=d origin/main -- *.rs | .lines
if files
  rustfmt --edition 2024 -- {files...}
  git diff --exit-code -- {files...}
```

Keep: `<path` as a line source; `{files...}` splat; `arg` declarations; `uses` with program names;
`exit CODE, MSG`. Drop: sigil-free commands (a typo in a Yan function name becomes "program not
granted", and adding a program to `uses` can change what an existing line means); parenless stages
break N3 unless every surface agrees; implicit failure breaks law 4.

### P2 Readable: plain Yan with a standard library (axis: learnability, zero new syntax)

Principles: Yan Shell is just Yan in a `.ynsh` file plus a shell-flavoured std; every process is a
`Command` value; every failure is a `?`.

```
#!/usr/bin/env -S ynsh
--- Fail CI when the build has more warnings than allowed.
main(args: Args) fail ShellError uses proc, fs =
  let ws = fs.root(".", write)
  let build = proc.run(Command("cargo", ["build", "--release"])).check()?
  ws.write("build.log", build.stderr)?
  let n = build.stderr.lines().filter(l -> "warning:" in l).count()
  if n > args.int("max", 0)
    fail exit(1, "\(n) warnings")
```

```
#!/usr/bin/env -S ynsh
--- Archive day-old logs and ship their error counts.
main(args: Args) fail ShellError uses proc, fs, net =
  let logs = fs.root("./logs", write)
  let out = fs.root("./out", write)
  for f in logs.glob("*.log").filter(f -> f.age() > 1d)
    let counts = f.lines()?.filter(l -> "ERROR" in l).sorted().run_counts()
    out.write("\(f.stem()).errors", counts.render())?
    proc.run(Command("gzip", [f.path()])).check()?
  let backup = net.only(connect=["backup.example.com:22"])
  with net = backup
    proc.run(Command("rsync", ["-a", "out/", "backup.example.com:/srv/errors/"])).check()?
```

```
#!/usr/bin/env -S ynsh
--- Test every workspace package in parallel and list the failures.
main(args: Args) fail ShellError uses proc, fs, time =
  let out = fs.root("./out", write)
  let meta = proc.run(Command("cargo", ["metadata", "--no-deps", "--format-version", "1"])).check()?
  let pkgs = json.parse[Metadata](meta.stdout)?.packages.map(p -> p.name)
  let results = par(limit: 8) for p in pkgs
    with timeout(10min)
      let r = proc.run(Command("cargo", ["test", "-q", "-p", p]))?
      out.write("\(p).log", r.stdout + r.stderr)?
      (p, r.success())
  let bad = results.filter((p, ok) -> not ok).map((p, ok) -> p).sorted()
  if not bad.is_empty()
    fail exit(1, bad.join("\n"))
```

```
#!/usr/bin/env -S ynsh
--- Fail when changed Rust files are not formatted.
main(args: Args) fail ShellError uses proc, fs =
  let diff = proc.run(Command("git", ["diff", "--name-only", "--diff-filter=d", "origin/main", "--", "*.rs"])).check()?
  let files = diff.stdout.text()?.lines()
  if files.is_empty()
    return
  proc.run(Command("rustfmt", ["--edition", "2024", "--"]).with_args(files)).check()?
  proc.run(Command("git", ["diff", "--exit-code", "--"]).with_args(files)).check()?
```

Keep: everything lowers trivially, nothing new to learn for a Yan user, the types are exactly R691
to R693, `with timeout(...)` and `par(limit: ...)` read well. Drop: the argv lists. Twice the size of
bash; nobody migrates from bash to this, which is the surface's whole job.

### P3 Canonical: one way, keyword per line (axis: agent token cost and checkability)

Principles: every statement starts with a keyword (`grant`, `param`, `run`, `pipe`, `let`, `check`,
`stop`, `skip`, `collect`), so the grammar is LL(1) on the first token and fits a small GBNF for
constrained decoding; no expressions beyond comparisons and field access; no shebang (run by
`ynsh`).

```
grant proc cargo
grant write build.log
param max int 0
run cargo build --release >& build.log
let n = count build.log match "warning:"
check n <= max else "{n} warnings"
```

```
grant proc gzip rsync
grant write logs out
grant net backup.example.com:22
for f in glob logs/*.log
  skip f.age <= 1d
  pipe read f | match "ERROR" | sort | uniq_count > out/{f.stem}.errors
  run gzip {f}
run rsync -a out/ backup.example.com:/srv/errors/
```

```
grant proc cargo
grant write out
let meta = json run cargo metadata --no-deps --format-version 1
par 8 for p in meta.packages.name
  limit 10min
  run cargo test -q -p {p} >& out/{p}.log
  collect bad p if failed
check bad.empty else "{bad.sorted}"
```

```
grant proc git rustfmt
grant write .
let files = lines run git diff --name-only --diff-filter=d origin/main -- *.rs
stop if files.empty
run rustfmt --edition 2024 -- {files...}
run git diff --exit-code -- {files...}
```

Keep: smallest token count of all (26% under bash); `grant` lines read as policy; `check COND else
MSG` is a lovely gate; keyword-led lines make `ynsh plan` trivial. Drop: it is a second language, not
a subset of Yan (fails N2 and N7: `count build.log match ...` has no Yan printing); `collect` and
`skip` are ad hoc; it will need expressions the day after launch.

### P4 Least authority: SHILL in Yan (axis: safety and explicitness)

Principles: authority is passed, never ambient; each grant is a typed value with exact privileges;
each program grant says what the child may see (paths, hosts), enforced by an OS sandbox;
`atomic dir` stages writes and commits them only on success.

```
#!/usr/bin/env -S ynsh
grant ws = dir(".", read, write: ["build.log"])
grant cargo = exe("cargo", sees: ws.read + "~/.cargo", net: ["index.crates.io:443"])

--- Fail CI when the build has more warnings than allowed.
gate(max: int = 0) fail ShellError uses ws, cargo =
  let r = cargo.run(["build", "--release"], stderr: ws / "build.log")?
  let n = (ws / "build.log").lines()?.filter(.contains("warning:")).count()
  if n > max
    fail exit(1, "\(n) warnings")
```

```
#!/usr/bin/env -S ynsh
grant logs = dir("./logs", read, write: ["*.gz"], remove: ["*.log"])
grant out = dir("./out", create)
grant gzip = exe("gzip", sees: logs)
grant rsync = exe("rsync", sees: out.read, net: ["backup.example.com:22"])

ship() fail ShellError uses logs, out, gzip, rsync =
  atomic out
    for f in logs.glob("*.log") if f.age > 1d
      let counts = f.lines()?.filter(.contains("ERROR")).sorted().run_counts()
      out.write("\(f.stem).errors", counts)?
  for f in logs.glob("*.log") if f.age > 1d
    gzip.run([f])?
  rsync.run(["-a", out / "", "backup.example.com:/srv/errors/"])?
```

```
#!/usr/bin/env -S ynsh
grant ws = dir(".", read)
grant out = dir("./out", create)
grant cargo = exe("cargo", sees: ws + "~/.cargo", writes: ["./target"], net: none)

test_all() fail ShellError uses ws, out, cargo =
  let meta = cargo.run(["metadata", "--no-deps", "--format-version", "1"]).json[Metadata]()?
  let bad = par(limit: 8) for p in meta.packages
    with timeout(10min)
      let r = cargo.status(["test", "-q", "-p", p.name], both: out / "\(p.name).log")?
      if r.success() then None else p.name
  let failed = bad.compact().sorted()
  if failed
    fail exit(1, failed.join("\n"))
```

```
#!/usr/bin/env -S ynsh
grant src = dir(".", read, write: ["**/*.rs"])
grant git = exe("git", sees: src.read + "./.git", net: none)
grant rustfmt = exe("rustfmt", sees: src)

fmt_check() fail ShellError uses src, git, rustfmt =
  let files = git.run(["diff", "--name-only", "--diff-filter=d", "origin/main", "--", "*.rs"]).lines()?
  if not files
    return
  rustfmt.run(["--edition", "2024", "--", ...files])?
  git.run(["diff", "--exit-code", "--", ...files])?
```

Keep: `exe(..., sees: ..., net: ...)` is the only honest answer to F4; `atomic`; grants as named
values that a library function can receive. Drop the default verbosity: 2.2 times bash, and grants
like `sees: ws.read + "~/.cargo"` require knowing a tool's internals (cargo needs its home, target
directory and the index). Child confinement should be opt-in per program, with the unconfined case
shown, not forbidden.

### P5 Structured: data in the pipe (axis: expressive power)

Principles: pipelines carry typed values; external output is parsed at the boundary
(`.json[T]()`, `.json_lines[T]`, `.parse("{ts} {level} {msg}")`); tables from spec 11; `par for` and
`.par_map` for fan-out; `$` marks external commands, `$?` returns status.

```
#!/usr/bin/env -S ynsh
uses proc(cargo), fs(write: ".")
arg max = 0
let msgs = $ cargo build --release --message-format=json | .json_lines[CargoMsg]
msgs | .tee_text(./build.log)
let warns = msgs | .where(.level == warning) | .group_by(.code) | .table
if warns.total > max
  io.eprint(warns.sort_by(-.count).head(10))
  exit 1, "\(warns.total) warnings"
```

```
#!/usr/bin/env -S ynsh
uses proc(gzip, rsync), fs(write: [logs, out]), net(backup.example.com:22)
let old = ls ./logs/*.log | .where(.age > 1d)
par for f in old
  f.lines | .parse("{ts} {level} {msg}") | .where(.level == "ERROR")
    | .count_by(.msg) | .sort_by(-.count) > ./out/\(f.stem).errors.tsv
  $ gzip {f}
$ rsync -a ./out/ backup.example.com:/srv/errors/
```

```
#!/usr/bin/env -S ynsh
uses proc(cargo), fs(write: out)
let pkgs = $ cargo metadata --no-deps --format-version 1 | .json[Metadata].packages
let runs = pkgs | .par_map(8, p -> $? cargo test -q -p {p.name} &> ./out/\(p.name).log, timeout: 10min)
let report = runs | .table(pkg: .cmd.arg(3), ok: .success, secs: .elapsed.secs)
io.print(report.sort_by(.secs).tail(5))
if report | .any(not .ok)
  exit 1, report | .where(not .ok) | .column(pkg) | .join("\n")
```

```
#!/usr/bin/env -S ynsh
uses proc(git, rustfmt), fs(write: .)
let files = $ git diff --name-only --diff-filter=d origin/main -- *.rs | .lines
if files
  $ rustfmt --edition 2024 -- {files...}
  $ git diff --exit-code -- {files...}
```

Keep: `$` and `$?`; dot stages; `.json[T]()` at the boundary; format parsing patterns. Drop for the
first slice: tables and `group_by` in a shell (they belong to Yan and the dataflow surface); P5's T1
and T3 do more than the task asked, which is the risk of this axis.

### Measurements

Lexical tokens per task (identifiers, numbers and single punctuation marks; shebang and doc lines
excluded) and total bytes. Ratio is against bash.

| Design | T1 | T2 | T3 | T4 | Tokens | Bytes | vs bash |
| --- | --- | --- | --- | --- | --- | --- | --- |
| bash | 66 | 103 | 106 | 86 | 361 | 929 | 1.00 |
| P1 Terse | 56 | 85 | 84 | 69 | 294 | 806 | 0.81 |
| P2 Readable | 125 | 217 | 240 | 173 | 755 | 1872 | 2.09 |
| P3 Canonical | 47 | 78 | 73 | 69 | 267 | 887 | 0.74 |
| P4 Least authority | 152 | 230 | 216 | 188 | 786 | 1970 | 2.18 |
| P5 Structured | 111 | 140 | 167 | 75 | 493 | 1306 | 1.37 |
| Mix | 68 | 110 | 133 | 87 | 398 | 1036 | 1.10 |

### Bug-class matrix

How each design treats the failure classes from section 4. "Closed" means impossible by
construction; "checked" means a static error; "open" means as bad as bash.

| Class | bash | P1 | P2 | P3 | P4 | P5 | Mix |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Quoting and word splitting | open | closed | closed | closed | closed | closed | closed |
| Unintended globbing | open | open (`*.rs` bare) | closed | open | closed | open | closed (glob is a function; `"*.rs"` passed literally) |
| Ignored failures | flag with holes | implicit, unmarked | `?` each call | implicit | `?` each call | `$` marks | `$` marks; `$?` opt-out |
| Missing paths and permissions | runtime | checked against grants | runtime | checked | checked | checked | checked against grants |
| Ambient authority in Yan code | total | grants | `uses` only | grants | per value | grants | grants |
| Ambient authority in children | total | total | total | total | sandboxed | total | opt-in sandbox (D9) |
| Inspection-execution gap (GuardFall A to D) | open | closed | closed | closed | closed | closed | closed |
| Destructive flags on allowed programs (GuardFall E) | open | open | open | open | sandbox limits damage | open | sandbox limits damage; per-program arg rules later (D10) |
| Garbled parallel output | open | closed | closed | closed | closed | closed | closed |

## 8. The mix: Yan Shell as proposed

Taken from each: P1's `<path`, `arg`, `exit CODE, MSG` and splat; P2's rule that everything below
the header is Yan; P3's policy-first header and the plan view it enables; P4's per-program child
sandbox, made opt-in, and `atomic`; P5's `$`, `$?`, dot stages and typed boundaries.

### 8.1 Principles

1. A `.ynsh` file is a Yan program in script mode: top-level statements form `main`'s body.
2. The `uses` header is the whole policy, written as data, and is the only place authority appears.
3. `$` starts a process. Bare words after `$` are literal; nothing is split, globbed or re-scanned.
4. `|` is the pipe. A bare word stage is a program; a dot stage is Yan.
5. Failure is visible: `$` and `?` fail, `$?` returns the status, `exit` ends the script.
6. Familiar where it costs nothing (`|`, `>`, `>>`, `<`, `&>`, `$`), Yan everywhere else.

### 8.2 The four tasks

```
#!/usr/bin/env -S ynsh
--- Fail CI when the build has more warnings than allowed.
uses proc(cargo), fs(write: "build.log")
arg max: int = 0
$ cargo build --release &> build.log
let n = <build.log | .grep("warning:") | .count()
if n > max
  exit 1, "\(n) warnings"
```

```
#!/usr/bin/env -S ynsh
--- Archive day-old logs and ship their error counts.
uses proc(gzip, rsync), fs(write: ["logs", "out"]), net("backup.example.com:22")
for f in glob("logs/*.log") | .where(.age > 1d)
  <f | .grep("ERROR") | .sorted() | .run_counts() > out/{f.stem}.errors
  $ gzip {f}
$ rsync -a out/ backup.example.com:/srv/errors/
```

```
#!/usr/bin/env -S ynsh
--- Test every workspace package in parallel and list the failures.
uses proc(cargo), fs(write: "out")
let meta = $ cargo metadata --no-deps --format-version 1 | .json[Metadata]()
let bad = par(limit: 8) for p in meta.packages
  with timeout(10min)
    let r = $? cargo test -q -p {p.name} &> out/{p.name}.log
    if r.success() then None else p.name
let failed = bad | .compact() | .sorted()
if failed.len() > 0
  exit 1, failed.join("\n")
```

```
#!/usr/bin/env -S ynsh
--- Fail when changed Rust files are not formatted.
uses proc(git, rustfmt), fs(write: ".")
let files = $ git diff --name-only --diff-filter=d origin/main -- "*.rs" | .lines()
if files.len() > 0
  $ rustfmt --edition 2024 -- {files...}
  $ git diff --exit-code -- {files...}
```

T2 with confined children (D9), costing one clause per program:

```
uses proc(gzip sees ["logs"], rsync sees ["out"] net ["backup.example.com:22"]),
     fs(write: ["logs", "out"])
```

### 8.3 MOCK grammar

The script layer, on top of Yan's statement grammar (15.2):

```
script    = [shebang] [doccomment] header {argdecl} {item | stmt}
header    = "uses" grant {"," grant} NEWLINE          -- one logical line; continuation by indent
grant     = "proc" "(" prog {"," prog} ")"
          | "fs" "(" fsmode ":" paths {"," fsmode ":" paths} ")"
          | "net" "(" hosts ")"
          | "env" "(" STRING {"," STRING} ")"
          | NAME                                      -- any other capability, as in Yan
prog      = WORD ["sees" paths] ["net" hosts]         -- WORD may contain - and .
fsmode    = "read" | "write" | "create"
paths     = STRING | "[" STRING {"," STRING} "]"
argdecl   = "arg" NAME ":" type ["=" literal] NEWLINE -- becomes one @derive(Cli) field
stmt      = cmdline NEWLINE | pipeline NEWLINE | "exit" expr ["," expr] NEWLINE | yan-stmt
          | "atomic" STRING block
cmdline   = ("$" | "$?") word {word} {"|" stage} [redirect]
pipeline  = source {"|" stage} [redirect]             -- a statement or a let value
source    = "<" pathword | cmdline | postfix
stage     = word {word}                               -- a program, after $ only
          | "." NAME ["[" type "]"] "(" [args] ")" {"." NAME "(" [args] ")"}
word      = BAREWORD | STRING | "{" expr ["..."] "}" | pathword
pathword  = BAREWORD containing "/" or "." with optional "{" expr "}" parts
redirect  = (">" | ">>" | "&>" | "2>") pathword
```

Rules the grammar cannot show:

- Inside a statement-level pipeline, a trailing `>` followed by a word is a redirect. Comparison of a
  pipeline result needs parentheses: `if (<f | .count()) > 3`. Outside pipelines `>` is comparison.
- `|` is never bitwise or in `.ynsh`. Bitwise or is excluded from the surface's subset; `yan view
  --as ynsh` reports it as the excluding construct (N2).
- A program stage after a dot stage receives the stream rendered as lines of text; a dot stage
  after a program stage receives a line iterator, or bytes when its parameter type asks for bytes.
- `{x}` must be one value of a type that renders to one argument (`str`, `int`, `Path`, ...);
  `{xs...}` takes a `list` of those. A `{x}` inside a quoted literal part is an error (the RFC's
  LING-1, kept).
- No word is a glob. `glob("logs/*.log")` is a function returning rooted paths inside the grants;
  `"*.rs"` after `$` is passed through to the program literally.

### 8.4 Lowering (all to existing core, law 1)

| Surface | Lowers to |
| --- | --- |
| top-level statements | `main(args: Args) fail ShellError uses ... =` body |
| `uses proc(a, b)` | `uses proc` plus `with proc = proc.only(programs=["a", "b"])` around the body (R674) |
| `uses fs(write: "out")` | `uses fs` plus `with fs = fs.only(read=["out"], write=["out"])` (write implies read) |
| `uses net("h:22")` | `uses net` plus `with net = net.only(connect=["h:22"])` |
| `arg max: int = 0` | a field of a generated `@derive(Cli)` record parsed by `cli.parse` (R694); the `arg` line is the `env` mark |
| `$ w1 w2 {x} {xs...}` | `proc.run(Command("w1", ["w2", x.show()] + xs)).check()?`, where `check` fails with a non-zero-exit variant of `ShellError` |
| `$? ...` | the same without `check()`, returning `Output` (R692) |
| `a | b` between programs | `spawn` with piped stdio (R693; first slice waits for it) |
| `| .f(args)` | `f(stream, args)` through the implicit subject (15.3) |
| `<path` | `fs.read_lines(path)?` |
| `> p`, `>> p`, `&> p` | `fs.write`, `fs.append`, both streams; target checked against write grants |
| `exit c, msg` | `fail ShellError.exit(c, msg)` |
| `atomic "dir"` | writes go to a sibling temp directory, renamed over on success (D11) |

Five facts (law 4, N5): capability use is the `uses` header, `$`, `<`, `>` and `arg`; failure is
`$`, `?` and `exit` (`$?` is the explicit non-failing form); mutation and compile time are as in
Yan; concurrency is `par`, as in Yan.

Round trip (N3, N7): every construct above has one printing. The surface prints a Yan `main` back
as script mode when its body is in the subset and its header attenuations are literal; otherwise
`yan view --as ynsh` names the first construct that is not.

### 8.5 What the tooling gets for free

- `ynsh plan FILE` prints the authority manifest and every literal command without running
  anything: programs, path grants with modes, hosts, and which children are unconfined.
- `ynsh test FILE` runs `ex` examples and test blocks with `fake_proc`, a temp `fs` and a fake clock,
  so CI scripts get unit tests.
- An agent harness can accept a `.ynsh` file instead of a bash string: parse, show the plan, ask
  once, run exactly that tree.
- Parallel output from `par` commits in source order, so CI logs from T3 read package by package.

## 9. Open questions

- **D1. `{x}` or `\(x)` in command words?** The mix uses `{x}`, because Yan strings keep braces
  literal and `out/\(p.name).log` is noisy. The cost is two interpolation spellings in one file.
- **D2. Should `$` fail on non-zero exit by default?** Recommended yes, with `$?` as the opt-out. This
  diverges from R692's `proc.run` default, so the lowering adds `.check()`; it needs a `check` method
  or a `ShellError` in std.
- **D3. Parenless zero-argument stages** (`| .count` instead of `| .count()`). Shorter and nicer;
  breaks byte-for-byte agreement with Yan unless Yan adopts the same rule for the implicit subject.
- **D4. Default read authority.** Should a script read its working tree without saying so? The mix
  says no: `write` implies read and everything else needs `read:`. CI scripts may find this tedious.
- **D5. Process pipes before R693.** Ship the first slice with Yan-side pipelines only, or wait?
  Recommended: ship `$`, `$?`, `<`, redirects and dot stages first; program-to-program `|` after
  R693.
- **D6. `uses` header shape.** Data-like (`proc(cargo, git)`) as in the mix, or P3's `grant` lines?
  The mix keeps Yan's keyword so the header reads as a function's `uses`.
- **D7. Script mode in other surfaces.** Top-level statements as `main` would also suit Yan for small
  tools; that is a Yan decision, not a shell one.
- **D8. Windows.** argv-only `proc` avoids `cmd.exe` quoting, but Windows programs parse their own
  command line. PEP 787 hit the same wall. Probably: document and test the common tools only.
- **D9. Honest children (core and runtime, not syntax).** Make `proc` children inherit the
  attenuations in scope, enforced by Landlock on Linux, Seatbelt on macOS and `pledge`/`unveil` on
  OpenBSD, with `sees` and `net` clauses per program. Needs its own RFC; it would change what R674
  promises.
- **D10. Per-program argument policy.** GuardFall class E (`find -delete`) needs rules about flags,
  like PaSh's annotations but for authority. Research item.
- **D11. `atomic` blocks.** Rename-based staging is cheap and portable for one directory; anything
  wider needs filesystem snapshots. Worth it for agent-run scripts.
- **D12. Interactive use.** This study covers scripts. A REPL that accepts the same lines (with
  `uses` granted interactively, Deno-style prompts) is the natural next study.

## 10. Sources

- Oils YSH tour and reference: https://oilshell.org/release/latest/doc/ysh-tour.html ,
  https://oilshell.org/release/0.24.0/doc/ref/toc-ysh.html
- Nushell core-team notes on byte streams and redirection: https://hackmd.io/@nucore/rk0CgGDGR ;
  ByteStream change: https://github.com/nushell/nushell/pull/12774
- SHILL, OSDI 2014: https://usenix.org/conference/osdi14/technical-sessions/presentation/moore ;
  paper https://www.usenix.org/system/files/conference/osdi14/osdi14-paper-moore.pdf
- Greenberg, Kallas, Vasilakis, "Unix shell programming: the next 50 years", HotOS 2021:
  https://mgree.github.io/papers/hotos2021_shell.pdf
- Dong et al., "Bash in the Wild: Language Usage, Code Smells, and Bugs", TOSEM 2022:
  https://cs.uwaterloo.ca/~cnsun/publication/tosem22/tosem22.pdf
- BashFAQ 105, `set -e`: https://mywiki.wooledge.org/BashFAQ/105
- Vasilakis et al., "PaSh: Light-touch Data-Parallel Shell Processing": https://arxiv.org/abs/2007.09436
- PEP 787, safer subprocess usage with t-strings: https://peps.python.org/pep-0787/
- GuardFall research note, Cloud Security Alliance, July 2026:
  https://labs.cloudsecurityalliance.org/research/csa-research-note-guardfall-ai-agent-shell-injection-2026070/
- Fault-tolerant sandboxing for AI coding agents, arXiv 2512.12806: https://arxiv.org/abs/2512.12806v1
- Bun Shell: https://bun.sh/docs/runtime/shell ; Bun Shell vs zx: https://www.pkgpulse.com/guides/bun-shell-vs-zx-2026
- Amber at FOSDEM 2026: https://fosdem.org/schedule/event/GGLZS9-amber-lang-bash-transpiler
- Landlock userspace API: https://mirrors.lzu.edu.cn/git/linux-nf/src/branch/main/Documentation/userspace-api/landlock.rst
- Deno permissions: https://docs.deno.com/go/--allow-read
- Task runners: https://discourse.charmhub.io/t/make-vs-just-a-detailed-comparison/16097
