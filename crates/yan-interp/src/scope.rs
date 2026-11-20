//! `std.scope` (9.4): a `Scope` is a record holding the id `yan_rt::scope::Scopes` gave its
//! scope; the VM opens and closes the scopes.

use yan_std::registry::{Id, id::*};

use super::Value::{self, Bool, Data, Int, Unit};
use super::bind::{Bind, bad_args};

fn handle(id: u64) -> Value {
    Data {
        name: "Scope".into(),
        fields: vec![("id".into(), Int(i128::from(id)))],
    }
}

fn id_of(v: &Value) -> Option<u64> {
    match v {
        Data { fields, .. } => match fields.as_slice() {
            [(_, Int(n))] => u64::try_from(*n).ok(),
            _ => None,
        },
        _ => None,
    }
}

pub(crate) static BINDS: &[(Id, Bind)] = &[
    (
        STD_SCOPE_ROOT,
        Bind::Plain(|_| Ok(handle(yan_rt::scope::ROOT))),
    ),
    (STD_SCOPE_CURRENT, Bind::Cap(|it, _| Ok(handle(it.scope)))),
    (
        STD_SCOPE_DRAIN,
        Bind::Cap(|it, a| match (a.first().and_then(id_of), a.get(1)) {
            (Some(s), Some(Value::Duration(grace))) => {
                let at = super::time::mono_now(it).saturating_add(*grace);
                super::time::scopes(it);
                it.scopes.drain(s, at);
                Ok(Unit)
            }
            _ => bad_args(STD_SCOPE_DRAIN),
        }),
    ),
    (
        STD_SCOPE_IS_DRAINING,
        Bind::Cap(|it, a| match a.first().and_then(id_of) {
            Some(s) => Ok(Bool(super::time::scopes(it).draining(s))),
            None => bad_args(STD_SCOPE_IS_DRAINING),
        }),
    ),
];
