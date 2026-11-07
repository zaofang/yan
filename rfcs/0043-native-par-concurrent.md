# 0043: Native `par concurrent`: branches and blocking callees as resumable tasks

- Status: accepted
- Research: none; the gap is issue #615 against the interpreter of PR #628
- Rows: none expected; the behaviour is already 9.1 and R390, this note fixes how native code gets it

## Summary

Native code lowers `par` as its blocks in sequence and has no channel runtime, so
`conformance/yag/run/concurrency/par_concurrent_channel_progress.yag` cannot pass on Cranelift. This note
gives the compiled path the interpreter's model: each branch of a `par concurrent` becomes a resumable task,
split at its blocking channel operations by a MIR transform, with a `std.chan` runtime and the scheduling
policy in yan-rt. Native then prints what the interpreter prints, by construction.

Today the native run stops before any of this, with I001 "`_t1` was already consumed or dropped". That is
not about `par`: it comes from the destructuring `tx, rx = Channel[int](0)`, which optimized Core spells as
two projections of one non-copy tuple. The post check (`yan-check/src/post.rs`, the `views` map) treats each
`move` of a field view as a move of the whole root, so the second field fails. A program with a channel and
no `par` hits the same diagnostic.

## Decisions

### D1: where task state lives

Options: (a) every task runs over the frame of the function that holds the `par`, and a blocking operation
suspends only at task level, as in the interpreter; (b) Rust-style coroutines: the branches still run over
the enclosing frame, and every function that may block, called from a task, also gets a resumable variant
over a heap frame, so a wait inside a nested call suspends its task.

(a) is smaller, but it keeps the interpreter's "a wait inside a nested call" limit, which ordinary programs
hit as soon as a branch calls a helper that sends. (b) removes it and gives the frames that cancel-all (R394)
needs to drop half-run work. It costs a may-suspend analysis, a second variant of each such function, and a
frame layout.

- Decision: (b), the long-term option; Antonio, 2026-11-07

### D2: where the scheduling policy lives

Options: (a) in yan-rt, as a port of `run_tasks` (`yan-interp/src/vm.rs`) and the channel binds
(`yan-interp/src/sync.rs`); generated code only dispatches on the task index the runtime returns; (b) inline
in MIR as generated blocks.

