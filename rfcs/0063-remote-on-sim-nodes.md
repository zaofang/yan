# 0063: `Remote` on `std.sim`: which node code runs on, the `cluster` surface and the `sim` faults

- Status: accepted (D1 and D3 by lead cards, 2026-11-10)
- Research: none; follows RFC 0056 D9 (accepted, R960) and the survey in #929.
- Rows: R988 (D1, D6), R989 (D2); R417 admits listed cuts (D3); R992 (D3, D5) and R993 (monitor) with step 3

## Summary

RFC 0056 D9 and R417 say `cluster = sim(nodes, seed)` runs every node in one process, each with its
own root supervisor, a seed-derived node id and its own task-path prefix, and that `drop`, `delay` and
`partition` faults are drawn from the seed. No row says which node a piece of running code is on, yet
every fault is decided per ordered pair (sending node, receiving node), and `self_node()` (R400) must
answer something. This RFC fixes that model (D1), the step-1 surface of `std.cluster` (D2), the `sim`
constructor and its faults (D3), `RemoteError` and the fail types of a call through `Remote` (D4), and
the fault semantics (D5, D6). D1 and D3 are consequential; the rest follow the spec. The interpreter
implements step 1 of #929 (D1, D2, D4 and the no-fault path of D3) once D1 is accepted.

The interpreter runs a handler on the caller's task (`take`, run, `settle`; RFC 0056 D5), and native
does the same through thunks (D8). So a node cannot be a property of a task alone: a task that calls a
handler of an actor on another node runs that handler's body for the duration of the call.

## Decisions

### D1: which node code runs on

Every actor has a home node, fixed when it starts. Running code is on exactly one node, its current
node:

1. The task that binds `cluster = sim(n, seed)` is node 0, and so is everything that existed before the
   binding (its actors, its root supervisor). Nodes 1 to n-1 are new; each gets its own root supervisor
   (R957), whose children stop in reverse start order when the `with` block ends.
2. A task's node is inherited by the tasks it starts (`par` branches, `par for` bodies).
3. While a handler runs, the current node is its actor's home node; it returns to the caller's node when
   the handler settles. An `@reentrant` handler that lends the state keeps its actor's node.
4. An actor's home node is the current node where `actor.spawn` or `actor.start` runs, or the node
   `cluster.spawn(..., placement=)` chose (R412).

A plain `Actor[S, P]` handle never leaves its node: it is not `Wire`, so it cannot cross through a
`Remote` call or an off-node start. A call through `Actor[S, P]` is therefore always same-node and never
faulted. Only a call or send through `Remote[...]` crosses nodes, and its fault pair is (current node,
home node). `cluster.self_node()` returns the current node.

Options:

- (a) as above: node 0 is the binding task, handlers run on their actor's node.
- (b) node 0 is an observer outside the cluster: the binding task is on no node, every `Remote` call
  from it is faulted, and `self_node()` fails there. Simpler to state, but no real deployment has a
  caller outside every node, and actors made before the binding would belong nowhere.
- (c) a node is a property of the task only, entered explicitly by `cluster.call_remote(node, f, args)`
  (R407); handlers run on the caller's node. Breaks R411 (a node going down must take its actors with
  it) and makes `self_node()` inside a handler depend on who called.

Recommendation: (a). It matches a real node, where the process that binds `cluster` is a member, and
gives every actor one node that `NodeDown` and partitions can act on.

- Decision: (a), lead card, 2026-11-10
- Row: (one new row extending R417)

### D2: the `std.cluster` surface for step 1

`cluster` is the `Cluster` capability of section 7 (R400), declared in a new `std.cluster` module.
Step 1 adds:

- `Node`: an opaque `Wire + Eq + Hash` value holding the node id (R403); shown as 32 hex digits.
- `type Placement = Local() | On(node: Node) | Spread() | ByKey(key: bytes) | Near(node: Node)` (R412).
  Step 1 runs `Local` and `On`; the others fail loud with a diagnostic until a later step, never a
  silent `Local`.
- `cluster.nodes() -> List[Node]` in view order, `cluster.self_node() -> Node` (D1).
- `cluster.spawn(ChildSpec(start, (args...), restart=r), placement=Local(), capacity=1024)
  -> Remote[Actor[S, P]] fail RemoteError`. It reuses the `ChildSpec` of R958, as R958 says: an
  off-node start needs `start` to be `@remote` (E721) and the arguments `Wire` (E720), and the start runs
  on the chosen node with that node's capabilities (R407). The result is always `Remote`, so placement
  never changes the caller's type (R401). The actor is a child of the target node's root supervisor, or
  of the bound supervisor when the placement is the current node.

`call_remote`, `monitor`, `demonitor`, `view`, `entity` and `group` stay for later steps (#929 steps 3
and M4).

- Decision: as above, inconsequential, follows R400, R407, R412 and R958
- Row: (one new row in 9.6.1)

### D3: the `sim` constructor and its faults

`std.sim.sim(nodes: int, seed: int, faults: Faults = Faults()) -> std.cluster.Cluster`, with

```
class Faults:
    drop: float = 0.0          # chance each message (request or reply) is lost
    delay: Duration = 0 * s    # each message waits a uniform draw in [0, delay]
    partition: Duration = 0 * s  # mean time between partitions; 0 means none
    heal: Duration = 1 * s     # how long each partition lasts
    cuts: List[Cut] = []       # explicit partitions, in addition to the seeded ones

class Cut:
    at: Duration               # virtual time since the binding
    heal: Duration             # how long it lasts
    side: Set[int]             # node indices on one side; the rest are on the other
```

