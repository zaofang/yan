//! std.net: TCP under `mem_net` and `net.only` (RFC 0032). The interpreter has no `os` handler.

use std::collections::{HashMap, VecDeque};

use yan_std::registry::{Id, id::*};

use super::bind::{Bind, bad_args};
use super::time::wait_out;
use super::{Interp, R, Value, unsupported};

const LISTENER: &str = "net.TcpListener";
const STREAM: &str = "net.TcpStream";
const FIRST_PORT: u32 = 49152;

#[derive(Default)]
pub(crate) struct Nets {
    nets: Vec<Network>,
    listeners: Vec<Listener>,
    streams: Vec<Stream>,
}

struct Network {
    bound: HashMap<String, usize>,
    next_port: u32,
}

struct Listener {
    addr: String,
    queued: VecDeque<usize>,
}

struct Stream {
    local: String,
    peer_addr: String,
    peer: usize,
    inbox: VecDeque<u8>,
    closed: bool,
}

fn handle(name: &str, i: usize) -> Value {
    Value::Data {
        name: name.into(),
        fields: vec![("id".into(), Value::Int(i as i128))],
    }
}

fn index(v: &Value) -> usize {
    match v {
        Value::Data { fields, .. } => match fields[0].1 {
            Value::Int(i) => i as usize,
            _ => unreachable!("a net handle"),
        },
        _ => unreachable!("a net handle"),
    }
}

fn text(v: &Value) -> &str {
    match v {
        Value::Str(s) => s,
        _ => unreachable!("a str"),
    }
}

fn error(addr: &str, reason: &str, message: &str) -> Value {
    Value::Data {
        name: "NetError".into(),
        fields: vec![
            ("addr".into(), Value::Str(addr.into())),
            (
                "reason".into(),
                Value::Data {
                    name: reason.into(),
                    fields: Vec::new(),
                },
            ),
            ("message".into(), Value::Str(message.into())),
        ],
    }
}

pub(super) fn show_error(fields: &[(super::Name, Value)]) -> String {
    format!("{}: {}", text(&fields[0].1), text(&fields[2].1))
}

fn split(addr: &str) -> Option<(&str, u32)> {
    let (host, port) = addr.rsplit_once(':')?;
    let port = port.parse().ok().filter(|p| *p <= u16::MAX as u32)?;
    Some((host, port))
}

/// Whether a `net.only` pattern, an exact `host:port` or `host:*`, grants `addr`.
fn grants(patterns: &Value, addr: &str) -> bool {
    let Value::List(ps) = patterns else {
        unreachable!("a list of patterns")
    };
    ps.iter().any(|p| {
        let p = text(p);
        p == addr
            || p.strip_suffix(":*")
                .is_some_and(|h| addr.rsplit_once(':').is_some_and(|(a, _)| a == h))
    })
}

/// The `mem_net` network under handler `h`, after every `net.only` layer granted `addr`.
fn network(h: &Value, addr: &str, listen: bool) -> R<Result<usize, Value>> {
    match h {
        Value::Data { name, fields } if name == "OnlyNet" => {
            let k = if listen { 2 } else { 1 };
            if grants(&fields[k].1, addr) {
                network(&fields[0].1, addr, listen)
            } else {
                Ok(Err(error(addr, "NotGranted", "not granted by net.only")))
            }
        }
        Value::Data { name, .. } if name == "MemNet" => Ok(Ok(index(h))),
        _ => unsupported("the os `net` handler in the interpreter"),
    }
}

fn listen(it: &mut Interp, h: &Value, addr: &str) -> R<Result<Value, Value>> {
    let n = match network(h, addr, true)? {
        Ok(n) => n,
        Err(e) => return Ok(Err(e)),
    };
    let Some((host, port)) = split(addr) else {
        return Ok(Err(error(addr, "OsError", "not a host:port address")));
    };
    let net = &mut it.net.nets[n];
    let addr = if port == 0 {
        net.next_port += 1;
        format!("{host}:{}", net.next_port - 1)
    } else {
        addr.to_owned()
    };
    if net.bound.contains_key(&addr) {
        return Ok(Err(error(&addr, "AddrInUse", "address in use")));
    }
    let l = it.net.listeners.len();
    net.bound.insert(addr.clone(), l);
    it.net.listeners.push(Listener {
        addr,
        queued: VecDeque::new(),
    });
    Ok(Ok(handle(LISTENER, l)))
}

fn connect(it: &mut Interp, h: &Value, addr: &str) -> R<Result<Value, Value>> {
    let n = match network(h, addr, false)? {
        Ok(n) => n,
        Err(e) => return Ok(Err(e)),
    };
    let net = &mut it.net.nets[n];
    let Some(&l) = net.bound.get(addr) else {
        return Ok(Err(error(addr, "Refused", "connection refused")));
    };
    let local = format!("127.0.0.1:{}", net.next_port);
    net.next_port += 1;
    let (c, s) = (it.net.streams.len(), it.net.streams.len() + 1);
    let stream = |local: &str, peer_addr: &str, peer| Stream {
        local: local.into(),
        peer_addr: peer_addr.into(),
        peer,
        inbox: VecDeque::new(),
        closed: false,
    };
    it.net.streams.push(stream(&local, addr, s));
    it.net.streams.push(stream(addr, &local, c));
    it.net.listeners[l].queued.push_back(s);
    Ok(Ok(handle(STREAM, c)))
}

