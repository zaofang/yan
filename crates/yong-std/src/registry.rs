//! The intrinsic registry (R561): one entry per std intrinsic and capability operation, plus the
//! scalar `<op>_<ty>` family. Ids are indices into [`ENTRIES`], which is sorted by path; they are
//! stable within one build only.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Id(pub u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Reads its arguments only.
    Pure,
    /// Writes a `var` argument.
    Mutates,
    /// A capability operation.
    Cap,
}

/// The stages an intrinsic may run in (stage 0 is compile time).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Any,
    RunOnly,
}

#[derive(Debug)]
pub struct Entry {
    pub path: &'static str,
    /// The declaration in the std sources, or the arity and result for a scalar op.
    pub sig: &'static str,
    pub min_args: usize,
    pub max_args: usize,
    pub effect: Effect,
    /// Declares `fail`: the interpreter and native code return a failure value.
    pub fails: bool,
    /// Takes a `Callable` it may call back.
    pub callback: bool,
}

impl Entry {
    pub fn stage(&self) -> Stage {
        match self.effect {
            Effect::Cap => Stage::RunOnly,
            Effect::Pure | Effect::Mutates => Stage::Any,
        }
    }
}

include!(concat!(env!("OUT_DIR"), "/registry.rs"));

pub fn id(path: &str) -> Option<Id> {
    ENTRIES
        .binary_search_by(|e| e.path.cmp(path))
        .ok()
        .map(|i| Id(i as u16))
}

pub fn entry(id: Id) -> &'static Entry {
    &ENTRIES[id.0 as usize]
}

#[cfg(test)]
mod selftest {
    use super::*;

    #[test]
    fn ids_round_trip_and_cover_the_families() {
        for (i, e) in ENTRIES.iter().enumerate() {
            assert_eq!(id(e.path), Some(Id(i as u16)), "{}", e.path);
        }
        assert_eq!(id("std.list.append"), Some(id::STD_LIST_APPEND));
        assert_eq!(entry(id::STD_LIST_APPEND).effect, Effect::Mutates);
        assert_eq!(entry(id::STD_IO_PRINT).stage(), Stage::RunOnly);
        assert!(entry(id::STD_REGEX_COMPILE).fails);
        assert!(entry(id::STD_LIST_SORT).callback);
        assert_eq!(
            (
                entry(id::STD_PRELUDE_RANGE).min_args,
                entry(id::STD_PRELUDE_RANGE).max_args
            ),
            (1, 2)
        );
        assert!(id("add_checked_i64").is_some() && id("not_bool").is_some());
        assert!(id("not_i64").is_none());
    }
}
