# 0043: Native `par concurrent`: branches as resumable tasks over the enclosing frame

- Status: draft
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

Options: (a) every task runs over the frame of the function that holds the `par`; a resume point is a block
of that function's CFG, and a blocking operation can suspend only at task level, as in the interpreter; (b)
Rust-style coroutines: any function that may block becomes a state machine over a heap frame, so a wait
inside a nested call also suspends.

Recommendation: (a). A `par` is block-structured: the function does not return until every branch ends, so
the frame outlives the tasks and nothing has to move to the heap. Values live across a suspension are the
function's own locals; MIR lowering already gives every binding a fresh local (`lower.rs`, one local per
slot per bind), so branches never share one, and the transform asserts it. Cranelift's SSA construction
joins those locals through the dispatcher like any other loop. (b) changes the calling convention of every
function that can reach a channel, needs frame layout and drop of half-run frames, and buys only the nested
wait the interpreter does not support either. It stays a later RFC.

- Decision:

### D2: where the scheduling policy lives

Options: (a) in yan-rt, as a port of `run_tasks` (`yan-interp/src/vm.rs`) and the channel binds
(`yan-interp/src/sync.rs`); generated code only dispatches on the task index the runtime returns; (b) inline
in MIR as generated blocks.

Recommendation: (a). The order of output is observable, so the native policy must be the interpreter's to
the step: the progress counter, the "seen at progress" retry rule, the waiting-receiver list a rendezvous
send checks, and the bookkeeping done at step boundaries (a receive that succeeds after parking stays in the
waiting list until its task's step ends). Rust code in yan-rt can be ported line for line and unit-tested
against recorded interpreter traces; MIR blocks cannot. The cost is one runtime call per switch.

- Decision:

### D3: every task parked

The interpreter fails the lowest-index task's operation as a plain wait would: `Timeout` under a deadline,
otherwise it stops as unsupported. Native has no deadlines yet, and 9.6 and R396 say a global deadlock panics
with kind `deadlock`. Recommendation: native panics `deadlock` (8.3), and the interpreter follows in a small
separate PR, so the backends agree again.

- Decision:

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
  or returns ends the block"; the other tasks' owned locals are leaked for now (see Limits).

Nested `par concurrent` inside a branch is a nested region with its own handle; the runtime keeps a stack of
schedulers, so the inner block runs to completion within one step of the outer task, as in the interpreter.

Loops need nothing special. A resume point is the block of the blocked operation, so a loop that blocks
resumes at that operation with its counters intact in the frame, and the loop's back edge is untouched.

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

- A blocking operation inside a function called from a branch does not suspend (D1); it is the D3 case.
- A branch that returns or fails leaks the other tasks' live owned values, and a channel freed with elements
  queued leaks them. Cancel-all (R394) and drops of half-run tasks are follow-ups.
- No deadlines on native, so no `Timeout` from a wait.

## PR split

1. I001 on destructured non-copy tuples: `yan-check/src/post.rs`, `yan-opt/src/drops.rs`. Done when a
   channel program with no `par` gets past the check to MIR lowering, and the workspace tests pass.
2. `std.chan` in yan-rt, `Sender`/`Receiver` shapes and `Channel`, `send`, `recv` lowering outside `par`,
   with the Cranelift calls. Done when a buffered channel used by one task runs natively with the
   interpreter's output.
3. `Par`, `Suspend`, `TaskEnd`, the transform, the scheduler and its trace tests against the interpreter.
   Done when `par_concurrent_channel_progress.yag` prints `0 1 2` under `--backend cranelift`.
4. The interpreter's all-parked case panics `deadlock` (D3), if accepted.

## Spec changes

None. 9.1 and R390 already require this behaviour of every handler.
