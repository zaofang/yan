//! C-ABI entry points of `std.regex` for native code (R561): the `Match` record is five words,
//! `whole`, `begin`, `finish`, `groups` (a two-word-element list of `str | None`: tag 0 and the
//! text, or tag 1) and `names` (a dict of group name to index).

use crate::abi::{List, str_block, yan_rt_list_new, yan_rt_list_push};
use crate::map::{Map, yan_rt_map_push};
use crate::regex::{self as rx, Anchor, Captures, Regex};

fn text<'a>(s: *const String) -> &'a str {
    // SAFETY: callers pass `Str` blocks that outlive the call.
    unsafe { &*s }
}

fn anchored(pattern: *const String, a: i64) -> Regex {
    rx::compiled(
        text(pattern),
        match a {
            0 => Anchor::None,
            1 => Anchor::Start,
            _ => Anchor::Both,
        },
    )
}

/// The rejection reason of `pattern`, or null when it compiles.
///
/// # Safety
/// `p` is a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_regex_check(p: *const String) -> *mut String {
    match rx::build(text(p)) {
        Ok(_) => std::ptr::null_mut(),
        Err(reason) => str_block(&reason),
    }
}

/// `regex.escape`.
///
/// # Safety
/// `s` is a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_regex_escape(s: *const String) -> *mut String {
    str_block(&rx::escape(text(s)))
}

/// `r.split(s)`.
///
/// # Safety
/// `p` and `s` are `Str` blocks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_regex_split(p: *const String, s: *const String) -> *mut List {
    let l = yan_rt_list_new(1);
    for part in rx::split(&anchored(p, 0), text(s)) {
        // SAFETY: `l` is a fresh list of one-word elements that nothing else borrows.
        unsafe { *yan_rt_list_push(l) = str_block(part) as u64 };
    }
    l
}

/// `r.sub(repl, s)`.
///
/// # Safety
/// `p`, `repl` and `s` are `Str` blocks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_regex_sub(
    p: *const String,
    repl: *const String,
    s: *const String,
) -> *mut String {
    str_block(&rx::sub(&anchored(p, 0), text(repl), text(s)))
}

/// The five words of `Match` for `c` at `out`.
fn fill(re: &Regex, s: &str, c: &Captures, out: *mut u64) {
    let whole = c.get(0).expect("group 0");
    let groups = yan_rt_list_new(2);
    for g in c.iter() {
        // SAFETY: `groups` is a fresh list of two-word elements that nothing else borrows.
        let slot = unsafe { std::slice::from_raw_parts_mut(yan_rt_list_push(groups), 2) };
        match g {
            Some(g) => slot[1] = str_block(g.as_str()) as u64,
            None => slot[0] = 1,
        }
    }
    let schema = "s";
    let names = Map::new(schema, 1, 1);
    for (i, n) in re.capture_names().enumerate() {
        if let Some(n) = n {
            let key = str_block(n) as u64;
            // SAFETY: `names` is a fresh dict with one key slot and one value slot, and `n` is a
            // distinct group name.
            unsafe { *yan_rt_map_push(names, &key) = i as u64 };
        }
    }
    // SAFETY: the caller passes room for five words.
    let out = unsafe { std::slice::from_raw_parts_mut(out, 5) };
    out[0] = str_block(whole.as_str()) as u64;
    out[1] = rx::chars(s, whole.start()) as u64;
    out[2] = rx::chars(s, whole.end()) as u64;
    out[3] = groups as u64;
    out[4] = names as u64;
}

/// `search` (anchor 0), `match` (1) or `fullmatch` (2): 1 and the `Match` words at `out`, or 0.
///
/// # Safety
/// `p` and `s` are `Str` blocks; `out` has five words.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_regex_find(
    p: *const String,
    s: *const String,
    anchor: i64,
    out: *mut u64,
) -> u8 {
    let re = anchored(p, anchor);
    match re.captures(text(s)) {
        Some(c) => {
            fill(&re, text(s), &c, out);
            1
        }
        None => 0,
    }
}

/// `find_all`: a list of five-word `Match` elements.
///
/// # Safety
/// `p` and `s` are `Str` blocks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_regex_find_all(p: *const String, s: *const String) -> *mut List {
    let re = anchored(p, 0);
    let l = yan_rt_list_new(5);
    for c in re.captures_iter(text(s)) {
        // SAFETY: `l` is a fresh list of five-word elements that nothing else borrows.
        fill(&re, text(s), &c, unsafe { yan_rt_list_push(l) });
    }
    l
}
