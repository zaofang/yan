//! The C-ABI entry points native code calls (R561).

use std::io::Write;

/// # Safety
/// `ptr` points to `len` bytes of UTF-8.
unsafe fn text<'a>(ptr: *const u8, len: usize) -> &'a str {
    // SAFETY: the caller's contract.
    unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(ptr, len)) }
}

/// `std.io.print`: the text and a newline on stdout.
///
/// # Safety
/// `ptr` points to `len` bytes of UTF-8.
pub unsafe extern "C" fn yong_rt_print(ptr: *const u8, len: usize) {
    // SAFETY: forwarded.
    let s = unsafe { text(ptr, len) };
    let mut out = std::io::stdout().lock();
    if writeln!(out, "{s}").and_then(|()| out.flush()).is_err() {
        std::process::exit(1);
    }
}

/// A panic of kind `ptr[..len]` (8.3): reported on stderr, exit status 101.
///
/// # Safety
/// `ptr` points to `len` bytes of UTF-8.
pub unsafe extern "C" fn yong_rt_panic(ptr: *const u8, len: usize) -> ! {
    // SAFETY: forwarded.
    let kind = unsafe { text(ptr, len) };
    let _ = std::io::stdout().flush();
    eprintln!("panic({kind}): {}", panic_message(kind));
    std::process::exit(101)
}

/// The message a panic of `kind` prints when its site gives no specific one.
pub fn panic_message(kind: &str) -> &'static str {
    match kind {
        "bounds" => "index out of range",
        "overflow" => "integer overflow",
        "div_zero" => "division by zero",
        "assert" => "assertion failed",
        "contract" => "precondition failed",
        "match" => "no arm matched the value",
        "pattern" => "the value did not match the pattern",
        "unreachable" => "reached code marked unreachable",
        "exclusivity" => "two mutable accesses to the same place",
        "conversion" => "the value does not fit the target type",
        _ => "invalid value",
    }
}

// `Str` and `List` values in native code are pointers to these leaked blocks; drops are not
// elaborated yet (5.3).

/// A list of `words`-slot elements.
pub struct List {
    words: usize,
    data: Vec<u64>,
}

fn leak<T>(x: T) -> *mut T {
    Box::into_raw(Box::new(x))
}

fn panic(kind: &str) -> ! {
    // SAFETY: a str is valid UTF-8.
    unsafe { yong_rt_panic(kind.as_ptr(), kind.len()) }
}

/// A `Str` block for `s`, owned by the program from now on.
pub fn str_block(s: &str) -> *mut String {
    leak(s.to_owned())
}

/// # Safety
/// `s` is a `Str` block.
pub unsafe extern "C" fn yong_rt_print_str(s: *const String) {
    // SAFETY: the caller's contract.
    let s = unsafe { &*s };
    // SAFETY: a String is UTF-8.
    unsafe { yong_rt_print(s.as_ptr(), s.len()) }
}

/// # Safety
/// `a` and `b` are `Str` blocks.
pub unsafe extern "C" fn yong_rt_concat(a: *const String, b: *const String) -> *mut String {
    // SAFETY: the caller's contract.
    let (a, b) = unsafe { (&*a, &*b) };
    leak(format!("{a}{b}"))
}

pub extern "C" fn yong_rt_show_i64(x: i64) -> *mut String {
    leak(x.to_string())
}

pub extern "C" fn yong_rt_show_u64(x: u64) -> *mut String {
    leak(x.to_string())
}

pub extern "C" fn yong_rt_show_f64(x: f64) -> *mut String {
    leak(crate::fmt::float_repr(x))
}

pub extern "C" fn yong_rt_show_bool(x: u8) -> *mut String {
    leak(if x != 0 { "True" } else { "False" }.to_owned())
}

/// # Safety
/// `s` is a `Str` block.
pub unsafe extern "C" fn yong_rt_repr(s: *const String) -> *mut String {
    // SAFETY: the caller's contract.
    leak(crate::fmt::repr(unsafe { &*s }))
}

/// # Safety
/// `a` and `b` are `Str` blocks.
pub unsafe extern "C" fn yong_rt_str_eq(a: *const String, b: *const String) -> u8 {
    // SAFETY: the caller's contract.
    u8::from(unsafe { *a == *b })
}

pub extern "C" fn yong_rt_list_new(words: u64) -> *mut List {
    leak(List {
        words: words as usize,
        data: Vec::new(),
    })
}

/// The slots of a new last element, valid until the list next grows.
///
/// # Safety
/// `l` is a `List` block that nothing else borrows.
pub unsafe extern "C" fn yong_rt_list_push(l: *mut List) -> *mut u64 {
    // SAFETY: the caller's contract.
    let l = unsafe { &mut *l };
    let at = l.data.len();
    l.data.resize(at + l.words, 0);
    l.data[at..].as_mut_ptr()
}

/// # Safety
/// `l` is a `List` block.
pub unsafe extern "C" fn yong_rt_list_len(l: *const List) -> i64 {
    // SAFETY: the caller's contract.
    let l = unsafe { &*l };
    l.data.len().checked_div(l.words).unwrap_or(0) as i64
}

/// The slots of element `i`; a `bounds` panic when there is none.
///
/// # Safety
/// `l` is a `List` block.
pub unsafe extern "C" fn yong_rt_list_at(l: *mut List, i: i64) -> *mut u64 {
    // SAFETY: the caller's contract.
    let n = unsafe { yong_rt_list_len(l) };
    if !(0..n).contains(&i) {
        panic("bounds");
    }
    // SAFETY: as above; `i` is in bounds.
    let l = unsafe { &mut *l };
    l.data[i as usize * l.words..].as_mut_ptr()
}

/// # Safety
/// `l` is a `List` block.
pub unsafe extern "C" fn yong_rt_list_copy(l: *const List) -> *mut List {
    // SAFETY: the caller's contract.
    let l = unsafe { &*l };
    leak(List {
        words: l.words,
        data: l.data.clone(),
    })
}

/// End the program with `code` once stdout is flushed: `main` failed (8.1).
pub extern "C" fn yong_rt_exit(code: i64) -> ! {
    let _ = std::io::stdout().flush();
    std::process::exit(code as i32)
}

/// `words` zeroed words: a closure's captures.
pub extern "C" fn yong_rt_alloc(words: u64) -> *mut u64 {
    Box::leak(vec![0u64; words as usize].into_boxed_slice()).as_mut_ptr()
}
