//! std.cli: parse arguments against a `derive(Cli)` schema (RFC 0019 D14, D15). The first problem
//! in argument order is the one reported, with fixed wording.

use std::collections::HashMap;

use serde_json::Value as J;
use yan_std::registry::{Id, id::*};

use super::bind::{Bind, bad_args};
use super::native::opt;
use super::{R, Value};

fn s<'a>(j: &'a J, k: &str) -> &'a str {
    j[k].as_str().unwrap_or("")
}

fn list<'a>(j: &'a J, k: &str) -> &'a [J] {
    j[k].as_array().map_or(&[], Vec::as_slice)
}

fn ty_name(f: &J) -> String {
    match s(f, "ty") {
        "choice" => list(f, "choices")
            .iter()
            .map(|c| c[1].as_str().unwrap_or("").to_owned())
            .collect::<Vec<_>>()
            .join("|"),
        t => t.to_owned(),
    }
}

/// `usage: prog [options] <a> [<b>...] <command>` for the record or command `r` under `prog`.
fn usage(prog: &str, r: &J) -> String {
    let mut out = format!("usage: {prog} [options]");
    for f in list(r, "fields") {
        match s(f, "kind") {
            "pos" => out += &format!(" <{}>", s(f, "flag")),
            "rest" => out += &format!(" [<{}>...]", s(f, "flag")),
            "cmd" => out += " <command>",
            _ => {}
        }
    }
    out
}

fn shown(d: &J) -> String {
    match d {
        J::String(x) => yan_rt::fmt::repr(x),
        J::Bool(true) => "True".into(),
        J::Bool(false) => "False".into(),
        J::Null => "None".into(),
        J::Number(n) if n.is_f64() => yan_rt::fmt::float_repr(n.as_f64().unwrap_or(0.0)),
        d => d.to_string(),
    }
}

fn help(prog: &str, r: &J) -> String {
    let mut out = usage(prog, r);
    if !s(r, "about").is_empty() {
        out += &format!("\n\n{}", s(r, "about"));
    }
    let fs = list(r, "fields");
    let line = |head: String, f: &J| {
        let mut tail: Vec<String> = Vec::new();
        if !s(f, "help").is_empty() {
            tail.push(s(f, "help").to_owned());
        }
        if let Some(d) = f.get("default") {
            tail.push(format!("(default: {})", shown(d)));
        }
        match tail.is_empty() {
            true => format!("\n  {head}"),
            false => format!("\n  {head}  {}", tail.join(" ")),
        }
    };
    let pos: Vec<_> = fs
        .iter()
        .filter(|f| matches!(s(f, "kind"), "pos" | "rest"))
        .collect();
    if !pos.is_empty() {
        out += "\n\narguments:";
        for f in pos {
            out += &line(format!("<{}> {}", s(f, "flag"), ty_name(f)), f);
        }
    }
    out += "\n\noptions:";
    for f in fs {
        match s(f, "kind") {
            "flag" => out += &line(format!("--{}", s(f, "flag")), f),
            "opt" => out += &line(format!("--{} <{}>", s(f, "flag"), ty_name(f)), f),
            _ => {}
        }
    }
    out += "\n  -h, --help  show this help";
    if let Some(c) = fs.iter().find(|f| s(f, "kind") == "cmd") {
        out += "\n\ncommands:";
        for v in list(c, "commands") {
            out += &format!("\n  {}", s(v, "cmd"));
            if !s(v, "about").is_empty() {
                out += &format!("  {}", s(v, "about"));
            }
        }
    }
    out
}

fn error(message: String, is_help: bool) -> Value {
    Value::Data {
        name: "CliError".into(),
        fields: vec![
            ("message".into(), Value::Str(message.into())),
            ("help".into(), Value::Bool(is_help)),
        ],
    }
}

/// `text` as a value of field `f`'s type, or the expected type's name.
fn value(f: &J, text: &str) -> Result<Value, String> {
    let bad = || ty_name(f);
    Ok(match s(f, "ty") {
        "str" => Value::Str(text.into()),
        "Path" => Value::Path(text.into()),
        "int" => Value::Int(text.parse::<i64>().map_err(|_| bad())?.into()),
        "float" => Value::Float(text.parse::<f64>().map_err(|_| bad())?),
        "choice" => {
            let v = list(f, "choices")
                .iter()
                .find(|c| c[1].as_str() == Some(text))
                .ok_or_else(|| format!("one of {}", bad().replace('|', ", ")))?;
            Value::Data {
                name: v[0].as_str().unwrap_or("").into(),
                fields: Vec::new(),
            }
        }
        _ => return Err(bad()),
    })
}

fn default(f: &J) -> Value {
    match &f["default"] {
        J::String(x) if s(f, "ty") == "Path" => Value::Path(x.as_str().into()),
        J::String(x) => Value::Str(x.as_str().into()),
        J::Bool(b) => Value::Bool(*b),
        J::Number(n) if s(f, "ty") == "float" => Value::Float(n.as_f64().unwrap_or(0.0)),
        J::Number(n) => Value::Int(n.as_i64().unwrap_or(0).into()),
        _ => Value::Unit,
    }
}

