//! std.env: the host environment, the `fake_env` test handler and `Secret` (RFC 0019 D6, D7).

use std::rc::Rc;

use yong_std::registry::{Id, id::*};

use super::bind::{Bind, bad_args};
use super::native::opt;
use super::{Key, Value, civil};

/// The handler `fake_env(args, vars, zone)`; the default `env` handler under `yong test`.
pub(crate) fn fake(args: Value, vars: Value, zone: Value) -> Value {
    Value::Data {
        name: "FakeEnv".into(),
        fields: vec![
            ("args".into(), args),
            ("vars".into(), vars),
            ("zone".into(), zone),
        ],
    }
}

pub(crate) fn test_default() -> Value {
    fake(
        Value::List(Vec::new()),
        Value::Dict(Default::default()),
        civil::utc(),
    )
}

/// The fields of a `fake_env` handler, or `None` for the host.
fn fields(h: &Value) -> Option<&[(String, Value)]> {
    match h {
        Value::Data { name, fields } if name == "FakeEnv" => Some(fields),
        _ => None,
    }
}

fn str_list(xs: impl Iterator<Item = String>) -> Value {
    Value::List(xs.map(|s| Value::Str(s.into())).collect())
}

fn var(h: &Value, name: &str) -> Option<Value> {
    match fields(h) {
        Some(f) => match &f[1].1 {
            Value::Dict(d) => d.get(&Key(Value::Str(name.into()))).cloned(),
            _ => None,
        },
        None => std::env::var(name).ok().map(|s| Value::Str(s.into())),
    }
}

/// `TZ`, then the zone `/etc/localtime` links to, then UTC (R603).
fn host_zone() -> Value {
    let tz = std::env::var("TZ").ok();
    let link = std::fs::read_link("/etc/localtime").ok().and_then(|p| {
        let s = p.to_str()?.to_owned();
        Some(s.split_once("zoneinfo/")?.1.to_owned())
    });
    tz.as_deref()
        .map(|t| t.trim_start_matches(':'))
        .into_iter()
        .chain(link.as_deref())
        .find_map(civil::lookup)
        .unwrap_or_else(civil::utc)
}

pub(super) static BINDS: &[(Id, Bind)] = &[
    (
        STD_TEST_FAKE_ENV,
        Bind::Plain(|a| match a {
            [args, vars, zone] => Ok(fake(args.clone(), vars.clone(), zone.clone())),
            _ => bad_args(STD_TEST_FAKE_ENV),
        }),
    ),
    (
        STD_ENV_ARGS,
        Bind::Cap(|it, a| {
            Ok(match fields(&a[0]) {
                Some(f) => f[0].1.clone(),
                None => str_list(it.args.iter().cloned()),
            })
        }),
    ),
    (
        STD_ENV_VAR,
        Bind::Cap(|_, a| match a {
            [h, Value::Str(n)] => Ok(opt(var(h, n))),
            _ => bad_args(STD_ENV_VAR),
        }),
    ),
    (
        STD_ENV_SECRET,
        Bind::Cap(|_, a| match a {
            [h, Value::Str(n)] => Ok(opt(var(h, n).map(|v| Value::Secret(Rc::new(v))))),
            _ => bad_args(STD_ENV_SECRET),
        }),
    ),
    (
        STD_ENV_VARS,
        Bind::Cap(|_, a| {
            Ok(match fields(&a[0]) {
                Some(f) => {
                    let Value::Dict(d) = &f[1].1 else {
                        return bad_args(STD_ENV_VARS);
                    };
                    let mut d = d.clone();
                    d.sort_by(|k1, _, k2, _| match (&k1.0, &k2.0) {
                        (Value::Str(x), Value::Str(y)) => x.cmp(y),
                        _ => std::cmp::Ordering::Equal,
                    });
                    Value::Dict(d)
                }
                None => {
                    let mut vs: Vec<(String, String)> = std::env::vars_os()
                        .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
                        .collect();
                    vs.sort();
                    Value::Dict(
                        vs.into_iter()
                            .map(|(k, v)| (Key(Value::Str(k.into())), Value::Str(v.into())))
                            .collect(),
                    )
                }
            })
        }),
    ),
    (
        STD_ENV_LOCAL_ZONE,
        Bind::Cap(|_, a| {
            Ok(match fields(&a[0]) {
                Some(f) => f[2].1.clone(),
                None => host_zone(),
            })
        }),
    ),
    (
        STD_PRELUDE_SECRET,
        Bind::Plain(|a| match a {
            [v] => Ok(Value::Secret(Rc::new(std::mem::take(v)))),
            _ => bad_args(STD_PRELUDE_SECRET),
        }),
    ),
    (
        STD_ENV_EXPOSE,
        Bind::Plain(|a| match a {
            [Value::Secret(v)] => Ok((**v).clone()),
            _ => bad_args(STD_ENV_EXPOSE),
        }),
    ),
];
