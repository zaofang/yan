//! C-ABI entry points of `std.path` for native code. A `Path` block is a `Str` block holding the
//! normalized text, so both share allocation and release (R587 to R591).

use crate::abi::{List, panic, str_block, yan_rt_list_new, yan_rt_list_push};
use crate::path;

fn text<'a>(s: *const String) -> &'a str {
    // SAFETY: callers pass `Str` blocks that outlive the call.
    unsafe { &*s }
}

/// `win` is 1 for a `WindowsPath` block, 0 for a `Path` block.
fn w(win: i64) -> bool {
    win != 0
}

fn opt(s: Option<String>) -> *mut String {
    s.map_or(std::ptr::null_mut(), |s| str_block(&s))
}

/// `Path(s)` and `s.as_path()`: the normalized text of `s`.
///
/// # Safety
/// `s` is a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_path_new(win: i64, s: *const String) -> *mut String {
    str_block(&path::Parsed::new(w(win), text(s)).text())
}

/// `parent` (0), `name` (1), `suffix` (2), `stem` (3), `normalize` (4) of a path.
///
/// # Safety
/// `p` is a `Path` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_path_op(win: i64, op: i64, p: *const String) -> *mut String {
    str_block(&path::op(w(win), op, text(p)))
}

/// `with_name` (0) or `with_suffix` (1); panics `value` on a bad name or suffix.
///
/// # Safety
/// `p` is a `Path` block, `n` a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_path_with(
    win: i64,
    op: i64,
    p: *const String,
    n: *const String,
) -> *mut String {
    match path::with(w(win), op != 0, text(p), text(n)) {
        Some(s) => str_block(&s),
        None => panic("value"),
    }
}

/// `p.join(q)`.
///
/// # Safety
/// `p` is a `Path` block, `q` a `Path` or `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_path_join(
    win: i64,
    p: *const String,
    q: *const String,
) -> *mut String {
    str_block(&path::join(w(win), text(p), text(q)))
}

/// `is_absolute`.
///
/// # Safety
/// `p` is a `Path` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_path_is_absolute(win: i64, p: *const String) -> u8 {
    u8::from(path::is_absolute(w(win), text(p)))
}

/// `starts_with`.
///
/// # Safety
/// `p` and `b` are `Path` blocks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_path_starts_with(
    win: i64,
    p: *const String,
    b: *const String,
) -> u8 {
    u8::from(path::starts_with(w(win), text(p), text(b)))
}

/// `relative_to`: the relative path, or null when `b` is not a prefix.
///
/// # Safety
/// `p` and `b` are `Path` blocks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_path_relative_to(
    win: i64,
    p: *const String,
    b: *const String,
) -> *mut String {
    opt(path::relative_to(w(win), text(p), text(b)))
}

/// `parts`: the anchor, if any, then the segments.
///
/// # Safety
/// `p` is a `Path` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_path_parts(win: i64, p: *const String) -> *mut List {
    let l = yan_rt_list_new(1);
    for part in path::parts(w(win), text(p)) {
        // SAFETY: `l` is a fresh list of one-word elements that nothing else borrows.
        unsafe { *yan_rt_list_push(l) = str_block(&part) as u64 };
    }
    l
}

/// `drive` (0), `root` (1) or `anchor` (2) of a `WindowsPath`.
///
/// # Safety
/// `p` is a `WindowsPath` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_path_win_part(op: i64, p: *const String) -> *mut String {
    str_block(&path::win_part(op, text(p)))
}

/// `to_posix`: the same segments as a `Path` block, or null when there is a drive or a root.
///
/// # Safety
/// `p` is a `WindowsPath` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_path_to_posix(p: *const String) -> *mut String {
    opt(path::to_posix(text(p)))
}

/// `==` on `WindowsPath` blocks: equal when the case-folded parts are.
///
/// # Safety
/// `a` and `b` are `WindowsPath` blocks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_winpath_eq(a: *const String, b: *const String) -> u8 {
    u8::from(path::win_eq(text(a), text(b)))
}
