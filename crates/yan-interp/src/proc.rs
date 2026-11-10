//! std.proc: `Command`, `run` on the host or under `fake_proc` (RFC 0019 D11, D12).

use std::io::Write as _;
use std::process::Stdio;
use std::rc::Rc;

use yan_std::registry::{Id, id::*};

use super::bind::{Bind, bad_args};
use super::native::opt;
use super::time::{host_time_left, timeout};
use super::{Interp, Key, R, Value};

/// The record name of a `Command`, which no program can declare.
pub(super) const COMMAND: &str = "proc.Command";
const PROGRAM: usize = 0;
const ARGS: usize = 1;
const VARS: usize = 2;
const CLEAR: usize = 3;
const CWD: usize = 4;
const INPUT: usize = 5;
const UNCONFINED: usize = 6;

fn command(program: Value, args: Value) -> Value {
    Value::Data {
        name: COMMAND.into(),
        fields: vec![
            ("program".into(), program),
            ("args".into(), args),
            ("vars".into(), Value::List(Vec::new())),
            ("clear_env".into(), Value::Bool(false)),
            ("cwd".into(), Value::Unit),
            ("input".into(), Value::Unit),
            ("unconfined".into(), Value::Bool(false)),
        ],
    }
}

/// `cmd` with field `k` changed by `f`.
fn with(cmd: &Value, k: usize, f: impl FnOnce(&mut Value)) -> R<Value> {
    let mut c = cmd.clone();
    match &mut c {
        Value::Data { fields, .. } => f(&mut fields[k].1),
        _ => return bad_args(STD_PROC_ARG),
    }
    Ok(c)
}

fn field(cmd: &Value, k: usize) -> &Value {
    match cmd {
        Value::Data { fields, .. } => &fields[k].1,
        _ => unreachable!("a Command"),
    }
}

fn text(v: &Value) -> &str {
    match v {
        Value::Str(s) | Value::Path(s) => s,
        _ => unreachable!("a str"),
    }
}

