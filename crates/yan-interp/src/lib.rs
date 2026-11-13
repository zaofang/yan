//! Interpreter over Yan IR text: `compile` lowers each function to register bytecode, `vm` runs it.
//!
//! `par` runs its branches in order, which M0 allows. Contracts run at the `check` level for
//! `requires` and, at every `ret`, `ensures` with `out`; `decreases` and `example` are not evaluated yet.

use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::hash::{BuildHasherDefault, Hash, Hasher};
use std::io::{BufRead, Read, Write};
use std::rc::Rc;

use indexmap::{IndexMap, IndexSet};
use num_bigint::BigInt;
use yan_core::ast::*;
use yan_diag::Span;

mod bind;
mod bits;
mod civil;
mod cli;
mod cluster;
mod collections;
mod compile;
mod crypto_rand;
mod csvwire;
mod ct;
mod encoding;
mod env;
mod fault;
mod iter;
mod json;
mod jsonwire;
mod keyed;
mod log;
mod math;
mod meta;
mod native;
mod net;
mod path;
mod prelude;
mod proc;
mod rand;
mod regex;
mod simcheck;
mod simfault;
#[cfg(test)]
mod std_parity;
#[cfg(test)]
mod std_selftest;
mod supervise;
mod sync;
mod time;
mod toolchain;
mod vm;

pub const EXIT_FAILURE: i32 = 1;
pub const EXIT_PANIC: i32 = 101;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum Value {
    #[default]
    Unit,
    Bool(bool),
    Int(i128),
    Float(f64),
    Char(char),
    Str(Rc<str>),
    Tuple(Vec<Value>),
    List(Vec<Value>),
    /// Insertion-ordered; removal keeps the order of the rest (std.dict).
    Dict(IndexMap<Key, Value, BuildHasherDefault<Fnv>>),
    Set(IndexSet<Key, BuildHasherDefault<Fnv>>),
    /// A deque and its `maxlen`.
    Deque(VecDeque<Value>, Option<usize>),
    /// A 4-ary min-heap in array order.
    Heap(Vec<Value>),
    /// Sorted by key.
    SortedMap(Vec<(Value, Value)>),
    SortedSet(Vec<Value>),
    BigInt(BigInt),
    /// A record or a sum variant.
    Data {
        name: Name,
        fields: Vec<(Name, Value)>,
    },
    Cap(Name),
    Bytes(Rc<[u8]>),
    ByteArray(Vec<u8>),
    Path(Rc<str>),
    /// A `Secret[T]` (RFC 0019 D7); it shows as `Secret(***)`.
    Secret(Rc<Value>),
    WindowsPath(Rc<path::WinPath>),
    Duration(i128),
    Instant(i128),
    Mono(i128),
    PlainDate(jiff::civil::Date),
    PlainTime(jiff::civil::Time),
    PlainDateTime(jiff::civil::DateTime),
    /// Years, months, weeks and days, with one sign.
    Period([i64; 4]),
    Zone(Rc<civil::Zone>),
    Closure(Rc<vm::Clo>),
    Iter(iter::It),
    /// A stage-0 type handle (RFC 0051 D1).
    Type(Rc<meta::Handle>),
}

/// FNV-1a: frames hash short names on every access, where SipHash dominated the profile.
#[derive(Default)]
pub(crate) struct Fnv(u64);

