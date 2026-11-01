//! std support code (22.2, 22.4). Each intrinsic's implementation lives here once; the
//! interpreter calls it directly and native code will call it through a C-ABI symbol (R561).
//! Functions take and return plain Rust data, never interpreter values.

pub mod text;
