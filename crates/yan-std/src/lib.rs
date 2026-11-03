//! The standard library: the intrinsic registry (R561) and the `.yag` sources in
//! tiers (spec 22.2, R474, R562).

pub mod intrinsics;
pub mod regex;
pub mod registry;
pub mod tiers;

pub use tiers::Tier;

// `SOURCES`: `(module, source)` for each `std.<module>` written in Yan Agent, sorted by module, from
// `lib/<tier>/`; std.collections declares one submodule per type, since their methods share
// names. `TIERS[i]` is the tier of `SOURCES[i]`.
include!(concat!(env!("OUT_DIR"), "/sources.rs"));