impl Hasher for Fnv {
    fn write(&mut self, bytes: &[u8]) {
        let mut h = if self.0 == 0 {
            0xcbf2_9ce4_8422_2325
        } else {
            self.0
        };
        for b in bytes {
            h = (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3);
        }
        self.0 = h;
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

pub(crate) type Map<K, V> = HashMap<K, V, BuildHasherDefault<Fnv>>;

/// A dict key or set element. Only hashable values reach here; the checker rejects the rest.
#[derive(Clone, Debug, PartialEq)]
struct Key(Value);

impl Eq for Key {}

impl Hash for Key {
    fn hash<H: Hasher>(&self, h: &mut H) {
        fn go<H: Hasher>(v: &Value, h: &mut H) {
            std::mem::discriminant(v).hash(h);
            match v {
                Value::Bool(b) => b.hash(h),
                Value::Int(i) => i.hash(h),
                Value::Float(f) => (if *f == 0.0 { 0.0f64 } else { *f }).to_bits().hash(h),
                Value::Char(c) => c.hash(h),
                Value::Str(s) => s.hash(h),
                Value::Bytes(b) => b.hash(h),
                Value::Path(s) => s.hash(h),
                Value::Duration(n) | Value::Instant(n) | Value::Mono(n) => n.hash(h),
                Value::WindowsPath(w) => w.folded().hash(h),
                Value::PlainDate(d) => d.hash(h),
                Value::PlainTime(t) => t.hash(h),
                Value::PlainDateTime(dt) => dt.hash(h),
                Value::Period(p) => p.hash(h),
                Value::Zone(z) => z.id().hash(h),
                Value::BigInt(b) => b.hash(h),
                Value::Tuple(xs) | Value::List(xs) => xs.iter().for_each(|x| go(x, h)),
                Value::Data { name, fields } => {
                    name.hash(h);
                    fields.iter().for_each(|(_, x)| go(x, h));
                }
                Value::Type(t) => format!("{:?}", t.ty).hash(h),
                _ => {}
            }
        }
        go(&self.0, h)
    }
}
/// Why a run stopped early.
#[derive(Debug)]
pub enum Stop {
    /// A panic (8.3).
    Panic(Box<Panic>),
    /// A construct this interpreter does not run yet: a tool failure, not a program result.
    Unsupported(String),
    /// A budget ran out (10.4, R676): `depth`, `steps`, `heap` or `time`.
    Limit(&'static str),
}

/// Budgets (10.4, R487, R676): call depth, steps counted as executed calls and loop
/// iterations, bytes allocated above what the process held at the start (counted only under
/// [`yan_rt::heap::Counting`]), and a wall-clock deadline.
#[derive(Clone, Copy)]
pub struct Limits {
    pub depth: usize,
    pub steps: u64,
    pub heap: usize,
    pub deadline: Option<std::time::Instant>,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            depth: 1_000,
            steps: 50_000_000,
            heap: usize::MAX,
            deadline: None,
        }
    }
}

/// What a stage-0 evaluation gives: a value that can cross into run time, or the kind of
/// value that cannot (E262).
pub enum Stage0 {
    Value(yan_core::ast::ConstVal),
    NotMaterializable(&'static str),
    /// A `Type` made by `record_type`, `sum_type` or `newtype` (RFC 0051 D3).
    Type(Reified),
}

/// A stage-0 `Type` result: the resolved type, and the declarations that reification made, each
/// before any that mention it.
#[derive(Debug)]
pub struct Reified {
    pub ty: yan_core::ast::Type,
    pub decls: Vec<yan_core::ast::Item>,
}

/// Evaluate nullary `fn f` of `m` at stage 0, on a thread with room for deep recursion.
#[cfg(not(target_family = "wasm"))]
pub fn stage0(m: &Module, f: &str, limits: Limits) -> R<Stage0> {
    std::thread::scope(|s| {
        std::thread::Builder::new()
            .stack_size(1 << 30)
            .spawn_scoped(s, || stage0_here(m, f, limits))
            .expect("spawn the stage-0 thread")
            .join()
            .unwrap_or_else(|_| unsupported("the stage-0 evaluator crashed"))
    })
}

/// WebAssembly in the browser has no threads; the playground links a large stack instead.
#[cfg(target_family = "wasm")]
pub fn stage0(m: &Module, f: &str, limits: Limits) -> R<Stage0> {
    stage0_here(m, f, limits)
}

fn stage0_here(m: &Module, f: &str, limits: Limits) -> R<Stage0> {
    let program = compile::compile(m);
    let Some(code) = program.fns.get(f).copied() else {
        return unsupported(format!("no `fn {f}` to evaluate"));
    };
    let mut sink = std::io::sink();
    let mut vm = vm::Vm::new(
        program,
        Interp {
            out: &mut sink,
            clocks: Vec::new(),
            rngs: Vec::new(),
            args: Vec::new(),
            ceiling: Default::default(),
            deadline: None,
            chans: Vec::new(),
            actors: Vec::new(),
            sups: Default::default(),
            actor_waits: Vec::new(),
            nodes: Vec::new(),
            placed: None,
            sim: None,
            pct: None,
            monitors: Vec::new(),
            groups: Vec::new(),
            seq: Default::default(),
            remote: Vec::new(),
            replied: true,
            sending: None,
            wait_since: Vec::new(),
            doomed: Vec::new(),
            net: Default::default(),
            test: false,
            progress: 0,
            waiting_recv: Vec::new(),
            park_ok: false,
            park: None,
            force: false,
            in_par: 0,
            cancel: Vec::new(),
            task_done: false,
            host: None,
        },
    );
    vm.limit(limits);
    match vm.call(code, Vec::new())?.0 {
        Ok(v) => Ok(meta::reified(&v).map_or_else(|| materialize(&v), Stage0::Type)),
        Err(_) => unsupported("a failing stage-0 call"),
    }
}

/// 10.4: scalars, strings, lists, tuples and data of them cross into run time.
fn materialize(v: &Value) -> Stage0 {
    use yan_core::ast::{ConstVal as C, Literal as L};
    fn go(v: &Value) -> Result<C, &'static str> {
        Ok(match v {
            Value::Unit => C::Lit(L::Unit),
            Value::Bool(b) => C::Lit(L::Bool(*b)),
            Value::Int(n) => C::Lit(L::Int(n.to_string())),
            Value::Float(f) => C::Lit(L::Float(format!("{f:?}"))),
            Value::Char(c) => C::Lit(L::Char(*c)),
            Value::Str(s) => C::Lit(L::Str(s.to_string())),
            Value::List(xs) => C::List(xs.iter().map(go).collect::<Result<_, _>>()?),
            Value::Tuple(xs) => C::Tuple(xs.iter().map(go).collect::<Result<_, _>>()?),
            Value::Data { name, fields } => C::Path {
                path: vec![name.clone()],
                fields: Some(
                    fields
                        .iter()
                        .map(|(n, x)| Ok::<_, &str>((n.clone(), go(x)?)))
                        .collect::<Result<_, _>>()?,
                ),
            },
            Value::Closure(_) => return Err("a function value"),
            Value::Type(_) => return Err("a `Type`"),
            _ => return Err("this value"),
        })
    }
    match go(v) {
        Ok(c) => Stage0::Value(c),
        Err(k) => Stage0::NotMaterializable(k),
    }
}

/// A panic's kind, message and the frames it unwound through, innermost first.
#[derive(Debug)]
pub struct Panic {
    pub kind: String,
    pub message: String,
    pub trace: Vec<Frame>,
    /// Named operands (R796); never text, collections, `Secret` or `Untrusted`.
    pub fields: Vec<(String, Operand)>,
}

/// A panic operand: a number, bool or unit-carrying value, else only its type name (R796).
#[derive(Debug, Clone, PartialEq)]
pub enum Operand {
    Int(i128),
    Float(f64),
    Bool(bool),
    /// Nanoseconds of a duration, instant or monotonic reading.
    Nanos(i128),
    Type(&'static str),
    /// An item identity hash (R874).
    Hash([u8; 32]),
}

impl Operand {
    pub(crate) fn of(v: &Value) -> Operand {
        match v {
            Value::Int(n) => Operand::Int(*n),
            Value::Float(x) => Operand::Float(*x),
            Value::Bool(b) => Operand::Bool(*b),
            Value::Duration(n) | Value::Instant(n) | Value::Mono(n) => Operand::Nanos(*n),
            Value::Unit => Operand::Type("Unit"),
            Value::Secret(_) => Operand::Type("Secret"),
            Value::Str(_) | Value::Char(_) => Operand::Type("Text"),
            Value::List(_) | Value::Tuple(_) | Value::Dict(_) | Value::Set(_) => {
                Operand::Type("Collection")
            }
            _ => Operand::Type("Value"),
        }
    }
}

#[derive(Debug)]
pub struct Frame {
    pub item: String,
    pub at: Span,
}

impl Panic {
    /// `panic(kind): message` and one `  at item line:col` line per frame (8.3).
    pub fn render(&self, path: &str) -> String {
        let mut s = format!("panic({}): {}", self.kind, self.message);
        for f in &self.trace {
            s += &format!("\n  at {} {path}:{}:{}", f.item, f.at.line, f.at.col);
        }
        s
    }
}

pub(crate) type R<T> = Result<T, Stop>;

pub(crate) struct Interp<'o> {
    out: &'o mut dyn Write,
    /// Nanoseconds slept under each `fixed_time` or `virtual` handler, by handler.
    clocks: Vec<i128>,
    /// The generator of each `seeded` handler, by handler.
    rngs: Vec<rand::Gen>,
    /// The program's arguments under the host `env` handler.
    args: Vec<String>,
    /// What a child process may touch, computed once from the entry handlers (RFC 0065 D5).
    ceiling: yan_sandbox::Ceiling,
    /// The earliest enclosing `deadline` scope, as monotonic nanoseconds (9.4).
    deadline: Option<i128>,
    /// Each channel's capacity and queued values, by channel.
    chans: Vec<(usize, VecDeque<Value>)>,
    /// Each actor, by actor (9.6).
    actors: Vec<sync::Actor>,
    /// Every supervisor and the open `with supervisor` scopes (9.6).
    sups: supervise::Sups,
    /// The tasks parked on a busy actor, by task path, and the actor each waits for (RFC 0056 D6).
    actor_waits: Vec<(Vec<usize>, usize)>,
    /// The nodes tasks entered, innermost last, by task path (RFC 0063 D1).
    nodes: Vec<(Vec<usize>, usize)>,
    /// The node a `cluster.spawn` placed its start on, when not the caller's (RFC 0063 D2).
    placed: Option<usize>,
    /// The `sim` handler last built: nodes, faults, seed and the clock at its binding (RFC 0063 D3).
    sim: Option<Value>,
    /// The PCT scheduler last built, which chooses the next task of a `par concurrent` (R999).
    pct: Option<yan_rt::pct::Pct>,
    /// Each monitored actor, the channel its `Down` goes to and the node of the watcher (RFC 0063 D7).
    monitors: Vec<(usize, usize, usize)>,
    /// The members of each named group, in join order (R414).
    groups: Vec<(String, Vec<Value>)>,
    /// Messages sent so far over each ordered node pair, which index its fault stream.
    seq: std::collections::HashMap<(usize, usize), u64>,
    /// The calls through `Remote` awaiting their reply, innermost last, by task path.
    remote: Vec<(Vec<usize>, simfault::Pending)>,
    /// Whether the last reply through `Remote` arrived, for `std.cluster.commit`.
    replied: bool,
    /// The failure of the send through `Remote` about to be made, by task path.
    sending: Option<(Vec<usize>, Value)>,
    /// When each task waiting on an actor began to wait, for the 5 s default deadline (9.6).
    wait_since: Vec<(Vec<usize>, i128)>,
    /// Parked callers that a wait cycle chose to fail `Deadlock` when they next retry (D6).
    doomed: Vec<Vec<usize>>,
    /// Every `mem_net` network, listener and stream end.
    net: net::Nets,
    /// Running under `yan test`, which picks the default handlers (7.3).
    test: bool,
    /// Counts channel progress: a transfer, or a task starting to wait for a receive.
    progress: u64,
    /// The channels tasks of a `par concurrent` are parked receiving on.
    waiting_recv: Vec<usize>,
    /// A blocking operation may park its task: set by the VM for calls made at task level.
    park_ok: bool,
    /// Set by a blocking operation that parked: (is a receive, channel).
    park: Option<(bool, usize)>,
    /// Every task is parked: fail the next blocking operation as a plain wait would.
    force: bool,
    /// How many `par concurrent` blocks are running tasks.
    in_par: u32,
    /// Per running `par concurrent` block, innermost last: a task left it and cancelled the
    /// rest (9.4).
    cancel: Vec<bool>,
    /// Set when a task's branch ends.
    task_done: bool,
    /// Answers `toolchain` queries; set only by [`query`].
    host: Option<&'o dyn Host>,
}

pub(crate) fn panic<T>(kind: &str) -> R<T> {
    panic_with(kind, yan_rt::abi::panic_message(kind))
}

pub(crate) fn panic_with<T>(kind: &str, message: impl Into<String>) -> R<T> {
    Err(Stop::Panic(Box::new(Panic {
        kind: kind.to_owned(),
        message: message.into(),
        trace: Vec::new(),
        fields: Vec::new(),
    })))
}

pub(crate) fn panic_fields<T>(
    kind: &str,
    message: impl Into<String>,
    fields: Vec<(impl Into<String>, Operand)>,
) -> R<T> {
    Err(Stop::Panic(Box::new(Panic {
        kind: kind.to_owned(),
        message: message.into(),
        trace: Vec::new(),
        fields: fields.into_iter().map(|(n, o)| (n.into(), o)).collect(),
    })))
}

/// A missing key as panic operands: the first 64 bits of BLAKE3 over its canonical encoding and
/// its type name (R876).
pub(crate) fn missing_key(k: &Value) -> Vec<(&'static str, Operand)> {
    fn enc(v: &Value, out: &mut yan_rt::keyhash::Enc) {
        match v {
            Value::Unit => out.unit(),
            Value::Bool(b) => out.bool(*b),
            Value::Int(n) => out.int(*n),
            Value::Float(x) => out.float(*x),
            Value::Char(c) => out.char(*c),
            Value::Str(s) | Value::Path(s) => out.text(s),
            Value::Bytes(b) => out.bytes(b),
            Value::Duration(n) | Value::Instant(n) | Value::Mono(n) => out.nanos(*n),
            Value::Tuple(xs) | Value::List(xs) => {
                out.seq(xs.len());
                xs.iter().for_each(|x| enc(x, out));
            }
            Value::Data { name, fields } => {
                out.data(name);
                fields.iter().for_each(|(_, x)| enc(x, out));
            }
            Value::BigInt(b) => out.bigint(&b.to_string()),
            Value::WindowsPath(w) => out.win_path(w.folded()),
            Value::PlainDate(d) => out.civil(0, &d.to_string()),
            Value::PlainTime(t) => out.civil(1, &t.to_string()),
            Value::PlainDateTime(dt) => out.civil(2, &dt.to_string()),
            Value::Period([y, m, w, d]) => out.period(&format!("{y}Y{m}M{w}W{d}D")),
            Value::Zone(z) => out.zone(z.id()),
            _ => out.opaque(),
        }
    }
    let mut buf = yan_rt::keyhash::Enc::new();
    enc(k, &mut buf);
    let head = buf.hash();
    let ty = match k {
        Value::Int(_) => "int",
        Value::Float(_) => "float",
        Value::Bool(_) => "bool",
        Value::Char(_) => "char",
        Value::Str(_) => "text",
        Value::Bytes(_) => "bytes",
        Value::Tuple(_) => "tuple",
        Value::List(_) => "list",
        Value::Data { .. } => "record",
        Value::BigInt(_) => "bigint",
        Value::Path(_) | Value::WindowsPath(_) => "path",
        Value::PlainDate(_) => "date",
        Value::PlainTime(_) => "time",
        Value::PlainDateTime(_) => "datetime",
        Value::Period(_) => "period",
        Value::Zone(_) => "zone",
        Value::Duration(_) => "duration",
        Value::Instant(_) => "instant",
        Value::Mono(_) => "mono",
        _ => "value",
    };
    vec![
        ("key_hash", Operand::Int(i128::from(head))),
        ("key_type", Operand::Type(ty)),
    ]
}

pub(crate) fn out_of_range<T>(i: i128, len: usize) -> R<T> {
    panic_fields(
        "bounds",
        format!("index {i} is out of range for length {len}"),
        vec![
            ("index", Operand::Int(i)),
            ("length", Operand::Int(len as i128)),
        ],
    )
}

pub(crate) fn slice_out_of_range<T>(i: i128, j: i128, len: usize) -> R<T> {
    panic_with(
        "bounds",
        format!("slice {i}:{j} is out of range for length {len}"),
    )
}

pub(crate) fn unsupported<T>(what: impl Into<String>) -> R<T> {
    Err(Stop::Unsupported(what.into()))
}

/// Run `fn main` of `m`, writing program output to `out`. Returns the exit status.
pub use toolchain::Host;

pub fn run(m: &Module, out: &mut dyn Write) -> R<i32> {
    run_with(m, out, Vec::new())
}

/// [`run`] with the program arguments `env.args()` returns.
pub fn run_with(m: &Module, out: &mut dyn Write, args: Vec<String>) -> R<i32> {
    run_reporting(m, out, args).map(|(status, _)| status)
}

/// [`run_with`], also giving the shown failure value when `main` failed.
pub fn run_reporting(
    m: &Module,
    out: &mut dyn Write,
    args: Vec<String>,
) -> R<(i32, Option<String>)> {
    let mut code = None;
    Ok(
        match entry(m, "main", out, None, args, &mut 0, Mode::Run, &mut code)? {
            Ok(_) => (0, None),
            Err(v) => (code.unwrap_or(EXIT_FAILURE), Some(prelude::show(&v, false))),
        },
    )
}

/// Run the test `fn f` of `m` under `limits`, on a thread with room for deep recursion.
/// `Ok(false)` means it returned a failure.
pub fn test(m: &Module, f: &str, limits: Limits, out: &mut (dyn Write + Send)) -> R<bool> {
    test_metered(m, f, limits, out, &mut 0)
}

/// [`test`], adding the steps it took to `spent`.
pub fn test_metered(
    m: &Module,
    f: &str,
    limits: Limits,
    out: &mut (dyn Write + Send),
    spent: &mut u64,
) -> R<bool> {
    std::thread::scope(|s| {
        std::thread::Builder::new()
            .stack_size(1 << 30)
            .spawn_scoped(s, || {
                entry(
                    m,
                    f,
                    out,
                    Some(limits),
                    Vec::new(),
                    spent,
                    Mode::Run,
                    &mut None,
                )
                .map(|r| r.is_ok())
            })
            .expect("spawn the test thread")
            .join()
            .unwrap_or_else(|_| unsupported("the interpreter crashed"))
    })
}

/// Run the nullary `fn f` of `m`, which may use the `toolchain` capability, under `limits`
/// (21.4). `Ok` is what it returned as JSON, `Err` its failure value as text.
pub fn query(
    m: &Module,
    f: &str,
    limits: Limits,
    out: &mut (dyn Write + Send),
    host: &(dyn Host + Sync),
    spent: &mut u64,
) -> R<Result<serde_json::Value, String>> {
    std::thread::scope(|s| {
        std::thread::Builder::new()
            .stack_size(1 << 30)
            .spawn_scoped(s, || {
                entry(
                    m,
                    f,
                    out,
                    Some(limits),
                    Vec::new(),
                    spent,
                    Mode::Query(host),
                    &mut None,
                )
                .map(|r| r.map(|v| json_of(&v)).map_err(|v| prelude::show(&v, false)))
            })
            .expect("spawn the query thread")
            .join()
            .unwrap_or_else(|_| unsupported("the interpreter crashed"))
    })
}

/// Call the served tool `fn f` of `m` (RFC 0062) with `args`, a json object, under `limits`; `fs`
/// is confined beneath `root`. `Ok` is the result as json text; `Err` the failure value or the
/// argument error as text.
pub fn tool(
    m: &Module,
    f: &str,
    args: &str,
    root: &str,
    limits: Limits,
    out: &mut (dyn Write + Send),
    spent: &mut u64,
) -> R<Result<String, String>> {
    let Some(sc) = tool_schema(m, f) else {
        return unsupported(format!("`{f}` is not a tool"));
    };
    std::thread::scope(|s| {
        std::thread::Builder::new()
            .stack_size(1 << 30)
            .spawn_scoped(s, || {
                let args = match jsonwire::tool_args(&sc, args) {
                    Ok(v) => v,
                    Err(e) => return Ok(Err(prelude::show(&e, false))),
                };
                let call = ToolCall {
                    args,
                    root: root.into(),
                };
                let r = entry(
                    m,
                    f,
                    out,
                    Some(limits),
                    Vec::new(),
                    spent,
                    Mode::Tool(call),
                    &mut None,
                )?;
                Ok(match r {
                    Ok(v) => match jsonwire::tool_result(&sc, &v) {
                        Some(j) => Ok(j),
                        None => return panic("value"),
                    },
                    Err(v) => Err(prelude::show(&v, false)),
                })
            })
            .expect("spawn the tool thread")
            .join()
            .unwrap_or_else(|_| unsupported("the interpreter crashed"))
    })
}

/// The schema elaboration attached to the tool `fn f` (RFC 0062).
fn tool_schema(m: &Module, f: &str) -> Option<serde_json::Value> {
    m.items.iter().find_map(
        |i| match (&i.kind, i.attrs.iter().find(|a| a.name == "tool")) {
            (ItemKind::Fn(d), Some(a)) if d.path.join(".") == f => match &a.args.as_ref()?[0].value
            {
                yan_core::ast::ConstVal::Lit(yan_core::ast::Literal::Str(t)) => {
                    serde_json::from_str::<serde_json::Value>(t)
                        .ok()?
                        .get("schema")
                        .cloned()
                }
                _ => None,
            },
            _ => None,
        },
    )
}

/// What `entry` runs: a program or test, a toolchain query, or a served tool call.
enum Mode<'a> {
    Run,
    Query(&'a dyn Host),
    Tool(ToolCall),
}

/// A served tool call: its decoded arguments and the root its `fs` is confined beneath.
struct ToolCall {
    args: Vec<Value>,
    root: String,
}

fn json_of(v: &Value) -> serde_json::Value {
    use serde_json::Value as J;
    match v {
        Value::Unit => J::Null,
        Value::Bool(b) => J::Bool(*b),
        Value::Int(n) => i64::try_from(*n).map_or_else(|_| J::String(n.to_string()), J::from),
        Value::Float(x) => serde_json::Number::from_f64(*x).map_or(J::Null, J::Number),
        Value::Str(s) => J::String(s.to_string()),
        Value::List(xs) | Value::Tuple(xs) => J::Array(xs.iter().map(json_of).collect()),
        v => J::String(prelude::show(v, false)),
    }
}

/// Call `fn f` of `m` with its capabilities bound; `false` when it returns a failure.
#[allow(clippy::too_many_arguments)]
fn entry(
    m: &Module,
    f: &str,
    out: &mut dyn Write,
    limits: Option<Limits>,
    argv: Vec<String>,
    spent: &mut u64,
    mode: Mode,
    exit: &mut Option<i32>,
) -> R<Result<Value, Value>> {
    let (host, tool) = match mode {
        Mode::Run => (None, None),
        Mode::Query(h) => (Some(h), None),
        Mode::Tool(t) => (None, Some(t)),
    };
    let program = compile::compile(m);
    let methods = program.methods.clone();
    let Some(code) = program.fns.get(f).copied() else {
        return unsupported(format!("no `fn {f}` in the module"));
    };
    let def = m
        .items
        .iter()
        .rev()
        .find_map(|i| match &i.kind {
            ItemKind::Fn(d) if d.path.join(".") == f => Some(d),
            _ => None,
        })
        .expect("a fn item");
    let (mut vals, root) = match tool {
        Some(t) => (t.args.into_iter(), Some(t.root)),
        None => (Vec::new().into_iter(), None),
    };
    let args = def
        .params
        .iter()
        .map(|p| match p {
            Param::Cap { name, .. } if host.is_some() && name != "toolchain" => unsupported(
                format!("`{f}` uses `{name}`; a query may use only `toolchain`"),
            ),
            // A tool's `fs` resolves beneath the served root (RFC 0062, R674).
            Param::Cap { name, .. } if name == "fs" && root.is_some() => Ok(Value::Data {
                name: "SubFs".into(),
                fields: vec![
                    ("inner".into(), Value::Cap(name.clone())),
                    (
                        "dir".into(),
                        Value::Str(root.clone().unwrap_or_default().into()),
                    ),
                ],
            }),
            Param::Cap { name, .. } => Ok(Value::Cap(name.clone())),
            Param::Value { .. } => vals
                .next()
                .map_or_else(|| unsupported(format!("`{f}` with value parameters")), Ok),
        })
        .collect::<R<Vec<_>>>()?;
    let mut it = Interp {
        out,
        clocks: Vec::new(),
        rngs: Vec::new(),
        args: argv,
        ceiling: proc::ceiling(&args),
        deadline: None,
        chans: Vec::new(),
        actors: Vec::new(),
        sups: Default::default(),
        actor_waits: Vec::new(),
        nodes: Vec::new(),
        placed: None,
        sim: None,
        pct: None,
        monitors: Vec::new(),
        groups: Vec::new(),
        seq: Default::default(),
        remote: Vec::new(),
        replied: true,
        sending: None,
        wait_since: Vec::new(),
        doomed: Vec::new(),
        net: Default::default(),
        test: limits.is_some() && root.is_none(),
        progress: 0,
        waiting_recv: Vec::new(),
        park_ok: false,
        park: None,
        force: false,
        in_par: 0,
        cancel: Vec::new(),
        task_done: false,
        host,
    };
    // The test handler set (7.3): `rand = seeded(0)`, `env = fake_env()`, `proc = fake_proc()`.
    let args = args
        .into_iter()
        .map(|a| match a {
            a if root.is_some() => a,
            Value::Cap(n) if limits.is_some() && n == "rand" => rand::seeded(&mut it, 0, "ChaCha8"),
            Value::Cap(n) if limits.is_some() && n == "env" => env::test_default(),
            Value::Cap(n) if limits.is_some() && n == "proc" => proc::test_default(),
            a => a,
        })
        .collect();
    let mut vm = vm::Vm::new(program, it);
    if let Some(l) = limits {
        vm.limit(l);
    }
    let r = vm.call(code, args);
    *spent += vm.steps();
    let r = r?.0;
    // A failing `main` exits with the `code` of its error's `Exit` impl (8.1, R430).
    if let (Err(Value::Data { name, .. }), true) = (&r, f == "main")
        && let Some(&c) = methods.get(format!("{name}.code").as_str())
        && let (Ok(Value::Int(n)), _) =
            vm.call(c, vec![r.as_ref().err().cloned().unwrap_or(Value::Unit)])?
    {
        *exit = i32::try_from(n).ok();
    }
    Ok(r)
}

pub(crate) fn int_range(ty: &str) -> Option<(i128, i128)> {
    Some(match ty {
        "i8" => (i8::MIN.into(), i8::MAX.into()),
        "i16" => (i16::MIN.into(), i16::MAX.into()),
        "i32" => (i32::MIN.into(), i32::MAX.into()),
        "i64" => (i64::MIN.into(), i64::MAX.into()),
        "i128" => (i128::MIN, i128::MAX),
        "u8" => (0, u8::MAX.into()),
        "u16" => (0, u16::MAX.into()),
        "u32" => (0, u32::MAX.into()),
        "u64" => (0, u64::MAX.into()),
        "Addr" => (0, u64::MAX.into()),
        _ => return None,
    })
}

fn parse_int(s: &str) -> R<i128> {
    let t: Cow<str> = if s.contains('_') {
        Cow::Owned(s.replace('_', ""))
    } else {
        Cow::Borrowed(s)
    };
    let (neg, t) = match t.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, t.as_ref()),
    };
    let (radix, digits) = match t.get(..2) {
        Some("0x") => (16, &t[2..]),
        Some("0o") => (8, &t[2..]),
        Some("0b") => (2, &t[2..]),
        _ => (10, t),
    };
    match i128::from_str_radix(digits, radix) {
        Ok(v) if neg => Ok(-v),
        Ok(v) => Ok(v),
        Err(_) => unsupported(format!("integer literal `{s}` beyond i128")),
    }
}

