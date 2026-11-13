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

fn alphabet(url: bool) -> &'static [u8; 64] {
    if url {
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
    } else {
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
    }
}

pub fn base64(b: &[u8], url: bool, pad: bool) -> String {
    let a = alphabet(url);
    let mut s = String::new();
    for chunk in b.chunks(3) {
        let n = chunk.iter().fold(0u32, |n, &c| n << 8 | c as u32) << (8 * (3 - chunk.len()));
        for k in 0..=chunk.len() {
            s.push(a[(n >> (18 - 6 * k) & 63) as usize] as char);
        }
        if pad {
            s.push_str(&"=="[..3 - chunk.len()]);
        }
    }
    s
}

/// Strict: one accepted spelling per byte string and alphabet (R582).
pub fn base64_decode(s: &str, url: bool) -> Result<Vec<u8>, usize> {
    let a = alphabet(url);
    let cs: Vec<char> = s.chars().collect();
    let body = cs.len() - cs.iter().rev().take_while(|c| **c == '=').count();
    let pads = cs.len() - body;
    let mut vals = Vec::with_capacity(body);
    for (i, c) in cs[..body].iter().enumerate() {
        match a.iter().position(|x| *x as char == *c) {
            Some(v) => vals.push(v as u32),
            None => return Err(i),
        }
    }
    if body % 4 == 1 || (pads > 0 && (!cs.len().is_multiple_of(4) || pads != 4 - body % 4)) {
        return Err(body);
    }
    let mut out = Vec::with_capacity(body * 3 / 4);
    for (g, chunk) in vals.chunks(4).enumerate() {
        let n = chunk.iter().fold(0u32, |n, v| n << 6 | v) << (6 * (4 - chunk.len()));
        let keep = chunk.len() - 1;
        // Bits past the last whole byte must be zero.
        if n & ((1 << (8 * (3 - keep))) - 1) != 0 {
            return Err(g * 4 + chunk.len() - 1);
        }
        out.extend((0..keep).map(|k| (n >> (16 - 8 * k)) as u8));
    }
    Ok(out)
}

/// The bytes of a hex string, or the index of the first bad digit (R583).
pub fn hex_decode(s: &str) -> Result<Vec<u8>, usize> {
    let ds = s
        .chars()
        .enumerate()
        .map(|(i, c)| c.to_digit(16).ok_or(i))
        .collect::<Result<Vec<_>, _>>()?;
    if ds.len() % 2 == 1 {
        return Err(ds.len());
    }
    Ok(ds.chunks(2).map(|d| (d[0] * 16 + d[1]) as u8).collect())
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

/// `b[i] = x`, `i` counting from the end when negative; panics out of range.
///
/// # Safety
/// `b` is a `bytearray` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_bytes_set(b: *mut Vec<u8>, i: i64, x: i64) {
    // SAFETY: the caller's contract.
    let b = unsafe { &mut *b };
    let at = if i < 0 { i + b.len() as i64 } else { i };
    match usize::try_from(at).ok().and_then(|at| b.get_mut(at)) {
        Some(c) => *c = x as u8,
        None => out_of_range(i, b.len()),
    }
}

/// # Safety
/// `b` is a `bytearray` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_bytes_push(b: *mut Vec<u8>, x: i64) {
    // SAFETY: the caller's contract.
    unsafe { &mut *b }.push(x as u8);
}

/// # Safety
/// `b` is a `bytearray` block and `x` a `bytes` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_bytes_extend(b: *mut Vec<u8>, x: *const Vec<u8>) {
    // SAFETY: the caller's contract.
    unsafe { &mut *b }.extend_from_slice(unsafe { &*x });
}

/// The last byte, removed, or -1 when there is none.
///
/// # Safety
/// `b` is a `bytearray` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_bytes_pop(b: *mut Vec<u8>) -> i64 {
    // SAFETY: the caller's contract.
    unsafe { &mut *b }.pop().map_or(-1, i64::from)
}

/// # Safety
/// `b` is a `bytearray` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_bytes_clear(b: *mut Vec<u8>) {
    // SAFETY: the caller's contract.
    unsafe { &mut *b }.clear();
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

fn out_of_range(i: i64, n: usize) -> ! {
    let kind = "bounds";
    // SAFETY: a str is valid UTF-8.
    unsafe { crate::abi::yan_rt_panic_ops(kind.as_ptr(), kind.len(), 64, 1, i, n as i64) }
}

