//! `char` support for native code: a `char` is its code point in one word (R475).

use crate::abi::{List, str_block, yan_rt_list_new, yan_rt_list_push};

fn char_of(c: i64) -> char {
    u32::try_from(c)
        .ok()
        .and_then(char::from_u32)
        .unwrap_or('\u{fffd}')
}

/// The one-char `Str` for code point `c`.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_char_str(c: i64) -> *mut String {
    str_block(&char_of(c).to_string())
}

/// The code points of `s`, one word each.
///
/// # Safety
/// `s` is a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_str_chars(s: *const String) -> *mut List {
    let l = yan_rt_list_new(1);
    // SAFETY: the caller's contract.
    for c in unsafe { &*s }.chars() {
        // SAFETY: `l` is a fresh list of one-word elements that nothing else borrows.
        unsafe { *yan_rt_list_push(l) = u64::from(u32::from(c)) };
    }
    l
}

/// The code point at char index `i` of `s`; panics `bounds` past the end.
///
/// # Safety
/// `s` is a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_char_at(s: *const String, i: i64) -> i64 {
    // SAFETY: the caller's contract.
    match usize::try_from(i)
        .ok()
        .and_then(|i| unsafe { &*s }.chars().nth(i))
    {
        Some(c) => i64::from(u32::from(c)),
        None => crate::abi::panic("bounds"),
    }
}

/// `is_digit` (0), `is_alpha` (1), `is_alnum` (2), `is_upper` (3), `is_lower` (4), `is_space` (5).
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_char_pred(op: i64, c: i64) -> u8 {
    let c = char_of(c);
    u8::from(match op {
        0 => c.is_ascii_digit(),
        1 => c.is_alphabetic(),
        2 => c.is_alphanumeric(),
        3 => c.is_uppercase(),
        4 => c.is_lowercase(),
        _ => c.is_whitespace(),
    })
}

/// `isdigit` (0), `isalpha` (1), `isalnum` (2), `isupper` (3), `islower` (4), `isspace` (5) of a `Str`.
///
/// # Safety
/// `s` is a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_str_pred(op: i64, s: *const String) -> u8 {
    // SAFETY: the caller's contract.
    let s = unsafe { &*s };
    u8::from(match op {
        0 => !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()),
        1 => !s.is_empty() && s.chars().all(char::is_alphabetic),
        2 => !s.is_empty() && s.chars().all(char::is_alphanumeric),
        3 => s.chars().any(char::is_uppercase) && !s.chars().any(char::is_lowercase),
        4 => s.chars().any(char::is_lowercase) && !s.chars().any(char::is_uppercase),
        _ => !s.is_empty() && s.chars().all(char::is_whitespace),
    })
}

/// Non-overlapping occurrences of `p` in `s`.
///
/// # Safety
/// `s` and `p` are `Str` blocks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_str_count(s: *const String, p: *const String) -> i64 {
    // SAFETY: the caller's contract.
    let (s, p) = unsafe { (&*s, &*p) };
    s.matches(p.as_str()).count() as i64
}

/// -1 when `n` is a Unicode scalar value, else 0 (the index of `Scalar` in `EncodingError`).
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_chr_check(n: i64) -> i64 {
    if u32::try_from(n).ok().and_then(char::from_u32).is_some() {
        -1
    } else {
        0
    }
}
