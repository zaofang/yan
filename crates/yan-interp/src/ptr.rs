//! `core.ptr`: pointer provenance (6.6). A pointer is an address and, optionally, the provenance
//! of the allocation it was derived from. Comparison looks at the address only.

use std::cell::RefCell;
use std::collections::BTreeMap;

use yan_std::registry::id::*;

use crate::Value::{self, Closure, Int, Ptr};
use crate::bind::{Bind, bad_args};

/// Allocation identity; `None` for a pointer that never dereferences.
pub(crate) type Prov = Option<u32>;

#[derive(Clone, Copy, Debug)]
pub(crate) struct PtrVal {
    pub addr: u64,
    pub prov: Prov,
}

impl PartialEq for PtrVal {
    fn eq(&self, other: &Self) -> bool {
        self.addr == other.addr
    }
}

thread_local! {
    /// Provenance of every exposed address (PNVI-ae-udi).
    static EXPOSED: RefCell<BTreeMap<u64, u32>> = const { RefCell::new(BTreeMap::new()) };
}

/// The alignment-free address `dangling` yields.
const DANGLING: u64 = 1;

fn addr_of(v: &Value) -> Option<u64> {
    match v {
        Int(a) => u64::try_from(*a).ok(),
        _ => None,
    }
}

pub(crate) const BINDS: &[(yan_std::registry::Id, Bind)] = &[
    (
        STD_PTR_ADDR,
        Bind::Plain(|a| match &*a {
            [Ptr(p)] => Ok(Int(p.addr.into())),
            _ => bad_args(STD_PTR_ADDR),
        }),
    ),
    (
        STD_PTR_WITH_ADDR,
        Bind::Plain(|a| match &*a {
            [Ptr(p), a] if addr_of(a).is_some() => Ok(Ptr(PtrVal {
                addr: addr_of(a).expect("checked"),
                prov: p.prov,
            })),
            _ => bad_args(STD_PTR_WITH_ADDR),
        }),
    ),
    (
        STD_PTR_MAP_ADDR,
        Bind::Vm(|vm, _, a| match &*a {
            [Ptr(p), Closure(f)] => {
                let r = vm.apply(f, vec![Int(p.addr.into())])?;
                match addr_of(&r) {
                    Some(addr) => Ok(Ptr(PtrVal { addr, prov: p.prov })),
                    None => bad_args(STD_PTR_MAP_ADDR),
                }
            }
            _ => bad_args(STD_PTR_MAP_ADDR),
        }),
    ),
    (
        STD_PTR_WITHOUT_PROVENANCE,
        Bind::Plain(|a| match &*a {
            [a] if addr_of(a).is_some() => Ok(Ptr(PtrVal {
                addr: addr_of(a).expect("checked"),
                prov: None,
            })),
            _ => bad_args(STD_PTR_WITHOUT_PROVENANCE),
        }),
    ),
    (
        STD_PTR_DANGLING,
        Bind::Plain(|a| match &*a {
            [] => Ok(Ptr(PtrVal {
                addr: DANGLING,
                prov: None,
            })),
            _ => bad_args(STD_PTR_DANGLING),
        }),
    ),
    (
        STD_PTR_EXPOSE_PROVENANCE,
        Bind::Plain(|a| match &*a {
            [Ptr(p)] => {
                if let Some(prov) = p.prov {
                    EXPOSED.with_borrow_mut(|e| e.insert(p.addr, prov));
                }
                Ok(Int(p.addr.into()))
            }
            _ => bad_args(STD_PTR_EXPOSE_PROVENANCE),
        }),
    ),
    (
        STD_PTR_WITH_EXPOSED_PROVENANCE,
        Bind::Plain(|a| match &*a {
            [a] if addr_of(a).is_some() => {
                let addr = addr_of(a).expect("checked");
                let prov = EXPOSED.with_borrow(|e| e.get(&addr).copied());
                Ok(Ptr(PtrVal { addr, prov }))
            }
            _ => bad_args(STD_PTR_WITH_EXPOSED_PROVENANCE),
        }),
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn call(id: yan_std::registry::Id, args: &[Value]) -> Value {
        let (_, Bind::Plain(f)) = BINDS.iter().find(|(i, _)| *i == id).unwrap() else {
            unreachable!()
        };
        f(&mut args.to_vec()).unwrap()
    }

    fn with_prov(addr: u64, prov: u32) -> Value {
        Ptr(PtrVal {
            addr,
            prov: Some(prov),
        })
    }

    #[test]
    fn with_addr_keeps_provenance_and_equality_is_by_address() {
        let p = with_prov(0x1000, 7);
        let Ptr(q) = call(STD_PTR_WITH_ADDR, &[p.clone(), Int(0x1008)]) else {
            panic!()
        };
        assert_eq!((q.addr, q.prov), (0x1008, Some(7)));
        assert_eq!(
            p,
            Ptr(PtrVal {
                addr: 0x1000,
                prov: None
            })
        );
    }

    #[test]
    fn exposed_provenance_round_trips_only_when_exposed() {
        let p = with_prov(0x2000, 3);
        let Ptr(cold) = call(STD_PTR_WITH_EXPOSED_PROVENANCE, &[Int(0x2000)]) else {
            panic!()
        };
        assert_eq!(cold.prov, None);
        assert_eq!(call(STD_PTR_EXPOSE_PROVENANCE, &[p]), Int(0x2000));
        let Ptr(hot) = call(STD_PTR_WITH_EXPOSED_PROVENANCE, &[Int(0x2000)]) else {
            panic!()
        };
        assert_eq!(hot.prov, Some(3));
    }

    #[test]
    fn without_provenance_and_dangling_carry_none() {
        let Ptr(a) = call(STD_PTR_WITHOUT_PROVENANCE, &[Int(64)]) else {
            panic!()
        };
        let Ptr(d) = call(STD_PTR_DANGLING, &[]) else {
            panic!()
        };
        assert_eq!((a.prov, d.prov), (None, None));
    }
}
