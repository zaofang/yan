//! The C-ABI entry points native code calls (R561).

use std::collections::HashSet;
use std::io::Write;
use std::sync::{Mutex, OnceLock};

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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_print(ptr: *const u8, len: usize) {
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_panic(ptr: *const u8, len: usize) -> ! {
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

// `Str` and `List` values in native code are pointers to these blocks; `drop` (5.4) frees them.

/// A list of `words`-slot elements.
pub struct List {
    pub(crate) words: usize,
    pub(crate) data: Vec<u64>,
}

fn leak<T>(x: T) -> *mut T {
    crate::heap::block_taken();
    Box::into_raw(Box::new(x))
}

fn panic(kind: &str) -> ! {
    // SAFETY: a str is valid UTF-8.
    unsafe { yan_rt_panic(kind.as_ptr(), kind.len()) }
}

/// A `Str` block for `s`, owned by the program from now on.
pub fn str_block(s: &str) -> *mut String {
    leak(s.to_owned())
}

fn statics() -> &'static Mutex<HashSet<usize>> {
    static STATICS: OnceLock<Mutex<HashSet<usize>>> = OnceLock::new();
    STATICS.get_or_init(Mutex::default)
}

/// A `Str` block for a literal. It lives for the whole run, is not counted as live, and
/// [`yan_rt_str_free`] ignores it, so every use of the literal may drop it.
pub fn static_str_block(s: &str) -> *mut String {
    let p = Box::into_raw(Box::new(s.to_owned()));
    statics().lock().expect("not poisoned").insert(p as usize);
    p
}

/// A literal's `Str` block, built from its `len` bytes of UTF-8 at `ptr` by the entry point of a
/// native object.
///
/// # Safety
/// `ptr` points to `len` bytes of UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_static_str(ptr: *const u8, len: usize) -> *mut String {
    // SAFETY: forwarded.
    static_str_block(unsafe { text(ptr, len) })
}

/// Free a `Str` block, unless it is a literal's.
///
/// # Safety
/// `s` is a `Str` block that nothing uses afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_str_free(s: *mut String) {
    if statics()
        .lock()
        .expect("not poisoned")
        .contains(&(s as usize))
    {
        return;
    }
    crate::heap::block_freed();
    // SAFETY: the caller's contract; the block came from `leak`.
    drop(unsafe { Box::from_raw(s) });
}

/// Free a `List` block, not its elements' blocks.
///
/// # Safety
/// `l` is a `List` block that nothing uses afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_list_free(l: *mut List) {
    crate::heap::block_freed();
    // SAFETY: the caller's contract; the block came from `leak`.
    drop(unsafe { Box::from_raw(l) });
}

/// # Safety
/// `s` is a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_print_str(s: *const String) {
    // SAFETY: the caller's contract.
    let s = unsafe { &*s };
    // SAFETY: a String is UTF-8.
    unsafe { yan_rt_print(s.as_ptr(), s.len()) }
}

/// # Safety
/// `a` and `b` are `Str` blocks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_concat(a: *const String, b: *const String) -> *mut String {
    // SAFETY: the caller's contract.
    let (a, b) = unsafe { (&*a, &*b) };
    leak(format!("{a}{b}"))
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_show_i64(x: i64) -> *mut String {
    leak(x.to_string())
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_show_u64(x: u64) -> *mut String {
    leak(x.to_string())
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_show_f64(x: f64) -> *mut String {
    leak(crate::fmt::float_repr(x))
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_div_i64(a: i64, b: i64) -> f64 {
    crate::num::div_i128(a.into(), b.into())
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_div_u64(a: u64, b: u64) -> f64 {
    crate::num::div_i128(a.into(), b.into())
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_show_bool(x: u8) -> *mut String {
    leak(if x != 0 { "True" } else { "False" }.to_owned())
}

/// # Safety
/// `s` is a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_repr(s: *const String) -> *mut String {
    // SAFETY: the caller's contract.
    leak(crate::fmt::repr(unsafe { &*s }))
}

/// # Safety
/// `a` and `b` are `Str` blocks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_str_eq(a: *const String, b: *const String) -> u8 {
    // SAFETY: the caller's contract.
    u8::from(unsafe { *a == *b })
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_list_new(words: u64) -> *mut List {
    leak(List {
        words: words as usize,
        data: Vec::new(),
    })
}

/// The slots of a new last element, valid until the list next grows.
///
/// # Safety
/// `l` is a `List` block that nothing else borrows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_list_push(l: *mut List) -> *mut u64 {
    // SAFETY: the caller's contract.
    let l = unsafe { &mut *l };
    let at = l.data.len();
    l.data.resize(at + l.words, 0);
    l.data[at..].as_mut_ptr()
}

/// # Safety
/// `l` is a `List` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_list_len(l: *const List) -> i64 {
    // SAFETY: the caller's contract.
    let l = unsafe { &*l };
    l.data.len().checked_div(l.words).unwrap_or(0) as i64
}

/// The slots of element `i`; a `bounds` panic when there is none.
///
/// # Safety
/// `l` is a `List` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_list_at(l: *mut List, i: i64) -> *mut u64 {
    // SAFETY: the caller's contract.
    let n = unsafe { yan_rt_list_len(l) };
    if !(0..n).contains(&i) {
        panic("bounds");
    }
    // SAFETY: as above; `i` is in bounds.
    let l = unsafe { &mut *l };
    l.data[i as usize * l.words..].as_mut_ptr()
}

/// # Safety
/// `l` is a `List` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_list_copy(l: *const List) -> *mut List {
    // SAFETY: the caller's contract.
    let l = unsafe { &*l };
    leak(List {
        words: l.words,
        data: l.data.clone(),
    })
}

