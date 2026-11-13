//! std.cli: parse arguments against a `derive(Cli)` schema (RFC 0019 D14, D15); the parser is
//! `yan_rt::cli`.

use yan_rt::cli::{CliErr, V};
use yan_std::registry::{Id, id::*};

use super::bind::{Bind, bad_args};
use super::native::opt;
use super::{R, Value};

fn error(e: CliErr) -> Value {
    Value::Data {
        name: "CliError".into(),
        fields: vec![
            ("message".into(), Value::Str(e.message.into())),
            ("help".into(), Value::Bool(e.help)),
        ],
    }
}

fn value(v: V) -> Value {
    match v {
        V::Str(s) => Value::Str(s.into()),
        V::Path(s) => Value::Path(s.into()),
        V::Int(n) => Value::Int(n.into()),
        V::Float(x) => Value::Float(x),
        V::Bool(b) => Value::Bool(b),
        V::None => Value::Unit,
        V::List(xs) => Value::List(xs.into_iter().map(value).collect()),
        V::Opt(x) => opt(x.map(|x| value(*x))),
        V::Variant(name) => Value::Data {
            name,
            fields: Vec::new(),
        },
        V::Rec(name, fields) => Value::Data {
            name,
            fields: fields.into_iter().map(|(n, v)| (n, value(v))).collect(),
        },
    }
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
    let args: Vec<String> = xs
        .iter()
        .map(|x| match x {
            Value::Str(s) => s.to_string(),
            _ => String::new(),
        })
        .collect();
    Ok(yan_rt::cli::parse_args(&args, schema)
        .map(value)
        .map_err(error))
}

pub(super) static BINDS: &[(Id, Bind)] = &[(STD_CLI_PARSE_ARGS, Bind::Fallible(parse_args))];
