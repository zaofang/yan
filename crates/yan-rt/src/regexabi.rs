//! C-ABI entry points of `std.regex` for native code (R561): the `Match` record is five words,
//! `whole`, `begin`, `finish`, `groups` (a two-word-element list of `str | None`: tag 0 and the
//! text, or tag 1) and `names` (a dict of group name to index).

use crate::abi::{List, store_slots, str_block};
use crate::jsonslots::Slot;
use crate::regex::{self as rx, Anchor, Captures, Regex};

fn text<'a>(s: *const String) -> &'a str {
    // SAFETY: callers pass `Str` blocks that outlive the call.
    unsafe { &*s }
}

fn anchored_text(pattern: &str, a: i64) -> Regex {
    rx::compiled(
        pattern,
        match a {
            0 => Anchor::None,
            1 => Anchor::Start,
            _ => Anchor::Both,
        },
    )
}

fn anchored(pattern: *const String, a: i64) -> Regex {
    anchored_text(text(pattern), a)
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
    let mut w = [0u64];
    // SAFETY: `w` has room for the one word of a list.
    unsafe { store_slots(w.as_mut_ptr(), vec![split_slots(text(p), text(s))]) };
    w[0] as *mut List
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

/// The five slots of `Match` for `c`.
fn match_slots(re: &Regex, s: &str, c: &Captures) -> Vec<Slot> {
    let whole = c.get(0).expect("group 0");
    let groups = c
        .iter()
        .flat_map(|g| match g {
            Some(g) => [Slot::Word(0), Slot::Str(g.as_str().to_owned())],
            None => [Slot::Word(1), Slot::Word(0)],
        })
        .collect();
    let entries = re
        .capture_names()
        .enumerate()
        .filter_map(|(i, n)| Some((vec![Slot::Str(n?.to_owned())], vec![Slot::Word(i as u64)])))
        .collect();
    vec![
        Slot::Str(whole.as_str().to_owned()),
        Slot::Word(rx::chars(s, whole.start()) as u64),
        Slot::Word(rx::chars(s, whole.end()) as u64),
        Slot::List(2, groups),
        Slot::Map {
            schema: "s".into(),
            kw: 1,
            vw: 1,
            entries,
        },
    ]
}

/// `search`, `match` or `fullmatch` (`anchor` 0, 1, 2): the slots of the `Match`, if any.
pub fn find_slots(pattern: &str, s: &str, anchor: i64) -> Option<Vec<Slot>> {
    let re = anchored_text(pattern, anchor);
    re.captures(s).map(|c| match_slots(&re, s, &c))
}

/// `find_all`: a list of five-slot `Match` elements.
pub fn find_all_slots(pattern: &str, s: &str) -> Slot {
    let re = anchored_text(pattern, 0);
    Slot::List(
        5,
        re.captures_iter(s)
            .flat_map(|c| match_slots(&re, s, &c))
            .collect(),
    )
}

/// `r.split(s)`.
pub fn split_slots(pattern: &str, s: &str) -> Slot {
    let parts = rx::split(&anchored_text(pattern, 0), s);
    Slot::List(
        1,
        parts.into_iter().map(|p| Slot::Str(p.to_owned())).collect(),
    )
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
    match find_slots(text(p), text(s), anchor) {
        Some(slots) => {
            // SAFETY: the caller passes room for five words.
            unsafe { store_slots(out, slots) };
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
    let mut w = [0u64];
    // SAFETY: `w` has room for the one word of a list.
    unsafe { store_slots(w.as_mut_ptr(), vec![find_all_slots(text(p), text(s))]) };
    w[0] as *mut List
}
