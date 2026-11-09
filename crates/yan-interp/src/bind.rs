//! Std intrinsics bound by registry id (R561). Call sites resolve their path to an id at compile
//! time; each id binds to one function here. Modules not yet converted are still reached
//! through the name-matching stages in `vm::native_call`.

use std::sync::LazyLock;

use yan_std::registry::{ENTRIES, Id};

use crate::vm::Vm;
use crate::{Interp, R, Value};

/// A capability operation whose `Err` is the failure value.
pub(crate) type CapFn = fn(&mut Interp, &mut [Value]) -> R<Result<Value, Value>>;
pub(crate) type VmFn = fn(&mut Vm, &mut [Value]) -> R<Result<Value, Value>>;

#[derive(Clone, Copy)]
pub(crate) enum Bind {
    Plain(fn(&mut [Value]) -> R<Value>),
    /// A capability operation, which writes through the interpreter.
    Cap(fn(&mut Interp, &mut [Value]) -> R<Value>),
    /// A capability operation that may fail; `Err` is the failure value.
    CapFallible(CapFn),
    /// `Err` is the failure value.
    Fallible(fn(&[Value]) -> R<Result<Value, Value>>),
    /// Runs in the VM, which it may call back into for closures.
    Vm(fn(&mut Vm, Id, &mut [Value]) -> R<Value>),
    /// Runs in the VM and may fail; `Err` is the failure value.
    VmFallible(VmFn),
}

/// The functions that call closures, bound to their VM handlers.
static VM: LazyLock<Vec<(Id, Bind)>> = LazyLock::new(|| {
    let iter = crate::iter::IDS
        .iter()
        .map(|&id| (id, Bind::Vm(|vm, id, a| vm.iter_call(id, a))));
    let keyed = crate::keyed::IDS
        .iter()
        .map(|&id| (id, Bind::Vm(|vm, id, a| vm.keyed_call(id, a))));
    iter.chain(keyed).collect()
});

static TABLE: LazyLock<Vec<Option<Bind>>> = LazyLock::new(|| {
    let mut t = vec![None; ENTRIES.len()];
    for &(id, b) in crate::native::BINDS
        .iter()
        .chain(crate::BUILTINS)
        .chain(crate::math::BINDS)
        .chain(crate::bits::BINDS)
        .chain(crate::ct::BINDS)
        .chain(crate::rand::BINDS)
        .chain(crate::env::BINDS)
        .chain(crate::crypto_rand::BINDS)
        .chain(crate::fault::BINDS)
        .chain(crate::sync::BINDS)
        .chain(crate::supervise::BINDS)
        .chain(crate::log::BINDS)
        .chain(crate::proc::BINDS)
        .chain(crate::net::BINDS)
        .chain(crate::cli::BINDS)
        .chain(crate::regex::BINDS)
        .chain(crate::prelude::BINDS)
        .chain(crate::collections::BINDS)
        .chain(crate::encoding::BINDS)
        .chain(crate::csvwire::BINDS)
        .chain(crate::json::BINDS)
        .chain(crate::meta::BINDS)
        .chain(crate::jsonwire::BINDS)
        .chain(crate::path::binds())
        .chain(crate::time::BINDS)
        .chain(crate::toolchain::BINDS)
        .chain(crate::civil::BINDS)
        .chain(VM.iter())
    {
        assert!(
            t[id.0 as usize].replace(b).is_none(),
            "{} bound twice",
            ENTRIES[id.0 as usize].path
        );
    }
    t
});

pub(crate) fn get(id: Id) -> Option<Bind> {
    TABLE[id.0 as usize]
}

/// The error for arguments a bound intrinsic does not take; elaboration rules them out.
pub(crate) fn bad_args<T>(id: Id) -> R<T> {
    crate::unsupported(format!(
        "call of `{}` with these arguments",
        ENTRIES[id.0 as usize].path
    ))
}

#[cfg(test)]
pub(crate) fn bound() -> impl Iterator<Item = &'static str> {
    ENTRIES
        .iter()
        .zip(TABLE.iter())
        .filter(|(_, b)| b.is_some())
        .map(|(e, _)| e.path)
}

#[cfg(test)]
pub(crate) fn call(path: &str, args: &mut [Value]) -> R<Result<Value, Value>> {
    let id = yan_std::registry::id(path).expect("registered");
    match get(id).expect("bound") {
        Bind::Plain(f) => f(args).map(Ok),
        Bind::Fallible(f) => f(args),
        Bind::Cap(_) | Bind::CapFallible(_) | Bind::Vm(_) | Bind::VmFallible(_) => {
            unreachable!("selftests call neither")
        }
    }
}
