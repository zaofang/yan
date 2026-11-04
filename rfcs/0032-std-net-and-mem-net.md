# 0032: A first `std.net` slice and the `mem_net` test handler

- Status: accepted (the lead chose "Accept all (D1 to D4)" on 2026-11-04)
- Research: [Language forums study](https://claude.ai/artifact/E8xVSw3fhTwuxFu38DqrtT), item F2
- Rows: R789 to R792

## Summary

The 7.1 capability table names `net` with `tcp_connect`, `tcp_listen`, `udp_bind` and `resolve`,
and 17.3 lists `std.net`, but no API exists, and the test handler set has "no net" (7.3, R377). So
any test touching `net` binds `os`, runs on real time and is never cached. Go 1.27 accepted the same
fix for `httptest` under `synctest`: a fake in-memory network.

This RFC fixes a first TCP slice of `std.net` and adds `mem_net()`, an in-memory handler, so
network code runs deterministically under the test handler set. A Yan-written handler of the same
trait stays possible (R672).

Deferred, each to its own later proposal:
- The `os` handler, `udp_bind`, `resolve`, `stream.split`, `listener.incoming`, `BufPool`, `Lease`,
  `recv_pooled` and `send_zc`, which wait for `Reader`, `Writer` and the event loop.
- A typed `SocketAddr`, CIDR grants and the post-DNS private-range rule (17.6), which come with the
  `os` handler.
- Latency and loss on `mem_net` driven by `fault = sim`, and TLS over it, which wait for `std.sim`
  and `std.tls`.

## Decisions

### D1: the TCP slice of `std.net`

`net` is a shared capability (7.4) with this trait:

- `net.tcp_listen(addr: str) -> TcpListener fail NetError`.
- `net.tcp_connect(addr: str) -> TcpStream fail NetError`.
- `net.only(connect: list[str] = [], listen: list[str] = []) -> Net`, see D3.

`TcpListener` and `TcpStream` are built-in handle types, like `Command` and `Sender`, so a program
cannot forge one. They are not `Wire` (9.8).

- `listener.accept() -> TcpStream fail NetError | Timeout` and `listener.addr() -> str`.
- `stream.send(b: bytes) -> None fail NetError`.
- `stream.recv(max: int) -> bytes fail NetError | Timeout`, which returns at most `max` bytes, and
  empty `bytes` once the peer has closed and every byte is read.
- `stream.close() -> None`, after which the peer reads end of stream. Closing twice does nothing.
- `stream.local() -> str` and `stream.peer() -> str`.

`accept` and `recv` wait. Under a deadline they fail with `Timeout` at the deadline (9.4, R472),
as channel `recv` does.

`NetError` has `addr: str`, `reason: NetReason` and `message: str`, where
`NetReason = Refused() | AddrInUse() | Closed() | NotGranted() | OsError()`. `show(e)` prints
`<addr>: <message>`.

Addresses are `host:port` strings in this slice; a typed `SocketAddr` comes with the `os` handler.

Recommendation: accept.

- Decision: accepted, 2026-11-04
- Row: R789

### D2: `mem_net()`

`std.test.mem_net() -> Net` is one in-memory network per handler value; two `mem_net()` handlers
share nothing.

- `tcp_listen(addr)` binds `addr` exactly. A port of `0` takes the next free port from 49152 up, in
  order, so the bound address is the same on every run. A bound address fails with `AddrInUse`.
- `tcp_connect(addr)` to an address with no listener fails with `Refused`. Otherwise the connection
  is queued on the listener at once, and `accept` takes queued connections in order. The client's
  local address is `127.0.0.1:<port>` with ports from 49152 up, from the same counter.
- Each direction is an unbounded byte queue. `send` appends to the peer's queue and fails with
  `Closed` once either end has closed. `recv` takes up to `max` bytes from its own queue.
- No operation takes time, so a test under `virtual` time is deterministic and cacheable.

Recommendation: accept.

- Decision: accepted, 2026-11-04
- Row: R790

### D3: `net.only`

`net.only(connect=..., listen=...)` attenuates any `net` handler. A pattern is an exact `host:port`
or `host:*`. An operation no pattern grants fails with `NotGranted` and the handler is never
reached. Nesting narrows: both layers must grant. CIDR patterns come with the `os` handler.

Recommendation: accept.

- Decision: accepted, 2026-11-04
- Row: R791

### D4: the test handler set keeps "no net"

7.3 is unchanged: a test that uses `net` binds `with net = mem_net():` itself. The option weighed
was making `mem_net()` the default. Explicit binding is recommended, because a test that reaches
the network should say so, and a listener and its client must share one handler value anyway.

Recommendation: accept.

- Decision: accepted, 2026-11-04
- Row: R792

## Spec changes

- 17.3: the `std.net` line names the slice and `mem_net`; the `test` line lists `mem_net()`.
- 7.1: the `net` row cites the new row.
