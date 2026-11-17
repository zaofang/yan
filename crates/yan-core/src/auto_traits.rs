//! Send and Sync, computed once over a checked type (6.4, R1040). Every E415 and E417 site asks here.

use crate::ast::{TArg, Type};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutoTraits {
    pub send: bool,
    pub sync: bool,
}

const BOTH: AutoTraits = AutoTraits {
    send: true,
    sync: true,
};
const NEITHER: AutoTraits = AutoTraits {
    send: false,
    sync: false,
};

impl AutoTraits {
    fn and(self, o: Self) -> Self {
        Self {
            send: self.send && o.send,
            sync: self.sync && o.sync,
        }
    }
}

pub fn auto_traits(t: &Type) -> AutoTraits {
    match t {
        Type::Named(n) => {
            let args = n.args.iter().fold(BOTH, |acc, a| match a {
                TArg::Type(t) => acc.and(auto_traits(t)),
                TArg::Nat(_) => acc,
            });
            match n.path.last().map(String::as_str) {
                Some("Rc" | "Weak" | "MutexGuard") => NEITHER,
                Some("Cell") => AutoTraits {
                    sync: false,
                    ..args
                },
                Some("Mutex") => AutoTraits {
                    send: args.send,
                    sync: args.send,
                },
                _ => args,
            }
        }
        Type::Tuple(ts) | Type::Union(ts) => ts.iter().fold(BOTH, |acc, t| acc.and(auto_traits(t))),
        Type::Refined { ty, .. } => auto_traits(ty),
        Type::Dyn(_) | Type::Fn(_) => BOTH,
    }
}

#[cfg(test)]
mod selftest {
    use super::*;
    use crate::ast::Named;

    fn named(n: &str, args: Vec<Type>) -> Type {
        Type::Named(Named {
            path: vec![n.into()],
            args: args.into_iter().map(TArg::Type).collect(),
        })
    }

    #[test]
    fn mutex_is_send_and_sync_iff_its_value_is_send() {
        let ok = auto_traits(&named("Mutex", vec![named("int", vec![])]));
        assert_eq!(ok, BOTH);
        let rc = auto_traits(&named(
            "Mutex",
            vec![named("Rc", vec![named("int", vec![])])],
        ));
        assert_eq!(rc, NEITHER);
        let cell = auto_traits(&named(
            "Mutex",
            vec![named("Cell", vec![named("int", vec![])])],
        ));
        assert_eq!(cell, BOTH);
    }

    #[test]
    fn a_guard_is_neither() {
        assert_eq!(
            auto_traits(&named("MutexGuard", vec![named("int", vec![])])),
            NEITHER
        );
    }

    #[test]
    fn a_cell_is_send_not_sync() {
        let c = auto_traits(&named("Cell", vec![named("int", vec![])]));
        assert_eq!(
            c,
            AutoTraits {
                send: true,
                sync: false
            }
        );
    }
}