pub(crate) fn literal(l: &Literal) -> R<Value> {
    Ok(match l {
        Literal::Int(s) => Value::Int(parse_int(s)?),
        Literal::Float(s) => Value::Float(
            s.replace('_', "")
                .parse()
                .map_err(|_| Stop::Unsupported(format!("float `{s}`")))?,
        ),
        Literal::Str(s) => Value::Str(s.as_str().into()),
        Literal::Char(c) => Value::Char(*c),
        Literal::Bool(b) => Value::Bool(*b),
        Literal::Unit => Value::Unit,
    })
}

fn checked(v: Option<i128>, ty: &str, a: i128, b: i128) -> R<Value> {
    let (lo, hi) = int_range(ty).expect("caller checked the type");
    match v {
        Some(v) if (lo..=hi).contains(&v) => Ok(Value::Int(v)),
        _ => panic_fields(
            "overflow",
            yan_rt::abi::panic_message("overflow"),
            vec![
                ("type", Operand::Type(ty_static(ty))),
                ("left", Operand::Int(a)),
                ("right", Operand::Int(b)),
            ],
        ),
    }
}

/// The interned name of a fixed-width integer type.
fn ty_static(ty: &str) -> &'static str {
    const NAMES: &[&str] = &[
        "i8", "i16", "i32", "i64", "i128", "u8", "u16", "u32", "u64", "u128", "int",
    ];
    NAMES.iter().find(|n| **n == ty).copied().unwrap_or("int")
}