Recommendation: (a). The order of output is observable, so the native policy must be the interpreter's to
the step: the progress counter, the "seen at progress" retry rule, the waiting-receiver list a rendezvous
send checks, and the bookkeeping done at step boundaries (a receive that succeeds after parking stays in the
waiting list until its task's step ends). Rust code in yan-rt can be ported line for line and unit-tested
against recorded interpreter traces; MIR blocks cannot. The cost is one runtime call per switch.

- Decision: (a); Antonio, 2026-11-07

### D3: every task parked

The interpreter fails the lowest-index task's operation as a plain wait would: `Timeout` under a deadline,
otherwise it stops as unsupported. Native has no deadlines yet, and 9.6 and R396 say a global deadlock panics
with kind `deadlock`. Recommendation: native panics `deadlock` (8.3) when every task at every level is parked, and the
interpreter follows in a small separate PR, so the backends agree again.

- Decision: native panics `deadlock`; Antonio, 2026-11-07

## Design

### MIR

Three terminators, emitted by lowering and removed by the transform before code generation:

- `Term::Par { tasks: Vec<BlockId>, join: BlockId }`: a `par concurrent` with two or more branches and no
  `rand` in scope (with `rand`, the interpreter runs branches in order, and so does native).
- `Term::Suspend { resume: BlockId }`: the current task yields; when scheduled again it continues at
  `resume`.
- `Term::TaskEnd`: the branch finished normally.

A blocking operation lowers to a retry loop. For `tx.send(v)`: block A copies `v`'s slots into a buffer and
calls `ChanSend(ch, buf, in_task)`; on status `BLOCKED` it goes to block B, `Suspend { resume: A }`; otherwise
on to the `Ok` path. `in_task` is a constant: true only when the operation is lowered inside a `Par` region
of this function, which is the interpreter's `park_ok`. Outside a task, a blocked operation is the D3 case.

### The transform (`yan-mir/src/tasks.rs`)

Per `Par` region, with `n` tasks:

- `Par` becomes `h = ParBegin(n)`, then a jump to a new dispatcher block.
- Dispatcher: `k = ParNext(h)`, then `Switch k` over one resume block per task and a last target that calls
  `ParEnd(h)` and jumps to `join`.
- Resume block of task `k`: `Switch state_k` over the task's entry and its resume points. `state_k` is a new
  `i64` local, 0 at `ParBegin`.
- `Suspend { resume }` in task `k`: `state_k = index of resume`, then a jump to the dispatcher. The park
  itself was recorded by the runtime inside the failed operation.
- `TaskEnd` in task `k`: `ParTaskEnd(h, k)`, then a jump to the dispatcher.
- `Return` or a failing exit inside a task: `ParEnd(h)` first. This is the interpreter's "a branch that fails
  or returns ends the block"; the other tasks are dropped by state (see Waits in nested calls).

Nested `par concurrent` inside a branch is a nested region with its own handle; the runtime keeps a stack of
schedulers, so the inner block runs to completion within one step of the outer task, as in the interpreter.

Loops need nothing special. A resume point is the block of the blocked operation, so a loop that blocks
resumes at that operation with its counters intact in the frame, and the loop's back edge is untouched.

### Waits in nested calls (D1 b)

- **May-suspend analysis.** A fixpoint over the MIR call graph: a function may suspend when it holds a
  blocking channel operation or a direct `Call` or `TailCall` of a function that may suspend. A function
  that holds a `par concurrent` does not by holding it; it may suspend only through its own task-level code
  outside the block.
- **Two variants.** A may-suspend function `g` keeps its plain variant, used outside any task, where a
  blocked operation is the D3 case. It also gets `g$co(frame: Ptr) -> status`, made by the same split as a
  branch: each blocking operation and each call of a may-suspend function is a resume point, and `state` is
  word 0 of the frame.
- **Frame.** Word 0 is the state, then the result slots, then one word per scalar local of `g$co`
  (parameters first; locals are scalars, so the frame is flat words). The child frame of a pending call is
  an ordinary local. Locals stay in registers within a run; the transform stores the locals live at the
  resume point (liveness on the MIR CFG) before a suspension and loads them on the resume edge.
- **Calls.** A task (or `g$co`) calling `h` that may suspend allocates `h`'s frame, stores the arguments, and
  loops on `st = h$co(frame)`: `SUSPENDED` suspends the caller with the same call as its resume point, the
  child frame kept in a live local; `DONE` loads the results and frees the frame. A resume therefore re-enters
  from the task down the chain of frames, as in Rust.
- **Nested `par`.** A `par concurrent` inside a function running as a coroutine, whose tasks are all parked,
  suspends the enclosing task instead of failing, so an outer sibling can unblock it. Deadlock is global: only
  the outermost scheduler panics `deadlock`.
- **Drop of a half-run frame.** Each `g$co` gets `g$drop(frame)`, which switches on the state and drops the
  owned locals the drop pass marks live at that point, then the child frame. `ParEnd` on an early exit drops
  the other tasks this way (R394). The branches over the enclosing frame get the same per-state drop blocks.

### yan-rt `std.chan`

- `ChanNew(cap, words) -> Ptr`: a channel of elements of `words` slots, with a FIFO of slots and a count of
  live ends. `Sender[T]` and `Receiver[T]` are one `Ptr` slot each, pointing at the same channel.
- `ChanSend(ch, buf, in_task) -> status` and `ChanRecv(ch, out, in_task) -> status`: the interpreter's rules,
  including "a rendezvous channel hands over only to a receiver already waiting". Blocked inside a task: the
  park `(is_recv, ch)` goes to the current scheduler and the status is `BLOCKED`.
- `ChanDrop(ch)`: one end gone; the channel is freed with its last end. Elements still queued are leaked.
- `ParBegin`, `ParNext`, `ParTaskEnd`, `ParEnd`: the scheduler of D2. `ParNext` with every live task parked
  and none retryable panics `deadlock` (D3).

### Ownership

`send` takes its value by `sink`: the slots are copied into the queue and the drop pass already treats the
source as consumed. `recv` hands the slots out as a new owned value. The ends stay owned by the enclosing
frame, branches use them `inout`, and the drops after the block are unchanged (`yan-opt/src/drops.rs` already
elaborates each branch on its own with `Flow::Normal`).

### I001

The post check and drop elaboration track moves per field of a destructured tuple: moving a field view moves
only that field, the root is gone once every non-copy field is moved, and the drop pass drops the fields
that were not moved. This is independent of `par` and lands first.

## Limits

- An indirect call (a closure or function value) does not suspend: a function value carries the plain
  variant only. Carrying the `$co` entry in function values changes their layout and is a follow-up.
- No deadlines on native, so no `Timeout` from a wait.
- The interpreter keeps its nested-call limit until it gets the same model; a program the interpreter stops
  on can run natively, but where both run the output is the same.

## PR split

1. I001 on destructured non-copy tuples: `yan-check/src/post.rs`, `yan-opt/src/drops.rs`. Done when a
   channel program with no `par` gets past the check to MIR lowering, and the workspace tests pass.
2. `std.chan` in yan-rt, `Sender`/`Receiver` shapes and `Channel`, `send`, `recv` lowering outside `par`,
   with the Cranelift calls. Done when a buffered channel used by one task runs natively with the
   interpreter's output.
3. `Par`, `Suspend`, `TaskEnd`, the task-level transform, the scheduler and its trace tests against the
   interpreter. Done when `par_concurrent_channel_progress.yag` prints `0 1 2` under `--backend cranelift`.
4. Coroutine variants: may-suspend analysis, frames with liveness spills, `$co` calls. Done when a branch that
   sends through a helper function makes progress natively.
5. Half-run drops (`$drop`, cancel-all on early exit) and nested `par` suspending its outer task.
6. The interpreter's all-parked case panics `deadlock` (D3).

## Spec changes

None. 9.1 and R390 already require this behaviour of every handler.
