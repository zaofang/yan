//! std.log: logfmt events, spans as child handlers, and the three handlers (RFC 0019 D8 to D10).

use std::io::Write as _;

use yong_std::registry::{Id, id::*};

use super::bind::{Bind, bad_args};
use super::{Interp, R, Value, unsupported};

const STDERR: i128 = 0;
const STDOUT: i128 = 1;
const DISCARD: i128 = 2;

fn rank(level: &Value) -> i128 {
    match level {
        Value::Data { name, .. } => match name.as_str() {
            "DebugLevel" => 0,
            "InfoLevel" => 1,
            "WarnLevel" => 2,
            _ => 3,
        },
        _ => 3,
    }
}

fn handler(sink: i128, min: i128, span: &str, fields: Vec<Value>) -> Value {
    Value::Data {
        name: "LogHandler".into(),
        fields: vec![
            ("sink".into(), Value::Int(sink)),
            ("min".into(), Value::Int(min)),
            ("span".into(), Value::Str(span.into())),
            ("fields".into(), Value::List(fields)),
        ],
    }
}

/// The handler a `log` capability value stands for: the default for its mode, or itself.
fn resolve(it: &Interp, h: &Value) -> (i128, i128, String, Vec<Value>) {
    match h {
        Value::Data { name, fields } if name == "LogHandler" => match &fields[..] {
            [
                (_, Value::Int(s)),
                (_, Value::Int(m)),
                (_, Value::Str(sp)),
                (_, Value::List(fs)),
            ] => (*s, *m, sp.to_string(), fs.clone()),
            _ => unreachable!("a log handler"),
        },
        // `yong test` writes every event into the test's output; `yong run` writes info and
        // above to stderr.
        _ if it.test => (STDOUT, 0, String::new(), Vec::new()),
        _ => (STDERR, 1, String::new(), Vec::new()),
    }
}

fn quoted(v: &str) -> String {
    if !v.is_empty()
        && !v
            .chars()
            .any(|c| c == ' ' || c == '=' || c == '"' || c.is_control())
    {
        return v.to_owned();
    }
    let mut out = String::from("\"");
    for c in v.chars() {
        match c {
            '"' => out += "\\\"",
            '\\' => out += "\\\\",
            '\n' => out += "\\n",
            c => out.push(c),
        }
    }
    out + "\""
}

fn pairs(fields: &[Value], line: &mut String) {
    for f in fields {
        if let Value::Tuple(kv) = f
            && let [Value::Str(k), Value::Str(v)] = &kv[..]
        {
            line.push_str(&format!(" {k}={}", quoted(v)));
        }
    }
}

fn event(it: &mut Interp, a: &[Value]) -> R<Value> {
    let [h, level, Value::Str(msg), Value::List(fields)] = a else {
        return bad_args(STD_LOG_EVENT);
    };
    let (sink, min, span, inherited) = resolve(it, h);
    let r = rank(level);
    if sink == DISCARD || r < min {
        return Ok(Value::Unit);
    }
    let name = ["debug", "info", "warn", "error"][r as usize];
    let mut line = String::new();
    if sink == STDERR {
        line = format!("ts={:.6} ", jiff::Timestamp::now());
    }
    line += &format!("level={name} msg={}", quoted(msg));
    if !span.is_empty() {
        line += &format!(" span={}", quoted(&span));
    }
    pairs(&inherited, &mut line);
    pairs(fields, &mut line);
    let done = if sink == STDOUT {
        writeln!(it.out, "{line}")
    } else {
        writeln!(std::io::stderr(), "{line}")
    };
    match done {
        Ok(()) => Ok(Value::Unit),
        Err(e) => unsupported(format!("log output: {e}")),
    }
}

fn span(it: &mut Interp, a: &[Value]) -> R<Value> {
    let [h, Value::Str(name), Value::List(fields)] = a else {
        return bad_args(STD_LOG_SPAN);
    };
    let (sink, min, parent, mut inherited) = resolve(it, h);
    let path = if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    };
    inherited.extend(fields.iter().cloned());
    Ok(handler(sink, min, &path, inherited))
}

pub(super) static BINDS: &[(Id, Bind)] = &[
    (STD_LOG_EVENT, Bind::Cap(|it, a| event(it, a))),
    (STD_LOG_SPAN, Bind::Cap(|it, a| span(it, a))),
    (
        STD_TEST_LOG_STDERR,
        Bind::Plain(|a| Ok(handler(STDERR, rank(&a[0]), "", Vec::new()))),
    ),
    (
        STD_TEST_LOG_STDOUT,
        Bind::Plain(|a| Ok(handler(STDOUT, rank(&a[0]), "", Vec::new()))),
    ),
    (
        STD_TEST_DISCARD_LOG,
        Bind::Plain(|_| Ok(handler(DISCARD, 0, "", Vec::new()))),
    ),
];
