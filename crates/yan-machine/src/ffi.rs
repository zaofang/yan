//! The host-call table: foreign functions the machine may call. A call reaches a symbol only
//! through this table, never through `dlopen`/`dlsym`, so a machine run holds exactly the
//! capabilities listed here.

use yan_rt::ffi::{FLOATS, INTS, narrow};

pub(crate) enum Host {
    Int(fn(&[i64; INTS], &[f64; FLOATS]) -> i64),
    Float(fn(&[i64; INTS], &[f64; FLOATS]) -> f64),
}

/// Scales in steps so an intermediate power of two never over- or underflows.
fn ldexp(mut x: f64, e: i32) -> f64 {
    let mut e = e.clamp(-2200, 2200);
    while e != 0 {
        let step = e.clamp(-1000, 1000);
        x *= 2f64.powi(step);
        e -= step;
    }
    x
}

pub(crate) fn lookup(link: &str, sym: &str) -> Result<Host, String> {
    let host = match (link, sym) {
        ("m", "fabs") => Host::Float(|_, f| f[0].abs()),
        ("m", "sqrt") => Host::Float(|_, f| f[0].sqrt()),
        ("m", "floor") => Host::Float(|_, f| f[0].floor()),
        ("m", "ceil") => Host::Float(|_, f| f[0].ceil()),
        ("m", "trunc") => Host::Float(|_, f| f[0].trunc()),
        ("m", "sin") => Host::Float(|_, f| f[0].sin()),
        ("m", "cos") => Host::Float(|_, f| f[0].cos()),
        ("m", "tan") => Host::Float(|_, f| f[0].tan()),
        ("m", "exp") => Host::Float(|_, f| f[0].exp()),
        ("m", "log") => Host::Float(|_, f| f[0].ln()),
        ("m", "pow") => Host::Float(|_, f| f[0].powf(f[1])),
        ("m", "fmod") => Host::Float(|_, f| f[0] % f[1]),
        ("m", "ldexp") => Host::Float(|i, f| ldexp(f[0], i[0] as i32)),
        ("m", "labs") => Host::Int(|i, _| i[0].wrapping_abs()),
        ("m", "abs") => Host::Int(|i, _| i64::from((i[0] as i32).wrapping_abs())),
        ("m", _) => return Err(format!("`m` has no host symbol `{sym}` in the machine")),
        _ => {
            return Err(format!(
                "library `{link}` is not in the machine's host table"
            ));
        }
    };
    Ok(host)
}

pub(crate) fn call(
    link: &str,
    sym: &str,
    kind: u8,
    ints: &[i64; INTS],
    floats: &[f64; FLOATS],
) -> Result<i64, String> {
    match lookup(link, sym)? {
        Host::Int(f) => Ok(narrow(f(ints, floats), kind)),
        Host::Float(_) => Err(format!("`{sym}` returns f64")),
    }
}

pub(crate) fn call_f64(
    link: &str,
    sym: &str,
    ints: &[i64; INTS],
    floats: &[f64; FLOATS],
) -> Result<f64, String> {
    match lookup(link, sym)? {
        Host::Float(f) => Ok(f(ints, floats)),
        Host::Int(_) => Err(format!("`{sym}` returns an integer")),
    }
}
