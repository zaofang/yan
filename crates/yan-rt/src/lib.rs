//! std support code (22.2, 22.4). Each intrinsic's implementation lives here once; the
//! interpreter calls it directly and native code will call it through a C-ABI symbol (R561).
//! Functions take and return plain Rust data, never interpreter values.

#[allow(unsafe_code)]
pub mod abi;
#[allow(unsafe_code)]
pub mod actor;
#[allow(unsafe_code)]
pub mod chan;
pub mod collections;
pub mod crypto_rand;
pub mod csv;
#[allow(unsafe_code)]
pub mod csvslots;
#[allow(unsafe_code)]
pub mod dq;
pub mod fmt;
#[allow(unsafe_code)]
pub mod heap;
pub mod json;
#[allow(unsafe_code)]
pub mod jsonslots;
pub mod keyhash;
pub mod list;
#[allow(unsafe_code)]
pub mod map;
pub mod math;
pub mod num;
#[allow(unsafe_code)]
pub mod pq;
pub mod rand;
pub mod regex;
#[allow(unsafe_code)]
pub mod sched;
#[allow(unsafe_code)]
pub mod select;
#[allow(unsafe_code)]
pub mod slice;
pub mod sort;
pub mod text;
pub mod wire;
#[allow(unsafe_code)]
pub mod wireslots;
