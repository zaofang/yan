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

/// The component types of a user type by name: a record's fields, a sum's variant payloads.
/// `None` for a name the caller does not declare (a type parameter, a std type).
pub type Components<'a> = &'a dyn Fn(&str) -> Option<Vec<Type>>;

/// Send and Sync are structural: a type has one when every component has it (6.4).
pub fn auto_traits(parts: Components, t: &Type) -> AutoTraits {
    walk(parts, t, &mut Vec::new())
}

fn walk(parts: Components, t: &Type, open: &mut Vec<String>) -> AutoTraits {
    match t {
        Type::Named(n) => {
            let args = n.args.iter().fold(BOTH, |acc, a| match a {
                TArg::Type(t) => acc.and(walk(parts, t, open)),
                TArg::Nat(_) => acc,
            });
            match n.path.last().map(String::as_str) {
                Some("Rc" | "Weak" | "MutexGuard") => NEITHER,
                // A handle sends messages; the actor owns its state, so `T` is never shared.
                Some("Actor") => BOTH,
                Some("Cell") => AutoTraits {
                    sync: false,
                    ..args
                },
                Some("Mutex" | "Once" | "Lazy") => AutoTraits {
                    send: args.send,
                    sync: args.send,
                },
                // Every clone reaches the one value, from whichever task holds it.
                Some("Arc") => {
                    let both = args.send && args.sync;
                    AutoTraits {
                        send: both,
                        sync: both,
                    }
                }
                _ => {
                    let name = n.path.join(".");
                    // A recursive type adds nothing on the way back to itself.
                    if open.contains(&name) {
                        return args;
                    }
                    let Some(ts) = parts(&name) else {
                        return args;
                    };
                    open.push(name);
                    let all = ts.iter().fold(args, |acc, t| acc.and(walk(parts, t, open)));
                    open.pop();
                    all
                }
            }
        }
        Type::Tuple(ts) | Type::Union(ts) => {
            ts.iter().fold(BOTH, |acc, t| acc.and(walk(parts, t, open)))
        }
        Type::Refined { ty, .. } => walk(parts, ty, open),
        Type::Dyn(_) | Type::Fn(_) => BOTH,
    }
}

/// Whether a `Mutex` is reachable in `t` without going through another handle: such a value is
/// not Freeze, and sharing it between tasks is a `nondet` site (R1041).
pub fn holds_mutex(parts: Components, t: &Type) -> bool {
    fn go(parts: Components, t: &Type, open: &mut Vec<String>) -> bool {
        match t {
            Type::Named(n) => {
                let name = n.path.join(".");
                if n.path.last().is_some_and(|l| l == "Mutex") {
                    return true;
                }
                if n.args
                    .iter()
                    .any(|a| matches!(a, TArg::Type(t) if go(parts, t, open)))
                {
                    return true;
                }
                if open.contains(&name) {
                    return false;
                }
                let Some(ts) = parts(&name) else {
                    return false;
                };
                open.push(name);
                let found = ts.iter().any(|t| go(parts, t, open));
                open.pop();
                found
            }
            Type::Tuple(ts) | Type::Union(ts) => ts.iter().any(|t| go(parts, t, open)),
            Type::Refined { ty, .. } => go(parts, ty, open),
            Type::Dyn(_) | Type::Fn(_) => false,
        }
    }
    go(parts, t, &mut Vec::new())
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
        let ok = auto_traits(&|_| None, &named("Mutex", vec![named("int", vec![])]));
        assert_eq!(ok, BOTH);
        let rc = auto_traits(
            &|_| None,
            &named("Mutex", vec![named("Rc", vec![named("int", vec![])])]),
        );
        assert_eq!(rc, NEITHER);
        let cell = auto_traits(
            &|_| None,
            &named("Mutex", vec![named("Cell", vec![named("int", vec![])])]),
        );
        assert_eq!(cell, BOTH);
    }

    #[test]
    fn a_guard_is_neither() {
        assert_eq!(
            auto_traits(&|_| None, &named("MutexGuard", vec![named("int", vec![])])),
            NEITHER
        );
    }

    #[test]
    fn a_cell_is_send_not_sync() {
        let c = auto_traits(&|_| None, &named("Cell", vec![named("int", vec![])]));
        assert_eq!(
            c,
            AutoTraits {
                send: true,
                sync: false
            }
        );
    }
}

#[cfg(test)]
mod actor_selftest {
    use super::*;
    use crate::ast::Named;

    #[test]
    fn an_actor_handle_is_send_and_sync_whatever_it_holds() {
        let cell = Type::Named(Named {
            path: vec!["Cell".into()],
            args: vec![],
        });
        let a = Type::Named(Named {
            path: vec!["Actor".into()],
            args: vec![TArg::Type(cell)],
        });
        assert_eq!(auto_traits(&|_| None, &a), BOTH);
    }
}

#[cfg(test)]
mod structural_selftest {
    use super::*;
    use crate::ast::Named;

    fn named(n: &str, args: Vec<Type>) -> Type {
        Type::Named(Named {
            path: vec![n.into()],
            args: args.into_iter().map(TArg::Type).collect(),
        })
    }

    fn parts(n: &str) -> Option<Vec<Type>> {
        let int = || named("int", vec![]);
        match n {
            "W" => Some(vec![named("Rc", vec![int()])]),
            "S" => Some(vec![named("Mutex", vec![int()])]),
            "Node" => Some(vec![int(), named("Node", vec![])]),
            _ => None,
        }
    }

    #[test]
    fn a_record_has_what_its_fields_have() {
        assert_eq!(auto_traits(&parts, &named("W", vec![])), NEITHER);
        assert_eq!(auto_traits(&parts, &named("S", vec![])), BOTH);
        assert_eq!(auto_traits(&parts, &named("Node", vec![])), BOTH);
        assert_eq!(
            auto_traits(&parts, &named("List", vec![named("W", vec![])])),
            NEITHER
        );
    }

    #[test]
    fn an_arc_needs_send_and_sync_of_its_value() {
        let cell = named("Cell", vec![named("int", vec![])]);
        assert_eq!(auto_traits(&parts, &named("Arc", vec![cell])), NEITHER);
        assert_eq!(
            auto_traits(&parts, &named("Arc", vec![named("int", vec![])])),
            BOTH
        );
    }

    #[test]
    fn a_mutex_is_found_inside_a_record() {
        assert!(holds_mutex(&parts, &named("S", vec![])));
        assert!(!holds_mutex(&parts, &named("W", vec![])));
        assert!(!holds_mutex(&parts, &named("Node", vec![])));
    }
}
