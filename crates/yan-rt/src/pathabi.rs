//! C-ABI entry points of `std.path` for native code. A `Path` block is a `Str` block holding the
//! normalized text, so both share allocation and release (R587 to R591).

use crate::abi::{List, panic, str_block, yan_rt_list_new, yan_rt_list_push};
use crate::path::Parsed;

fn text<'a>(s: *const String) -> &'a str {
    // SAFETY: callers pass `Str` blocks that outlive the call.
    unsafe { &*s }
}

fn posix(s: *const String) -> Parsed {
    Parsed::posix(text(s))
}

/// `Path(s)` and `s.as_path()`: the normalized text of `s`.
///
/// # Safety
/// `s` is a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_path_new(s: *const String) -> *mut String {
    str_block(&posix(s).text())
}

/// `parent` (0), `name` (1), `suffix` (2), `stem` (3), `normalize` (4) of a path.
///
/// # Safety
/// `p` is a `Path` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_path_op(op: i64, p: *const String) -> *mut String {
    let p = posix(p);
    let stem = |p: &Parsed| p.name()[..p.name().len() - p.suffix().len()].to_owned();
    match op {
        0 => str_block(&p.parent().text()),
        1 => str_block(p.name()),
        2 => str_block(p.suffix()),
        3 => str_block(&stem(&p)),
        _ => str_block(&p.normalize().text()),
    }
}

/// `with_name` (0) or `with_suffix` (1); panics `value` on a bad name or suffix.
///
/// # Safety
/// `p` is a `Path` block, `n` a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_path_with(
    op: i64,
    p: *const String,
    n: *const String,
) -> *mut String {
    let (p, n) = (posix(p), text(n));
    let name = if op == 0 {
        n.to_owned()
    } else {
        if !n.is_empty() && (!n.starts_with('.') || n == "." || n.contains(['/', '\\'])) {
            panic("value");
        }
        format!("{}{n}", &p.name()[..p.name().len() - p.suffix().len()])
    };
    if p.name().is_empty() || name.is_empty() || name == "." || name.contains(['/', '\\']) {
        panic("value");
    }
    let mut segs = p.segs.clone();
    *segs.last_mut().expect("named") = name;
    str_block(&p.with_segs(segs).text())
}

/// `p.join(q)`.
///
/// # Safety
/// `p` is a `Path` block, `q` a `Path` or `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_path_join(p: *const String, q: *const String) -> *mut String {
    str_block(&posix(p).join(&posix(q)).text())
}

/// `is_absolute`.
///
/// # Safety
/// `p` is a `Path` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_path_is_absolute(p: *const String) -> u8 {
    u8::from(!posix(p).root.is_empty())
}

/// `starts_with`.
///
/// # Safety
/// `p` and `b` are `Path` blocks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_path_starts_with(p: *const String, b: *const String) -> u8 {
    u8::from(posix(p).strip(&posix(b)).is_some())
}

/// `relative_to`: the relative path, or null when `b` is not a prefix.
///
/// # Safety
/// `p` and `b` are `Path` blocks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_path_relative_to(
    p: *const String,
    b: *const String,
) -> *mut String {
    match posix(p).strip(&posix(b)) {
        Some(segs) => {
            let rel = Parsed {
                drive: String::new(),
                root: "",
                segs,
                win: false,
            };
            str_block(&rel.text())
        }
        None => std::ptr::null_mut(),
    }
}

/// `parts`: the anchor, if any, then the segments.
///
/// # Safety
/// `p` is a `Path` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_path_parts(p: *const String) -> *mut List {
    let l = yan_rt_list_new(1);
    for part in posix(p).parts() {
        // SAFETY: `l` is a fresh list of one-word elements that nothing else borrows.
        unsafe { *yan_rt_list_push(l) = str_block(&part) as u64 };
    }
    l
}
