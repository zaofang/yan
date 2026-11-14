//! Calls of foreign-library functions (12.1, 12.2) with integer and `f64` arguments: the
//! interpreter calls `call` and native code calls `yan_rt_ffi_call` and `yan_rt_ffi_call_f64`,
//! so both backends reach the symbol the same way.
//!
//! The System V x86-64 and AAPCS64 conventions assign integer and floating arguments to separate
//! register files in declaration order, so a function of up to `INTS` integers and `FLOATS`
//! floats is called through one signature that fills every register.

#[cfg(unix)]
use std::collections::HashMap;
#[cfg(unix)]
use std::ffi::{CString, c_void};
#[cfg(unix)]
use std::sync::Mutex;

pub const INTS: usize = 6;
pub const FLOATS: usize = 8;

/// The result type code: low 7 bits the width, bit 7 set for signed; 0 for no result.
pub fn int_kind(bits: u8, signed: bool) -> u8 {
    bits | (u8::from(signed) << 7)
}

type IntFn =
    extern "C" fn(i64, i64, i64, i64, i64, i64, f64, f64, f64, f64, f64, f64, f64, f64) -> i64;
type FloatFn =
    extern "C" fn(i64, i64, i64, i64, i64, i64, f64, f64, f64, f64, f64, f64, f64, f64) -> f64;

#[cfg(unix)]
static SYMBOLS: Mutex<Option<HashMap<(String, String), usize>>> = Mutex::new(None);

#[cfg(unix)]
fn open(link: &str) -> *mut c_void {
    // Dev symlinks (`libm.so`) are absent on many hosts; the versioned names are tried next.
    for name in [
        format!("lib{link}.so"),
        format!("lib{link}.so.6"),
        format!("lib{link}.so.1"),
        format!("lib{link}.dylib"),
    ] {
        let c = CString::new(name).expect("no NUL in a library name");
        // SAFETY: `c` is a valid NUL-terminated string.
        let h = unsafe { libc::dlopen(c.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if !h.is_null() {
            return h;
        }
    }
    std::ptr::null_mut()
}

#[cfg(not(unix))]
fn symbol(link: &str, _sym: &str) -> Result<usize, String> {
    Err(format!("cannot load library `{link}` on this target"))
}

#[cfg(unix)]
fn symbol(link: &str, sym: &str) -> Result<usize, String> {
    let mut g = SYMBOLS.lock().unwrap_or_else(|e| e.into_inner());
    let cache = g.get_or_insert_with(HashMap::new);
    let key = (link.to_owned(), sym.to_owned());
    if let Some(&a) = cache.get(&key) {
        return Ok(a);
    }
    let h = open(link);
    if h.is_null() {
        return Err(format!("cannot load library `{link}`"));
    }
    let c = CString::new(sym).map_err(|_| format!("bad symbol `{sym}`"))?;
    // SAFETY: `h` is a live handle and `c` is NUL-terminated.
    let p = unsafe { libc::dlsym(h, c.as_ptr()) };
    if p.is_null() {
        return Err(format!("`{link}` has no symbol `{sym}`"));
    }
    cache.insert(key, p as usize);
    Ok(p as usize)
}

/// Narrow the raw register `v` to the result type `kind`.
fn narrow(v: i64, kind: u8) -> i64 {
    let bits = u32::from(kind & 0x7f);
    if bits == 0 || bits >= 64 {
        return v;
    }
    let shift = 64 - bits;
    if kind & 0x80 != 0 {
        (v << shift) >> shift
    } else {
        ((v as u64) << shift >> shift) as i64
    }
}

/// Calls `sym` of the library linked as `link`; the integer result is narrowed to `kind`.
#[allow(unsafe_code)]
pub fn call(
    link: &str,
    sym: &str,
    kind: u8,
    ints: [i64; INTS],
    floats: [f64; FLOATS],
) -> Result<i64, String> {
    let a = symbol(link, sym)?;
    // SAFETY: the declaration in the extern block asserts the symbol's signature (12.1); extra
    // register arguments are ignored by a callee that does not read them.
    let f = unsafe { std::mem::transmute::<usize, IntFn>(a) };
    let r = f(
        ints[0], ints[1], ints[2], ints[3], ints[4], ints[5], floats[0], floats[1], floats[2],
        floats[3], floats[4], floats[5], floats[6], floats[7],
    );
    Ok(narrow(r, kind))
}

/// As `call` for a function returning `f64`.
#[allow(unsafe_code)]
pub fn call_f64(
    link: &str,
    sym: &str,
    ints: [i64; INTS],
    floats: [f64; FLOATS],
) -> Result<f64, String> {
    let a = symbol(link, sym)?;
    // SAFETY: as in `call`.
    let f = unsafe { std::mem::transmute::<usize, FloatFn>(a) };
    Ok(f(
        ints[0], ints[1], ints[2], ints[3], ints[4], ints[5], floats[0], floats[1], floats[2],
        floats[3], floats[4], floats[5], floats[6], floats[7],
    ))
}

fn fatal(m: &str) -> ! {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    eprintln!("yan: {m}");
    std::process::exit(1)
}

macro_rules! entry {
    ($name:ident, $ret:ty, |$link:ident, $sym:ident, $kind:ident, $ints:ident, $floats:ident| $call:expr) => {
        /// # Safety
        /// `link` and `sym` point to live `String`s.
        #[allow(clippy::too_many_arguments)]
        pub unsafe extern "C" fn $name(
            link: *const String,
            sym: *const String,
            kind: i64,
            i0: i64,
            i1: i64,
            i2: i64,
            i3: i64,
            i4: i64,
            i5: i64,
            f0: f64,
            f1: f64,
            f2: f64,
            f3: f64,
            f4: f64,
            f5: f64,
            f6: f64,
            f7: f64,
        ) -> $ret {
            // SAFETY: the caller passes live strings.
            let ($link, $sym) = unsafe { (&*link, &*sym) };
            let $kind = kind as u8;
            let $ints = [i0, i1, i2, i3, i4, i5];
            let $floats = [f0, f1, f2, f3, f4, f5, f6, f7];
            match $call {
                Ok(v) => v,
                Err(m) => fatal(&m),
            }
        }
    };
}

entry!(yan_rt_ffi_call, i64, |link, sym, kind, ints, floats| call(
    link, sym, kind, ints, floats
));
entry!(
    yan_rt_ffi_call_f64,
    f64,
    |link, sym, _kind, ints, floats| call_f64(link, sym, ints, floats)
);