/// Parse `args` as record or command `r`, named `name`, under program text `prog`.
fn parse(prog: &str, r: &J, name: &str, args: &[String]) -> Result<Value, Value> {
    let fs = list(r, "fields");
    let fail = |msg: String| error(format!("error: {msg}\n{}", usage(prog, r)), false);
    let mut got: HashMap<&str, Value> = HashMap::new();
    let mut rest = Vec::new();
    let positional: Vec<&J> = fs.iter().filter(|f| s(f, "kind") == "pos").collect();
    let mut next_pos = 0;
    let mut options_done = false;
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        i += 1;
        if !options_done && a == "--" {
            options_done = true;
            continue;
        }
        if !options_done && (a == "-h" || a == "--help") {
            return Err(error(help(prog, r), true));
        }
        if !options_done && let Some(body) = a.strip_prefix("--") {
            let (key, inline) = match body.split_once('=') {
                Some((k, v)) => (k, Some(v.to_owned())),
                None => (body, None),
            };
            let Some(f) = fs
                .iter()
                .find(|f| matches!(s(f, "kind"), "flag" | "opt") && s(f, "flag") == key)
            else {
                return Err(fail(format!("unknown option '--{key}'")));
            };
            let field = s(f, "name");
            if got.contains_key(field) {
                return Err(fail(format!("'--{key}' given twice")));
            }
            let v = if s(f, "kind") == "flag" {
                if inline.is_some() {
                    return Err(fail(format!("option '--{key}' takes no value")));
                }
                Value::Bool(true)
            } else {
                let text = match inline {
                    Some(t) => t,
                    None if i < args.len() => {
                        i += 1;
                        args[i - 1].clone()
                    }
                    None => return Err(fail(format!("option '--{key}' needs a value"))),
                };
                let v = value(f, &text).map_err(|want| {
                    fail(format!(
                        "invalid value '{text}' for '--{key}': expected {want}"
                    ))
                })?;
                if f["optional"] == J::Bool(true) {
                    opt(Some(v))
                } else {
                    v
                }
            };
            got.insert(field, v);
            continue;
        }
        if let Some(f) = positional.get(next_pos) {
            next_pos += 1;
            let v = value(f, a).map_err(|want| {
                fail(format!(
                    "invalid value '{a}' for <{}>: expected {want}",
                    s(f, "flag")
                ))
            })?;
            got.insert(s(f, "name"), v);
            continue;
        }
        if let Some(f) = fs.iter().find(|f| s(f, "kind") == "rest") {
            let v = value(f, a).map_err(|want| {
                fail(format!(
                    "invalid value '{a}' for <{}>: expected {want}",
                    s(f, "flag")
                ))
            })?;
            rest.push(v);
            continue;
        }
        if let Some(f) = fs.iter().find(|f| s(f, "kind") == "cmd") {
            let Some(c) = list(f, "commands").iter().find(|c| s(c, "cmd") == a) else {
                return Err(fail(format!("unknown command '{a}'")));
            };
            let sub = format!("{prog} {a}");
            let v = parse(&sub, c, s(c, "name"), &args[i..])?;
            got.insert(s(f, "name"), v);
            i = args.len();
            continue;
        }
        return Err(fail(format!("unexpected argument '{a}'")));
    }
    let mut fields = Vec::new();
    for f in fs {
        let n = s(f, "name");
        let v = match (s(f, "kind"), got.remove(n)) {
            (_, Some(v)) => v,
            ("flag", None) => Value::Bool(false),
            ("rest", None) => Value::List(std::mem::take(&mut rest)),
            ("opt", None) if f["optional"] == J::Bool(true) => opt(None),
            ("opt", None) => default(f),
            ("cmd", None) => return Err(fail("missing command".into())),
            (_, None) => return Err(fail(format!("missing argument <{}>", s(f, "flag")))),
        };
        fields.push((n.to_owned(), v));
    }
    Ok(Value::Data {
        name: name.into(),
        fields,
    })
}

/// `message`.
pub(super) fn show_error(fields: &[(super::Name, Value)]) -> String {
    match &fields[0].1 {
        Value::Str(m) => m.to_string(),
        _ => String::new(),
    }
}

fn parse_args(a: &[Value]) -> R<Result<Value, Value>> {
    let [Value::List(xs), Value::Str(schema)] = a else {
        return bad_args(STD_CLI_PARSE_ARGS);
    };
    let r: J = serde_json::from_str(schema).expect("the elaborator's schema");
    let args: Vec<String> = xs
        .iter()
        .map(|x| match x {
            Value::Str(s) => s.to_string(),
            _ => String::new(),
        })
        .collect();
    Ok(parse(s(&r, "prog"), &r, s(&r, "name"), &args))
}

pub(super) static BINDS: &[(Id, Bind)] = &[(STD_CLI_PARSE_ARGS, Bind::Fallible(parse_args))];
