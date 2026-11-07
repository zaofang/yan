//! std.toolchain: read-only queries answered by the host that runs `query` (21.4). A run with
//! no host, which is every run but `query`, cannot bind the capability.

use yan_std::registry::{Id, id::*};

use super::bind::{Bind, bad_args};
use super::{R, Value, panic_with, unsupported};

/// Answers toolchain queries as JSON text; `Err` is a message for the query's author.
pub trait Host {
    fn toolchain(&self, op: &str, arg: &str) -> Result<String, String>;
}

pub(super) static BINDS: &[(Id, Bind)] = &[
    (
        STD_TOOLCHAIN_CHECK,
        Bind::Cap(|it, a| ask(it, "check", a, STD_TOOLCHAIN_CHECK)),
    ),
    (
        STD_TOOLCHAIN_SIG,
        Bind::Cap(|it, a| ask(it, "sig", a, STD_TOOLCHAIN_SIG)),
    ),
    (
        STD_TOOLCHAIN_SEARCH_TYPE,
        Bind::Cap(|it, a| ask(it, "search_type", a, STD_TOOLCHAIN_SEARCH_TYPE)),
    ),
];

fn ask(it: &mut super::Interp, op: &str, a: &[Value], id: Id) -> R<Value> {
    let [_, Value::Str(arg)] = a else {
        return bad_args(id);
    };
    let Some(host) = it.host else {
        return unsupported("the toolchain capability is not bound outside `query`");
    };
    match host.toolchain(op, arg) {
        Ok(text) => Ok(Value::Str(text.into())),
        Err(msg) => panic_with("toolchain", msg),
    }
}
