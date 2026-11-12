//! `bytes` blocks: a leaked `Vec<u8>`, as `Str` is a leaked `String` (R579 to R585).

use crate::abi::{List, leak, str_block, yan_rt_list_new, yan_rt_list_push};

/// CPython's repr of a byte string, always single-quoted (R584).
pub fn repr(b: &[u8]) -> String {
    let mut s = String::from("b'");
    for &c in b {
        match c {
            b'\\' => s.push_str("\\\\"),
            b'\'' => s.push_str("\\'"),
            b'\t' => s.push_str("\\t"),
            b'\n' => s.push_str("\\n"),
            b'\r' => s.push_str("\\r"),
            0x20..=0x7e => s.push(c as char),
            _ => s.push_str(&format!("\\x{c:02x}")),
        }
    }
    s.push('\'');
    s
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|c| format!("{c:02x}")).collect()
}

/// # Safety
/// `s` is a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_bytes_of_str(s: *const String) -> *mut Vec<u8> {
    // SAFETY: the caller's contract.
    leak(unsafe { &*s }.as_bytes().to_vec())
}

/// # Safety
/// `l` is a `List` block of one-word elements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_bytes_from_list(l: *const List) -> *mut Vec<u8> {
    // SAFETY: the caller's contract.
    let l = unsafe { &*l };
    leak(l.data.iter().map(|&w| w as u8).collect())
}

/// # Safety
/// `b` is a `bytes` block that nothing uses afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_bytes_free(b: *mut Vec<u8>) {
    crate::heap::block_freed();
    // SAFETY: the caller's contract; the block came from `leak`.
    drop(unsafe { Box::from_raw(b) });
}

/// # Safety
/// `b` is a `bytes` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_bytes_copy(b: *const Vec<u8>) -> *mut Vec<u8> {
    // SAFETY: the caller's contract.
    leak(unsafe { &*b }.clone())
}

/// # Safety
/// `b` is a `bytes` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_bytes_len(b: *const Vec<u8>) -> i64 {
    // SAFETY: the caller's contract.
    unsafe { &*b }.len() as i64
}

/// # Safety
/// `a` and `b` are `bytes` blocks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_bytes_concat(a: *const Vec<u8>, b: *const Vec<u8>) -> *mut Vec<u8> {
    // SAFETY: the caller's contract.
    let (a, b) = unsafe { (&*a, &*b) };
    leak([a.as_slice(), b.as_slice()].concat())
}

/// # Safety
/// `b` is a `bytes` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_bytes_to_list(b: *const Vec<u8>) -> *mut List {
    let l = yan_rt_list_new(1);
    // SAFETY: the caller's contract; `l` is a fresh list nothing else borrows.
    for &c in unsafe { &*b } {
        unsafe { *yan_rt_list_push(l) = c.into() };
    }
    l
}

/// # Safety
/// `a` and `p` are `bytes` blocks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_bytes_starts_with(a: *const Vec<u8>, p: *const Vec<u8>) -> u8 {
    // SAFETY: the caller's contract.
    unsafe { (&*a).starts_with(&*p) }.into()
}

/// # Safety
/// `a` and `p` are `bytes` blocks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_bytes_ends_with(a: *const Vec<u8>, p: *const Vec<u8>) -> u8 {
    // SAFETY: the caller's contract.
    unsafe { (&*a).ends_with(&*p) }.into()
}

/// # Safety
/// `b` is a `bytes` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_bytes_hex(b: *const Vec<u8>) -> *mut String {
    // SAFETY: the caller's contract.
    str_block(&hex(unsafe { &*b }))
}

/// # Safety
/// `b` is a `bytes` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_bytes_lossy(b: *const Vec<u8>) -> *mut String {
    // SAFETY: the caller's contract.
    str_block(&String::from_utf8_lossy(unsafe { &*b }))
}

/// # Safety
/// `b` is a `bytes` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_bytes_repr(b: *const Vec<u8>) -> *mut String {
    // SAFETY: the caller's contract.
    str_block(&repr(unsafe { &*b }))
}
