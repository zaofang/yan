//! std.proc: `Command`, `run` on the host or under `fake_proc` (RFC 0019 D11, D12).

use std::io::Write as _;
use std::process::Stdio;
use std::rc::Rc;

use yong_std::registry::{Id, id::*};

use super::bind::{Bind, bad_args};
use super::native::opt;
use super::{Key, R, Value};

/// The record name of a `Command`, which no program can declare.
pub(super) const COMMAND: &str = "proc.Command";
const PROGRAM: usize = 0;
const ARGS: usize = 1;
const VARS: usize = 2;
const CLEAR: usize = 3;
const CWD: usize = 4;
const INPUT: usize = 5;

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

fn host_run(cmd: &Value) -> Result<Value, Value> {
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
    let out = child.wait_with_output().map_err(fail)?;
    let _ = writer.join();
    Ok(output(
        out.status.code(),
        signal(&out.status),
        out.stdout,
        out.stderr,
    ))
}

fn run(h: &Value, cmd: &Value) -> Result<Value, Value> {
    match h {
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
        _ => host_run(cmd),
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
        STD_PROC_RUN,
        Bind::Fallible(|a| match a {
            [h, cmd] => Ok(run(h, cmd)),
            _ => bad_args(STD_PROC_RUN),
        }),
    ),
];