/// `<op>_<scalar>` intrinsics (assumption Y4). `None` when `name` is not an intrinsic.
pub(crate) fn intrinsic(name: &str, args: &[Value]) -> Option<R<Value>> {
    if name == "exclusive_check" {
        return Some(match args {
            [a, b] if a == b => panic("exclusivity"),
            _ => Ok(Value::Unit),
        });
    }
    let (op, ty) = name.rsplit_once('_')?;
    if let Some(r) = if ty == "bigint" {
        bits::bigint(op, args)
    } else {
        bits::int(op, ty, args)
    } {
        return Some(r);
    }
    if let [Value::BigInt(a), Value::BigInt(b)] = args {
        return native::bigint_op(op, a, b);
    }
    Some(match (args, int_range(ty)) {
        ([Value::Int(a), Value::Int(b)], Some(_)) => {
            let (a, b) = (*a, *b);
            match op {
                "add_checked" => checked(a.checked_add(b), ty, a, b),
                "sub_checked" => checked(a.checked_sub(b), ty, a, b),
                "mul_checked" => checked(a.checked_mul(b), ty, a, b),
                "floordiv" | "mod" if b == 0 => panic_fields(
                    "div_zero",
                    yan_rt::abi::panic_message("div_zero"),
                    vec![("dividend", Operand::Int(a)), ("divisor", Operand::Int(b))],
                ),
                "floordiv" => checked(
                    Some(a.div_euclid(b) - i128::from(a.rem_euclid(b) != 0 && b < 0)),
                    ty,
                    a,
                    b,
                ),
                "mod" => {
                    let r = a.rem_euclid(b);
                    checked(Some(if r != 0 && b < 0 { r + b } else { r }), ty, a, b)
                }
                "div" => Ok(Value::Float(yan_rt::num::div_i128(a, b))),
                "and" => Ok(Value::Int(a & b)),
                "or" => Ok(Value::Int(a | b)),
                "xor" => Ok(Value::Int(a ^ b)),
                "eq" => Ok(Value::Bool(a == b)),
                "ne" => Ok(Value::Bool(a != b)),
                "lt" => Ok(Value::Bool(a < b)),
                "le" => Ok(Value::Bool(a <= b)),
                "gt" => Ok(Value::Bool(a > b)),
                "ge" => Ok(Value::Bool(a >= b)),
                _ => return None,
            }
        }
        ([Value::Float(a), Value::Float(b)], None) if ty.starts_with('f') => {
            let (a, b) = (*a, *b);
            Ok(match op {
                "add" => Value::Float(a + b),
                "sub" => Value::Float(a - b),
                "mul" => Value::Float(a * b),
                "div" => Value::Float(a / b),
                "eq" => Value::Bool(a == b),
                "ne" => Value::Bool(a != b),
                "lt" => Value::Bool(a < b),
                "le" => Value::Bool(a <= b),
                "gt" => Value::Bool(a > b),
                "ge" => Value::Bool(a >= b),
                _ => return None,
            })
        }
        ([Value::Bool(a), Value::Bool(b)], None) if ty == "bool" => Ok(Value::Bool(match op {
            "and" => *a && *b,
            "or" => *a || *b,
            "eq" => a == b,
            "ne" => a != b,
            _ => return None,
        })),
        ([Value::Bool(a)], None) if ty == "bool" && op == "not" => Ok(Value::Bool(!a)),
        _ => return None,
    })
}