fn error(program: &str, reason: &str, message: String) -> Value {
    Value::Data {
        name: "ProcError".into(),
        fields: vec![
            ("program".into(), Value::Str(program.into())),
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

/// `<program>: <message>`.
pub(super) fn show_error(fields: &[(super::Name, Value)]) -> String {
    format!("{}: {}", text(&fields[0].1), text(&fields[2].1))
}

/// `Command(['echo', 'hi'])`: the argv.
pub(super) fn show_command(fields: &[(super::Name, Value)]) -> String {
    let mut argv = vec![fields[PROGRAM].1.clone()];
    if let Value::List(xs) = &fields[ARGS].1 {
        argv.extend(xs.iter().cloned());
    }
    format!(
        "Command({})",
        super::prelude::show(&Value::List(argv), false)
    )
}

fn output(code: Option<i32>, signal: Option<i32>, out: Vec<u8>, err: Vec<u8>) -> Value {
    Value::Data {
        name: "Output".into(),
        fields: vec![
            ("code".into(), opt(code.map(|c| Value::Int(c.into())))),
            ("signal".into(), opt(signal.map(|s| Value::Int(s.into())))),
            ("stdout".into(), Value::Bytes(Rc::from(out))),
            ("stderr".into(), Value::Bytes(Rc::from(err))),
        ],
    }
}

#[cfg(unix)]
fn signal(s: &std::process::ExitStatus) -> Option<i32> {
    std::os::unix::process::ExitStatusExt::signal(s)
}

#[cfg(not(unix))]
fn signal(_: &std::process::ExitStatus) -> Option<i32> {
    None
}

fn host_run(it: &Interp, cmd: &Value, left: Option<std::time::Duration>) -> Result<Value, Value> {
    let program = text(field(cmd, PROGRAM));
    let mut c = std::process::Command::new(program);
    if let Value::List(xs) = field(cmd, ARGS) {
        c.args(xs.iter().map(text));
    }
    if field(cmd, CLEAR) == &Value::Bool(true) {
        c.env_clear();
    }
    if let Value::List(vs) = field(cmd, VARS) {
        for v in vs {
            if let Value::Tuple(kv) = v {
                c.env(text(&kv[0]), text(&kv[1]));
            }
        }
    }
    if let Value::Str(d) | Value::Path(d) = field(cmd, CWD) {
        c.current_dir(&**d);
    }
    let input = match field(cmd, INPUT) {
        Value::Bytes(b) => b.to_vec(),
        _ => Vec::new(),
    };
    if field(cmd, UNCONFINED) != &Value::Bool(true)
        && let Err(u) = yan_sandbox::confine(&mut c, &it.ceiling)
    {
        return Err(error(program, "Unconfined", u.0.into()));
    }
    c.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let fail = |e: std::io::Error| {
        let reason = match e.kind() {
            std::io::ErrorKind::NotFound => "NotFound",
            std::io::ErrorKind::PermissionDenied => "PermissionDenied",
            _ => "OsError",
        };
        error(program, reason, e.to_string())
    };
    let mut child = c.spawn().map_err(fail)?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    // A writer thread, so a child that fills its output pipe before reading all its input
    // cannot deadlock against us.
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let out = match left {
        None => child.wait_with_output().map_err(fail)?,
        Some(left) => {
            let read = |r: Option<Box<dyn std::io::Read + Send>>| {
                std::thread::spawn(move || {
                    let mut buf = Vec::new();
                    if let Some(mut r) = r {
                        let _ = r.read_to_end(&mut buf);
                    }
                    buf
                })
            };
            let stdout = read(child.stdout.take().map(|r| Box::new(r) as _));
            let stderr = read(child.stderr.take().map(|r| Box::new(r) as _));
            let end = std::time::Instant::now() + left;
            let status = loop {
                if let Some(s) = child.try_wait().map_err(fail)? {
                    break s;
                }
                if std::time::Instant::now() >= end {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(timeout());
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            };
            std::process::Output {
                status,
                stdout: stdout.join().unwrap_or_default(),
                stderr: stderr.join().unwrap_or_default(),
            }
        }
    };
    let _ = writer.join();
    Ok(output(
        out.status.code(),
        signal(&out.status),
        out.stdout,
        out.stderr,
    ))
}

fn run(
    it: &Interp,
    h: &Value,
    cmd: &Value,
    left: Option<std::time::Duration>,
) -> Result<Value, Value> {
    match h {
        Value::Data { name, fields } if name == "OnlyProc" => {
            let program = text(field(cmd, PROGRAM));
            match &fields[1].1 {
                Value::List(ps) if !ps.iter().any(|p| text(p) == program) => Err(error(
                    program,
                    "PermissionDenied",
                    "not granted by proc.only".into(),
                )),
                _ => run(it, &fields[0].1, cmd, left),
            }
        }
        Value::Data { name, fields } if name == "FakeProc" => {
            let program = text(field(cmd, PROGRAM));
            match &fields[0].1 {
                Value::Dict(d) => d
                    .get(&Key(Value::Str(program.into())))
                    .cloned()
                    .ok_or_else(|| error(program, "NotFound", "not in fake_proc".into())),
                _ => unreachable!("a fake_proc map"),
            }
        }
        _ => host_run(it, cmd, left),
    }
}

pub(crate) fn test_default() -> Value {
    fake(Value::Dict(Default::default()))
}

fn fake(outputs: Value) -> Value {
    Value::Data {
        name: "FakeProc".into(),
        fields: vec![("outputs".into(), outputs)],
    }
}

pub(super) static BINDS: &[(Id, Bind)] = &[
    (
        STD_PRELUDE_COMMAND,
        Bind::Plain(|a| match a {
            [p, xs] => Ok(command(p.clone(), xs.clone())),
            _ => bad_args(STD_PRELUDE_COMMAND),
        }),
    ),
    (
        STD_PROC_PROGRAM,
        Bind::Plain(|a| Ok(field(&a[0], PROGRAM).clone())),
    ),
    (
        STD_PROC_ARGV,
        Bind::Plain(|a| {
            let mut argv = vec![field(&a[0], PROGRAM).clone()];
            if let Value::List(xs) = field(&a[0], ARGS) {
                argv.extend(xs.iter().cloned());
            }
            Ok(Value::List(argv))
        }),
    ),
    (
        STD_PROC_ARG,
        Bind::Plain(|a| {
            let s = a[1].clone();
            with(&a[0], ARGS, |xs| {
                if let Value::List(xs) = xs {
                    xs.push(s)
                }
            })
        }),
    ),
    (
        STD_PROC_WITH_ARGS,
        Bind::Plain(|a| {
            let more = a[1].clone();
            with(&a[0], ARGS, |xs| {
                if let (Value::List(xs), Value::List(more)) = (xs, more) {
                    xs.extend(more)
                }
            })
        }),
    ),
    (
        STD_PROC_ENV,
        Bind::Plain(|a| {
            let kv = Value::Tuple(vec![a[1].clone(), a[2].clone()]);
            with(&a[0], VARS, |vs| {
                if let Value::List(vs) = vs {
                    vs.push(kv)
                }
            })
        }),
    ),
    (
        STD_PROC_ENV_CLEAR,
        Bind::Plain(|a| {
            let c = with(&a[0], CLEAR, |b| *b = Value::Bool(true))?;
            with(&c, VARS, |vs| *vs = Value::List(Vec::new()))
        }),
    ),
    (
        STD_PROC_IN_DIR,
        Bind::Plain(|a| {
            let d = Value::Str(text(&a[1]).into());
            with(&a[0], CWD, |c| *c = d)
        }),
    ),
    (
        STD_PROC_STDIN,
        Bind::Plain(|a| {
            let b = a[1].clone();
            with(&a[0], INPUT, |i| *i = b)
        }),
    ),
    (
        STD_PROC_UNCONFINED,
        Bind::Plain(|a| with(&a[0], UNCONFINED, |u| *u = Value::Bool(true))),
    ),
    (
        STD_PROC_SUCCESS,
        Bind::Plain(|a| match &a[0] {
            Value::Data { fields, .. } => Ok(Value::Bool(match &fields[0].1 {
                Value::Data { name, fields } if name == "Some" => fields[0].1 == Value::Int(0),
                _ => false,
            })),
            _ => bad_args(STD_PROC_SUCCESS),
        }),
    ),
    (STD_TEST_FAKE_PROC, Bind::Plain(|a| Ok(fake(a[0].clone())))),
    (
        STD_PROC_ONLY,
        Bind::Plain(|a| match a {
            [h, programs] => Ok(Value::Data {
                name: "OnlyProc".into(),
                fields: vec![
                    ("inner".into(), h.clone()),
                    ("programs".into(), programs.clone()),
                ],
            }),
            _ => bad_args(STD_PROC_ONLY),
        }),
    ),
    (
        STD_PROC_RUN,
        Bind::CapFallible(|it, a| match a {
            [h, cmd] => Ok(run(it, h, cmd, host_time_left(it))),
            _ => bad_args(STD_PROC_RUN),
        }),
    ),
];

/// The ceiling a child inherits: the union of what the entry `fs` and `net` handlers grant.
pub(super) fn ceiling(entry: &[Value]) -> yan_sandbox::Ceiling {
    let mut c = yan_sandbox::Ceiling::default();
    for h in entry {
        let (read, write) = fs_grants(h);
        c.read.extend(read);
        c.write.extend(write);
        c.net |= net_granted(h);
    }
    c
}

fn net_granted(h: &Value) -> bool {
    match h {
        Value::Cap(n) => n == "net",
        Value::Data { name, fields } if name == "OnlyNet" => net_granted(&fields[0].1),
        _ => false,
    }
}

type Dirs = Vec<std::path::PathBuf>;

/// The directories handler `h` lets a program read and write; non-`fs` values grant none.
fn fs_grants(h: &Value) -> (Dirs, Dirs) {
    let root = || vec![std::path::PathBuf::from("/")];
    match h {
        Value::Cap(n) if n == "fs" => (root(), root()),
        Value::Data { name, fields } if name == "SubFs" => {
            let (r, w) = fs_grants(&fields[0].1);
            let dir = std::path::PathBuf::from(text(&fields[1].1));
            (
                meet(r, std::slice::from_ref(&dir)),
                meet(w, std::slice::from_ref(&dir)),
            )
        }
        Value::Data { name, fields } if name == "OnlyFs" => {
            let (r, w) = fs_grants(&fields[0].1);
            let paths = |v: &Value| match v {
                Value::List(xs) => xs.iter().map(|x| text(x).into()).collect::<Dirs>(),
                _ => Vec::new(),
            };
            (meet(r, &paths(&fields[1].1)), meet(w, &paths(&fields[2].1)))
        }
        _ => (Vec::new(), Vec::new()),
    }
}

/// The paths of `a` and `b` that lie within one another, each reduced to the deeper of the pair.
fn meet(a: Dirs, b: &[std::path::PathBuf]) -> Dirs {
    a.iter()
        .flat_map(|x| {
            b.iter().filter_map(move |y| match () {
                _ if y.starts_with(x) => Some(y.clone()),
                _ if x.starts_with(y) => Some(x.clone()),
                _ => None,
            })
        })
        .collect()
}