/// Branches run one after another here, so a wait nothing has ended yet never ends: under a
/// deadline it fails with `Timeout` at the deadline (9.4), and otherwise the run stops.
fn blocked(it: &mut Interp, op: &str) -> R<Result<Value, Value>> {
    match wait_out(it) {
        Some(t) => Ok(Err(t)),
        None => unsupported(format!(
            "`{op}` waiting on another task, in the interpreter"
        )),
    }
}

fn send(it: &mut Interp, s: usize, b: &[u8]) -> Result<Value, Value> {
    let peer = it.net.streams[s].peer;
    if it.net.streams[s].closed || it.net.streams[peer].closed {
        return Err(error(
            &it.net.streams[s].peer_addr,
            "Closed",
            "connection closed",
        ));
    }
    it.net.streams[peer].inbox.extend(b);
    Ok(Value::Unit)
}

fn recv(it: &mut Interp, s: usize, max: i128) -> R<Result<Value, Value>> {
    let peer = it.net.streams[s].peer;
    let peer_closed = it.net.streams[peer].closed;
    let st = &mut it.net.streams[s];
    if st.closed {
        return Ok(Err(error(&st.peer_addr, "Closed", "connection closed")));
    }
    if st.inbox.is_empty() {
        return if peer_closed {
            Ok(Ok(Value::Bytes(Vec::new().into())))
        } else {
            blocked(it, "recv")
        };
    }
    let n = st.inbox.len().min(max.max(0) as usize);
    let b: Vec<u8> = st.inbox.drain(..n).collect();
    Ok(Ok(Value::Bytes(b.into())))
}

pub(super) static BINDS: &[(Id, Bind)] = &[
    (
        STD_TEST_MEM_NET,
        Bind::Cap(|it, _| {
            it.net.nets.push(Network {
                bound: HashMap::new(),
                next_port: FIRST_PORT,
            });
            Ok(handle("MemNet", it.net.nets.len() - 1))
        }),
    ),
    (
        STD_NET_ONLY,
        Bind::Plain(|a| match a {
            [h, connect, listen] => Ok(Value::Data {
                name: "OnlyNet".into(),
                fields: vec![
                    ("inner".into(), h.clone()),
                    ("connect".into(), connect.clone()),
                    ("listen".into(), listen.clone()),
                ],
            }),
            _ => bad_args(STD_NET_ONLY),
        }),
    ),
    (
        STD_NET_TCP_LISTEN,
        Bind::CapFallible(|it, a| match a {
            [h, addr] => listen(it, h, text(addr)),
            _ => bad_args(STD_NET_TCP_LISTEN),
        }),
    ),
    (
        STD_NET_TCP_CONNECT,
        Bind::CapFallible(|it, a| match a {
            [h, addr] => connect(it, h, text(addr)),
            _ => bad_args(STD_NET_TCP_CONNECT),
        }),
    ),
    (
        STD_NET_LISTENER_ACCEPT,
        Bind::CapFallible(
            |it, a| match it.net.listeners[index(&a[0])].queued.pop_front() {
                Some(s) => Ok(Ok(handle(STREAM, s))),
                None => blocked(it, "accept"),
            },
        ),
    ),
    (
        STD_NET_LISTENER_ADDR,
        Bind::Cap(|it, a| {
            Ok(Value::Str(
                it.net.listeners[index(&a[0])].addr.as_str().into(),
            ))
        }),
    ),
    (
        STD_NET_STREAM_SEND,
        Bind::CapFallible(|it, a| match a {
            [s, Value::Bytes(b)] => Ok(send(it, index(s), b)),
            _ => bad_args(STD_NET_STREAM_SEND),
        }),
    ),
    (
        STD_NET_STREAM_RECV,
        Bind::CapFallible(|it, a| match a {
            [s, Value::Int(max)] => recv(it, index(s), *max),
            _ => bad_args(STD_NET_STREAM_RECV),
        }),
    ),
    (
        STD_NET_STREAM_CLOSE,
        Bind::Cap(|it, a| {
            it.net.streams[index(&a[0])].closed = true;
            Ok(Value::Unit)
        }),
    ),
    (
        STD_NET_STREAM_LOCAL,
        Bind::Cap(|it, a| {
            Ok(Value::Str(
                it.net.streams[index(&a[0])].local.as_str().into(),
            ))
        }),
    ),
    (
        STD_NET_STREAM_PEER,
        Bind::Cap(|it, a| {
            Ok(Value::Str(
                it.net.streams[index(&a[0])].peer_addr.as_str().into(),
            ))
        }),
    ),
];