pub(crate) fn data(name: &str, field: &str, v: Value) -> Value {
    Value::Data {
        name: name.to_owned(),
        fields: vec![(field.to_owned(), v)],
    }
}

/// The std functions used by M0 programs.
static BUILTINS: &[(yan_std::registry::Id, bind::Bind)] = {
    use bind::{Bind, bad_args};
    use yan_std::registry::id::*;
    &[
        (
            STD_IO_PRINT,
            Bind::Cap(|it, a| match a {
                [Value::Cap(_), Value::Str(s)] => match writeln!(it.out, "{s}") {
                    Ok(()) => Ok(Value::Unit),
                    Err(e) => unsupported(format!("stdout: {e}")),
                },
                _ => bad_args(STD_IO_PRINT),
            }),
        ),
        (
            STD_IO_READ_LINE,
            Bind::Cap(|_, _| {
                let mut line = String::new();
                match std::io::stdin().lock().read_line(&mut line) {
                    Ok(0) => Ok(native::opt(None)),
                    Ok(_) => {
                        let line = line.strip_suffix('\n').unwrap_or(&line);
                        let line = line.strip_suffix('\r').unwrap_or(line);
                        Ok(native::opt(Some(Value::Str(line.into()))))
                    }
                    Err(e) => unsupported(format!("stdin: {e}")),
                }
            }),
        ),
        (
            STD_IO_READ_ALL,
            Bind::Cap(|_, _| {
                let mut s = String::new();
                match std::io::stdin().lock().read_to_string(&mut s) {
                    Ok(_) => Ok(Value::Str(s.into())),
                    Err(e) => unsupported(format!("stdin: {e}")),
                }
            }),
        ),
        (STD_LIST_NEW, Bind::Plain(|_| Ok(Value::List(Vec::new())))),
        (
            STD_LIST_LEN,
            Bind::Plain(|a| match a {
                [Value::List(xs)] => Ok(Value::Int(xs.len() as i128)),
                _ => bad_args(STD_LIST_LEN),
            }),
        ),
        (
            STD_RESULT_IS_OK,
            Bind::Plain(|a| match a {
                [Value::Data { name, .. }] => Ok(Value::Bool(name == "Ok")),
                _ => bad_args(STD_RESULT_IS_OK),
            }),
        ),
    ]
};