/// End the program with `code` once stdout is flushed: `main` returned or failed (8.1). With
/// `YAN_REPORT_LIVE` set, `live: N` goes to stderr first, as `yan run --report-live` prints it.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_exit(code: i64) -> ! {
    let _ = std::io::stdout().flush();
    if std::env::var_os("YAN_REPORT_LIVE").is_some() {
        eprintln!("live: {}", crate::heap::live_blocks());
    }
    std::process::exit(code as i32)
}

/// `words` zeroed words: a closure's captures. A header word before them holds the count.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_alloc(words: u64) -> *mut u64 {
    crate::heap::block_taken();
    let mut b = vec![0u64; words as usize + 1];
    b[0] = words;
    // SAFETY: the block has the header word.
    unsafe { Box::leak(b.into_boxed_slice()).as_mut_ptr().add(1) }
}

/// Free a block from [`yan_rt_alloc`]; null (a closure without captures) is ignored.
///
/// # Safety
/// `p` is null or from `yan_rt_alloc`, and nothing uses it afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_free_env(p: *mut u64) {
    if p.is_null() {
        return;
    }
    crate::heap::block_freed();
    // SAFETY: the caller's contract; the header is one word before.
    unsafe {
        let head = p.sub(1);
        let n = *head as usize + 1;
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(head, n)));
    }
}

/// Slots into words at `out`, with `Str` and `List` blocks built for them.
///
/// # Safety
/// `out` has room for the slots.
unsafe fn store_slots(out: *mut u64, slots: Vec<crate::jsonslots::Slot>) {
    use crate::jsonslots::Slot;
    fn flat(slots: Vec<Slot>) -> Vec<u64> {
        slots
            .into_iter()
            .map(|s| match s {
                Slot::Word(w) => w,
                Slot::Str(s) => str_block(&s) as u64,
                Slot::List(words, xs) => leak(List {
                    words,
                    data: flat(xs),
                }) as u64,
            })
            .collect()
    }
    for (i, w) in flat(slots).into_iter().enumerate() {
        // SAFETY: the caller's contract.
        unsafe { out.add(i).write(w) };
    }
}

/// `std.json.parse`: the slots of the `Result` into `out`.
///
/// # Safety
/// `src` and `schema` are `Str` blocks; `out` has room for the result's slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_json_parse(
    out: *mut u64,
    src: *const String,
    schema: *const String,
) {
    // SAFETY: the caller's contract.
    unsafe { store_slots(out, crate::jsonslots::parse(&*src, &*schema)) }
}

/// `std.json.dump`: the json text of the value in `v`.
///
/// # Safety
/// `v` holds the schema's root type; `schema` is a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_json_dump(v: *const u64, schema: *const String) -> *mut String {
    // SAFETY: the caller's contract.
    let schema = unsafe { &*schema };
    let w = unsafe { std::slice::from_raw_parts(v, crate::jsonslots::root_width(schema)) };
    // SAFETY: as above.
    match unsafe { crate::jsonslots::dump(w, schema) } {
        Some(s) => str_block(&s),
        None => panic("value"),
    }
}