All draws come from the seed: each ordered node pair has its own stream, indexed by the message's
sequence number on that pair, so traffic between other nodes does not shift a pair's faults and
`explore` can vary one pair at a time. A partition splits the nodes into two non-empty sides drawn from
the seed. Time is the bound `time` handler's clock, so under `virtual` or `fixed_time` a delay advances
the clock like a sleep and a run repeats exactly; `drop=1.0` gives a deterministic `Unreachable`.

Options:

- (a) seeded rates only, as above (R417's "drawn from the seed").
- (b) (a) plus an explicit list of cuts, `cuts: List[Cut]` with `Cut(at, heal, side)`, for tests that
  need a partition at a known instant. Needs an R417 wording change, since a listed cut is not drawn
  from the seed.
- (c) a `Faults` handler trait the program implements, deciding each message itself. Most general, but
  a user handler can be nondeterministic and breaks seeded replay.

Recommendation: (a). It is what R417 says, keeps every run a function of the seed, and `drop=1.0` and a
short `partition` with a fixed seed already give deterministic tests. (b) can be added later without
breaking (a).

- Decision: (b), lead card, 2026-11-10. `Faults` gains `cuts: List[Cut]` as shown; R417's "drawn from
  the seed" becomes "drawn from the seed or listed as cuts".
- Row: (one new row in 9.6.1 for the constructor and `Faults`)

### D4: `RemoteError` and fail types

`std.cluster` declares `Unreachable`, `Rejected` and `SchemaMismatch` (empty classes) and
`type RemoteError = ActorError | Unreachable | Rejected | SchemaMismatch`, which is R408's "extends
`ActorError`": `Stopped(reason)` carries the `StopReason` of R977, including `NodeDown()`. A handler
call through `Remote[Actor[S, P]]` fails the handler's own fail type plus `RemoteError` (R408, 15's
actor paragraph). A send through `Remote[...]` fails `Undelivered[A, R]` whose `R` adds
`Unreachable | Rejected | SchemaMismatch` to the policy's reasons (R976). A call through `Remote` needs
`uses cluster` (R400); a handler of a same-node actor called through `Remote` needs it too, since the
type does not know the node.

- Decision: as above, inconsequential, follows R400, R408 and R976
- Row: none (R408, R976)

### D5: what a fault does to a call

- A dropped or partitioned request fails the call `Unreachable` once the message's delay has passed;
  the handler never runs.
- A dropped or partitioned reply fails the call `Unreachable` after the handler ran, so its effects
  stay: delivery is at most once (R408), never exactly once.
- A delayed message waits its delay on the clock under the caller's deadline; a deadline that expires
  first fails `Timeout`, and the remaining budget crosses with the request (R408).
- A send's admission is its request: a dropped send fails `Undelivered(reason=Unreachable(), args)`.
- Same-node calls through `Remote` are never faulted but still encode (R402).

- Decision: as above, inconsequential, follows RFC 0056 D9 ("a dropped message fails `Unreachable`")
- Row: none (R408, R960)

### D6: node ids and task paths

Node `i`'s id is the first 128 bits of BLAKE3 over `"yan.sim.node" || seed || i`, standing in for the
SPKI hash of R403, so ids repeat across runs and differ across seeds. Each node's tasks have the task
path prefix `[i]`, so `rand.split` streams (RFC 0019 D5) and traces differ per node and `yan replay
--merge` can tell them apart (R417).

- Decision: as above, inconsequential
- Row: none (R403, R417)

## Delivery

Step 1 of #929 on the interpreter: D1, D2, D4, D6 and `sim(nodes, seed)` without faults; the
`Remote` call path reuses `take`/`settle` and pushes the home node for the call. Cases go to a
`conform/*` PR: a two-node call, `self_node()` inside a handler on node 1, and an unsupported placement
diagnostic. Step 2 (always encode), step 3 (D3's faults, D5, `monitor`) and step 4 (native) follow.

## Spec changes

- 9.6.1: R417's fault sentence admits listed cuts (D3); a row after R417 for D1 and D6; a row for D2's `cluster.spawn`, `Node` and `Placement`; a row
  for D3's `sim` and `Faults`.
- 7: the `cluster` row points at `std.cluster`.
- 17.3 (std actors) or a new 17.x entry for `std.cluster` and `std.sim`.

## Step 3 as built

- Class fields take literal defaults only, so `Faults(...)` is a std constructor function that returns a `FaultPlan` with the fields of D3; `Cut` is a class.
- `monitor` delivers `Stopped(reason)`, the existing class, on a `Receiver[Stopped]`: the name `Down` is a variant of `std.math.Round`, and a std signature cannot name a class of another module. `demonitor` is dropping the receiver.
- A partition delivers `Stopped(reason=NodeDown())` to a monitor whose watcher it separates from the actor's node. Lead card 2026-11-10: delivery is lazy, checked at `monitor`, `sleep` and `recv` on the receiver (`simfault::sweep`), so no timer task exists and the result is a function of seed and clock.
- An `inout` argument is passed as an encoded copy and replaces its place once the reply arrives; a lost or unencodable reply leaves the place unchanged (`std.cluster.commit`).
- `cluster.spawn` start arguments round-trip through the codec; one that cannot be encoded panics `wire`, since the spawn has no failure of its own yet.
- `actor.send` through `Remote` is faulted at admission (D5): a lost request fails `Undelivered(reason=Unreachable(), args)`. Each send argument round-trips through the codec after the fault draw, and one the codec rejects fails the send `Undelivered(reason=SchemaMismatch(), args)` (R408).