#[cfg(test)]
mod selftest {
    use super::*;

    fn run_src(body: &str) -> (i32, String) {
        let src = format!("module t\n{body}");
        let (m, d) = yan_core::parse("t.yir", &src);
        assert!(
            d.is_empty(),
            "{:?}",
            d.iter().map(|d| d.render_line(None)).collect::<Vec<_>>()
        );
        let mut out = Vec::new();
        let status = match run(&m.unwrap(), &mut out) {
            Ok(s) => s,
            Err(Stop::Panic(..)) => EXIT_PANIC,
            Err(Stop::Unsupported(w)) => panic!("unsupported: {w}"),
            Err(Stop::Limit(w)) => panic!("limit: {w}"),
        };
        (status, String::from_utf8(out).unwrap())
    }

    #[test]
    fn a_task_leaves_frame_drops_to_the_frame() {
        let (s, out) = run_src(
            "record Bad { pub n: i64 }\n\
             fn f() -> Str fail Bad {\n let c: Tuple(Sender[i64], Receiver[i64]) = std.chan.Channel[i64](let 0)\n\
             var tx: Sender[i64] = c._0\n var rx: Receiver[i64] = c._1\n\
             par concurrent {\n  {\n   let r: Result[i64, Cancelled] = try std.chan.receiver.recv[i64](inout rx)\n\
             drop r\n   let a: Bad = Bad { n: 1 }\n   drop rx\n   drop tx\n   fail a\n  }\n\
             {\n   let w: Result[Unit, Cancelled] = try std.chan.sender.send[i64](inout tx, sink 5)\n\
             drop w\n   let b: Bad = Bad { n: 2 }\n   drop rx\n   drop tx\n   fail b\n  }\n }\n\
             drop rx\n drop tx\n ret \"ok\"\n}\n\
             fn main(cap io: Io) -> Unit {\n let r: Result[Str, Bad] = try f()\n\
             match r {\n  Ok { value: _ } => {\n   std.io.print(cap io, let \"ok\")\n  }\n\
             Err { error: e } => {\n   let s: Str = std.fmt.show[Bad](let e)\n   std.io.print(cap io, let s)\n  }\n }\n ret ()\n}\n",
        );
        assert_eq!((s, out.as_str()), (0, "Bad(n=1)\n"));
    }