/// A word holding `b[i]`, counting from the end when `i` is negative.
///
/// # Safety
/// `b` is a `bytes` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_bytes_index(b: *const Vec<u8>, i: i64) -> *mut u64 {
    thread_local!(static WORD: std::cell::UnsafeCell<u64> = const { std::cell::UnsafeCell::new(0) });
    // SAFETY: the caller's contract.
    let b = unsafe { &*b };
    let at = if i < 0 { i + b.len() as i64 } else { i };
    let Some(&c) = usize::try_from(at).ok().and_then(|at| b.get(at)) else {
        out_of_range(i, b.len())
    };
    WORD.with(|w| {
        // SAFETY: the word is thread-local and only read right after this call.
        unsafe { *w.get() = c.into() };
        w.get()
    })
}

/// # Safety
/// `b` is a `bytes` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_bytes_at(b: *const Vec<u8>, i: i64) -> i64 {
    // SAFETY: the caller's contract.
    usize::try_from(i)
        .ok()
        .and_then(|i| unsafe { &*b }.get(i))
        .map_or(-1, |&c| c.into())
}

/// # Safety
/// `b` and `sub` are `bytes` blocks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_bytes_find(b: *const Vec<u8>, sub: *const Vec<u8>) -> i64 {
    // SAFETY: the caller's contract.
    let (b, sub) = unsafe { (&*b, &*sub) };
    if sub.is_empty() {
        return 0;
    }
    b.windows(sub.len())
        .position(|w| w == sub.as_slice())
        .map_or(-1, |i| i as i64)
}

/// # Safety
/// `b` is a `bytes` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_bytes_slice(b: *const Vec<u8>, i: i64, j: i64) -> *mut Vec<u8> {
    // SAFETY: the caller's contract.
    let b = unsafe { &*b };
    match (usize::try_from(i), usize::try_from(j)) {
        (Ok(x), Ok(y)) if x <= y && y <= b.len() => leak(b[x..y].to_vec()),
        _ => out_of_range(j, b.len()),
    }
}

/// -1 when `b` is UTF-8, else the length of its valid prefix.
///
/// # Safety
/// `b` is a `bytes` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_utf8_check(b: *const Vec<u8>) -> i64 {
    // SAFETY: the caller's contract.
    std::str::from_utf8(unsafe { &*b }).map_or_else(|e| e.valid_up_to() as i64, |_| -1)
}

/// -1 when `s` is hex of even length, else the index of the first fault.
///
/// # Safety
/// `s` is a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_hex_check(s: *const String) -> i64 {
    // SAFETY: the caller's contract.
    hex_decode(unsafe { &*s }).map_or_else(|at| at as i64, |_| -1)
}

/// # Safety
/// `s` is a `Str` block that `yan_rt_hex_check` accepted.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_hex_decode(s: *const String) -> *mut Vec<u8> {
    // SAFETY: the caller's contract.
    leak(hex_decode(unsafe { &*s }).unwrap_or_default())
}

/// # Safety
/// `b` is a `bytes` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_base64(b: *const Vec<u8>, url: u8, pad: u8) -> *mut String {
    // SAFETY: the caller's contract.
    str_block(&base64(unsafe { &*b }, url != 0, pad != 0))
}

/// -1 when `s` is strict base64, else the index of the first fault.
///
/// # Safety
/// `s` is a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_base64_check(s: *const String, url: u8) -> i64 {
    // SAFETY: the caller's contract.
    base64_decode(unsafe { &*s }, url != 0).map_or_else(|at| at as i64, |_| -1)
}

/// # Safety
/// `s` is a `Str` block that `yan_rt_base64_check` accepted.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_base64_decode(s: *const String, url: u8) -> *mut Vec<u8> {
    // SAFETY: the caller's contract.
    leak(base64_decode(unsafe { &*s }, url != 0).unwrap_or_default())
}

/// `invalid <kind> at <at>` for an `EncodingError` whose reason is variant `reason` (R583).
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_encoding_error_show(at: i64, reason: i64) -> *mut String {
    let kind = match reason {
        0 => "UTF-8",
        1 => "hex",
        3 => "scalar value",
        _ => "base64",
    };
    str_block(&format!("invalid {kind} at {at}"))
}