    #[test]
    fn inout_writes_back_and_closures_capture() {
        let (s, _) = run_src(
            "fn bump(inout n: i64) -> Unit {\n n = add_checked_i64(let n, let 1)\n ret ()\n}\n\
             fn main() -> Unit {\n var a: i64 = 1\n bump(inout a)\n let b: i64 = 10\n\
             let f: Fn(let i64) -> i64 = fn [let b](let x: i64) -> i64 {\n let r: i64 = add_checked_i64(let x, let b)\n ret r\n }\n\
             let r: i64 = f(let a)\n assert eq_i64(let r, let 12)\n ret ()\n}\n",
        );
        assert_eq!(s, 0);
    }

    #[test]
    fn panic_names_values_and_each_frame() {
        let src = "module t\nfn half(let n: i64, let d: i64) -> i64 {\n let h: i64 = floordiv_i64(let n, let d)\n ret h\n}\n\
                   fn main() -> Unit {\n let z: i64 = 0\n let y: i64 = half(let 7, let z)\n ret ()\n}\n";
        let (m, d) = yan_core::parse("t.yir", src);
        assert!(d.is_empty());
        let Err(Stop::Panic(p)) = run(&m.unwrap(), &mut Vec::new()) else {
            panic!("expected a panic")
        };
        assert_eq!(
            p.render("t.yir"),
            "panic(div_zero): floor division of 7 by zero\n  at half t.yir:3:2\n  at main t.yir:8:2"
        );
    }

    #[test]
    fn overflow_and_div_zero_carry_operands() {
        let panic_of = |body: &str| {
            let (m, d) = yan_core::parse("t.yir", &format!("module t\n{body}"));
            assert!(d.is_empty());
            let Err(Stop::Panic(p)) = run(&m.unwrap(), &mut Vec::new()) else {
                panic!("expected a panic")
            };
            p.fields
        };
        let own = |v: Vec<(&str, Operand)>| -> Vec<(String, Operand)> {
            v.into_iter().map(|(n, o)| (n.to_owned(), o)).collect()
        };
        let ops = |f: &str| {
            format!(
                "fn main() -> Unit {{\n let a: i8 = 100\n let b: i8 = 100\n let r: i8 = {f}(let a, let b)\n ret ()\n}}\n"
            )
        };
        assert_eq!(
            panic_of(&ops("add_checked_i8")),
            own(vec![
                ("type", Operand::Type("i8")),
                ("left", Operand::Int(100)),
                ("right", Operand::Int(100)),
            ])
        );
        let div = "fn main() -> Unit {\n let a: i64 = 7\n let b: i64 = 0\n let r: i64 = floordiv_i64(let a, let b)\n ret ()\n}\n";
        assert_eq!(
            panic_of(div),
            own(vec![
                ("dividend", Operand::Int(7)),
                ("divisor", Operand::Int(0))
            ])
        );
    }

    #[test]
    fn failed_ensures_panics_with_contract() {
        let run_src = |ens: &str| {
            let src = format!(
                "module t\nfn f() -> i64 {ens} {{\n ret 1\n}}\nfn main() -> Unit {{\n let x: i64 = f()\n ret ()\n}}\n"
            );
            let (m, d) = yan_core::parse("t.yir", &src);
            assert!(d.is_empty());
            run(&m.unwrap(), &mut Vec::new())
        };
        let Err(Stop::Panic(p)) = run_src("ensures le_i64(let out, let 0)") else {
            panic!("expected a panic")
        };
        assert_eq!(p.fields[0], ("clause".to_owned(), Operand::Int(0)));
        assert!(run_src("ensures ge_i64(let out, let 0)").is_ok());
    }

    #[test]
    fn ensures_reads_entry_value_of_inout_parameter() {
        let run_src = |ens: &str| {
            let src = format!(
                "module t\nfn f(inout n: i64) -> Unit {ens} {{\n n = add_checked_i64(let n, let 1)\n ret ()\n}}\nfn main() -> Unit {{\n var x: i64 = 5\n f(inout x)\n ret ()\n}}\n"
            );
            let (m, d) = yan_core::parse("t.yir", &src);
            assert!(d.is_empty(), "{d:?}");
            run(&m.unwrap(), &mut Vec::new())
        };
        let r = run_src("ensures gt_i64(let n, let old_n)");
        assert!(r.is_ok(), "{:?}", r.err().map(|e| format!("{e:?}")));
        assert!(run_src("ensures le_i64(let n, let old_n)").is_err());
    }

    #[test]
    fn contract_and_dict_key_panics_carry_operands() {
        let fields = |body: &str| {
            let (m, d) = yan_core::parse("t.yir", &format!("module t\n{body}"));
            assert!(d.is_empty());
            let Err(Stop::Panic(p)) = run(&m.unwrap(), &mut Vec::new()) else {
                panic!("expected a panic")
            };
            p.fields
        };
        let c = fields(
            "fn f(let n: i64, let t: Text) -> Unit requires gt_i64(let n, let 5) {\n ret ()\n}\n\
             fn main() -> Unit {\n f(let 3, let \"a\")\n ret ()\n}\n",
        );
        let names: Vec<_> = c.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["clause", "item", "n"]);
        assert_eq!(c[0].1, Operand::Int(0));
        assert_eq!(c[2].1, Operand::Int(3));
        let a = fields(
            "fn main() -> Unit {\n let n: i64 = 3\n let m: i64 = 4\n let t: Text = \"a\"\n\
             let c: Bool = gt_i64(let n, let m)\n assert c\n ret ()\n}\n",
        );
        let names: Vec<_> = a.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["left", "right", "n", "m"]);
        let k = missing_key(&Value::Int(7));
        assert_eq!(k[1], ("key_type", Operand::Type("int")));
        assert_eq!(k, missing_key(&Value::Int(7)));
        assert_ne!(k[0].1, missing_key(&Value::Int(8))[0].1);
        // Native code builds the same bytes with `yan_rt::keyhash::Enc`.
        let mut e = yan_rt::keyhash::Enc::new();
        e.seq(2);
        e.int(1);
        e.text("a");
        let t = missing_key(&Value::Tuple(vec![Value::Int(1), Value::Str("a".into())]));
        assert_eq!(t[0].1, Operand::Int(i128::from(e.hash())));
        let big = |n: &str| missing_key(&Value::BigInt(n.parse().unwrap()));
        assert_ne!(big("1")[0].1, big("2")[0].1);
        assert_eq!(big("1")[1].1, Operand::Type("bigint"));
        let l = |n| missing_key(&Value::List(vec![Value::Int(n)]));
        assert_ne!(l(1)[0].1, l(2)[0].1);
        assert_eq!(l(1)[1].1, Operand::Type("list"));
        let sum = |v| {
            missing_key(&Value::Data {
                name: v,
                fields: vec![],
            })
        };
        assert_ne!(sum("A".into())[0].1, sum("B".into())[0].1);
        let p = |n| missing_key(&Value::Period([0, 0, 0, n]));
        assert_ne!(p(1)[0].1, p(2)[0].1);
    }

    #[test]
    fn deadline_cuts_a_sleep_short_with_timeout() {
        let (s, out) = run_src(
            "record Cancelled {}\nrecord Timeout {}\n\
             fn slow(cap io: Io, cap time: Time) -> Unit fail Timeout {\n\
             let t1: MonoInstant = std.time.monotonic(cap time)\n\
             let t2: Duration = std.time.duration.of.millis(let 50)\n\
             let t3: MonoInstant = std.time.mono.add(let t1, let t2)\n\
             deadline t3 {\n\
             let d1: Duration = std.time.duration.of.millis(let 10)\n\
             std.time.sleep(cap time, let d1)?\n\
             std.io.print(cap io, let \"a\")\n\
             let d2: Duration = std.time.duration.of.millis(let 200)\n\
             std.time.sleep(cap time, let d2)?\n\
             std.io.print(cap io, let \"b\")\n\
             }\n ret ()\n}\n\
             fn main(cap io: Io, cap time: Time) -> Unit {\n\
             let r: Result[Unit, Timeout] = try slow(cap io, cap time)\n\
             std.io.print(cap io, let \"end\")\n ret ()\n}\n",
        );
        assert_eq!((s, out.as_str()), (0, "a\nend\n"));
    }

    #[test]
    fn failure_out_of_main_exits_1() {
        let (s, _) = run_src(
            "record E { c: i64 }\nfn main() -> Unit fail E {\n let e: E = E { c: 1 }\n fail e\n}\n",
        );
        assert_eq!(s, EXIT_FAILURE);
    }

    #[test]
    fn run_reporting_shows_the_failure_of_main() {
        let src = "module t\nrecord E { c: i64 }\nfn main() -> Unit fail E {\n let e: E = E { c: 1 }\n fail e\n}\n";
        let (m, _) = yan_core::parse("t.yir", src);
        let (s, shown) = run_reporting(&m.unwrap(), &mut Vec::new(), Vec::new()).expect("runs");
        assert_eq!(
            (s, shown.is_some_and(|t| t.contains("c"))),
            (EXIT_FAILURE, true)
        );
    }

    #[test]
    fn floor_semantics() {
        let i = |op, a, b| match intrinsic(op, &[Value::Int(a), Value::Int(b)]) {
            Some(Ok(Value::Int(v))) => v,
            other => panic!("{other:?}"),
        };
        assert_eq!(i("floordiv_i64", -7, 2), -4);
        assert_eq!(i("floordiv_i64", 7, -2), -4);
        assert_eq!(i("floordiv_i64", -7, -2), 3);
        assert_eq!(i("mod_i64", -7, 3), 2);
        assert_eq!(i("mod_i64", 7, -3), -2);
        assert!(matches!(
            intrinsic(
                "floordiv_i64",
                &[Value::Int(i64::MIN.into()), Value::Int(-1)]
            ),
            Some(Err(_))
        ));
        assert!(matches!(
            intrinsic("add_checked_u8", &[Value::Int(255), Value::Int(1)]),
            Some(Err(_))
        ));
    }

    #[test]
    fn loops_and_labels() {
        let (s, out) = run_src(
            "fn main(cap io: Io) -> Unit {\n var i: i64 = 0\n loop 'l {\n i = add_checked_i64(let i, let 1)\n\
             let done: bool = ge_i64(let i, let 3)\n if done { break 'l }\n std.io.print(cap io, let \"x\")\n }\n ret ()\n}\n",
        );
        assert_eq!((s, out.as_str()), (0, "x\nx\n"));
    }

    #[test]
    fn join_point_returns_past_the_jumping_statement() {
        let (s, out) = run_src(
            "fn main(cap io: Io) -> Unit {\n var n: i64 = 0\n\
             jp k(let m: i64) { n = add_checked_i64(let n, let m) }\n\
             loop 'l {\n n = add_checked_i64(let n, let 1)\n let big: bool = gt_i64(let n, let 2)\n\
             if big { jump k(10) }\n }\n\
             assert eq_i64(let n, let 13)\n std.io.print(cap io, let \"done\")\n ret ()\n}\n",
        );
        assert_eq!((s, out.as_str()), (0, "done\n"));
    }
}
