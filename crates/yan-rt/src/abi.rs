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

/// A panic operand, as in the interpreter's `Operand`: an integer or a type name.
#[derive(Debug, Clone, PartialEq)]
pub enum PanicOperand {
    Int(i128),
    Float(f64),
    Bool(bool),
    /// An item identity hash (R882).
    Hash([u8; 32]),
    Type(String),
}

static PANIC_FIELDS: Mutex<Vec<(&'static str, PanicOperand)>> = Mutex::new(Vec::new());

/// The operand fields of the panic being reported (R796, R883), valid until the process exits.
pub fn panic_fields() -> Vec<(&'static str, PanicOperand)> {
    PANIC_FIELDS.lock().expect("not poisoned").clone()
}

fn ops_fields(
    kind: &str,
    bits: u64,
    signed: u64,
    a: i64,
    b: i64,
) -> Vec<(&'static str, PanicOperand)> {
    let wide = |x: i64| {
        if signed == 0 && bits == 64 {
            i128::from(x as u64)
        } else {
            i128::from(x)
        }
    };
    let mut f = Vec::new();
    match kind {
        "overflow" => {
            let ty = format!("{}{bits}", if signed != 0 { 'i' } else { 'u' });
            f.push(("type", PanicOperand::Type(ty)));
            f.push(("left", PanicOperand::Int(wide(a))));
            f.push(("right", PanicOperand::Int(wide(b))));
        }
        "div_zero" => {
            f.push(("dividend", PanicOperand::Int(wide(a))));
            f.push(("divisor", PanicOperand::Int(wide(b))));
        }
        "bounds" => {
            f.push(("index", PanicOperand::Int(wide(a))));
            f.push(("length", PanicOperand::Int(wide(b))));
        }
        _ => {}
    }
    f
}

/// A panic of kind `ptr[..len]` with integer operands `a` and `b` of the integer type of `bits`
/// width and signedness `signed`: `overflow` carries `type`, `left`, `right`; `div_zero` carries
/// `dividend`, `divisor`; `bounds` carries `index`, `length`; the rest carry none. Reported as [`yan_rt_panic`].
///
/// # Safety
/// `ptr` points to `len` bytes of UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_panic_ops(
    ptr: *const u8,
    len: usize,
    bits: u64,
    signed: u64,
    a: i64,
    b: i64,
) -> ! {
    // SAFETY: forwarded.
    let kind = unsafe { text(ptr, len) };
    let f = ops_fields(kind, bits, signed, a, b);
    *PANIC_FIELDS.lock().expect("not poisoned") = f;
    // SAFETY: forwarded.
    unsafe { yan_rt_panic(ptr, len) }
}

/// A `bounds` panic for a missing dict key: `key_hash` is the hash of the canonical encoding
/// `enc[..enc_len]` built with [`crate::keyhash::Enc`], `key_type` the type name `ty[..ty_len]` (R884).
///
/// # Safety
/// `ptr`, `ty` point to `len`, `ty_len` bytes of UTF-8; `enc` to `enc_len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_panic_key(
    ptr: *const u8,
    len: usize,
    enc: *const u8,
    enc_len: usize,
    ty: *const u8,
    ty_len: usize,
) -> ! {
    // SAFETY: the caller's contract.
    let (encoded, ty) = unsafe { (std::slice::from_raw_parts(enc, enc_len), text(ty, ty_len)) };
    let f = key_fields(encoded, ty);
    *PANIC_FIELDS.lock().expect("not poisoned") = f;
    // SAFETY: forwarded.
    unsafe { yan_rt_panic(ptr, len) }
}

fn key_fields(encoded: &[u8], ty: &str) -> Vec<(&'static str, PanicOperand)> {
    vec![
        (
            "key_hash",
            PanicOperand::Int(i128::from(crate::keyhash::hash(encoded))),
        ),
        ("key_type", PanicOperand::Type(ty.to_owned())),
    ]
}

/// # Safety
/// `rec` points to `3 * n` words as for [`yan_rt_panic_fields`].
unsafe fn decode_fields(n: u64, rec: *const u64) -> Vec<(&'static str, PanicOperand)> {
    // SAFETY: the caller's contract.
    let words = unsafe { std::slice::from_raw_parts(rec, 3 * n as usize) };
    words
        .chunks(3)
        .map(|w| {
            // SAFETY: a name is a live `Str` block, which lives for the whole run.
            let name: &'static str = Box::leak(
                unsafe { &*(w[0] as *const String) }
                    .clone()
                    .into_boxed_str(),
            );
            let v = match w[1] {
                0 => PanicOperand::Int(i128::from(w[2] as i64)),
                1 => PanicOperand::Int(i128::from(w[2])),
                2 => PanicOperand::Float(f64::from_bits(w[2])),
                // SAFETY: a `Str` block of 64 hex digits.
                4 => PanicOperand::Hash(unhex(unsafe { &*(w[2] as *const String) })),
                _ => PanicOperand::Bool(w[2] != 0),
            };
            (name, v)
        })
        .collect()
}

fn unhex(s: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (o, p) in out.iter_mut().zip(s.as_bytes().chunks(2)) {
        *o = u8::from_str_radix(std::str::from_utf8(p).unwrap_or("0"), 16).unwrap_or(0);
    }
    out
}

/// A panic of kind `ptr[..len]` with `n` named scalar operands: `rec` holds `n` triples of a
/// `Str` block (the name), a tag (0 signed, 1 unsigned, 2 float bits, 3 bool, 4 hex hash `Str` block) and the value.
///
/// # Safety
/// `ptr` points to `len` bytes of UTF-8; `rec` to `3 * n` words whose first of each triple is a
/// live `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_panic_fields(
    ptr: *const u8,
    len: usize,
    n: u64,
    rec: *const u64,
) -> ! {
    // SAFETY: the caller's contract.
    let f = unsafe { decode_fields(n, rec) };
    *PANIC_FIELDS.lock().expect("not poisoned") = f;
    // SAFETY: forwarded.
    unsafe { yan_rt_panic(ptr, len) }
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
        "deadlock" => "every task is blocked",
        "wire" => "a value the Wire codec rejects",
        _ => "invalid value",
    }
}

// `Str` and `List` values in native code are pointers to these blocks; `drop` (5.4) frees them.

/// A list of `words`-slot elements.
pub struct List {
    pub(crate) words: usize,
    pub(crate) data: Vec<u64>,
    /// A `deque`'s `maxlen`, or -1 (`yan_rt::dq`).
    pub(crate) limit: i64,
}

pub(crate) fn leak<T>(x: T) -> *mut T {
    crate::heap::block_taken();
    Box::into_raw(Box::new(x))
}

pub(crate) fn panic(kind: &str) -> ! {
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

/// `main` failed: its error, shown, on stderr, then exit status 1.
///
/// # Safety
/// `s` is a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_fail_main(s: *const String, code: i64) -> ! {
    let _ = std::io::stdout().flush();
    // SAFETY: the caller's contract.
    eprintln!("{}", unsafe { &*s });
    yan_rt_exit(code)
}

/// `std.io.read_line`: the next stdin line without its terminator, or null at end of input.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_read_line() -> *mut String {
    use std::io::BufRead;
    let mut line = String::new();
    match std::io::stdin().lock().read_line(&mut line) {
        Ok(0) | Err(_) => std::ptr::null_mut(),
        Ok(_) => {
            let line = line.strip_suffix('\n').unwrap_or(&line);
            str_block(line.strip_suffix('\r').unwrap_or(line))
        }
    }
}

/// `std.io.read_all`: the rest of stdin.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_read_all() -> *mut String {
    use std::io::Read;
    let mut s = String::new();
    let _ = std::io::stdin().lock().read_to_string(&mut s);
    str_block(&s)
}

thread_local! {
    static FS_REASON: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// `std.fs.read` on the host: the file's text, or null with the reason kept for [`yan_rt_fs_reason`].
///
/// # Safety
/// `path` is a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_fs_read(path: *const String) -> *mut String {
    // SAFETY: the caller's contract.
    match std::fs::read_to_string(unsafe { &*path }) {
        Ok(s) => str_block(&s),
        Err(e) => {
            let reason = match e.kind() {
                std::io::ErrorKind::NotFound => "not found".to_owned(),
                std::io::ErrorKind::PermissionDenied => "permission denied".to_owned(),
                _ => e.to_string(),
            };
            FS_REASON.with(|r| *r.borrow_mut() = reason);
            std::ptr::null_mut()
        }
    }
}

pub(crate) fn set_fs_reason(reason: String) {
    FS_REASON.with(|r| *r.borrow_mut() = reason);
}

/// The reason the last `yan_rt_fs_read` on this thread failed.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_fs_reason() -> *mut String {
    FS_REASON.with(|r| str_block(&r.borrow()))
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
/// `s` is a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_grapheme_count(s: *const String) -> i64 {
    // SAFETY: the caller's contract.
    crate::text::grapheme_count(unsafe { &*s }) as i64
}

/// Part `i` (0 before, 1 separator, 2 after) of `s` partitioned at `sep`, as a new `Str` block.
///
/// # Safety
/// `s` and `sep` are `Str` blocks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_str_partition(
    s: *const String,
    sep: *const String,
    i: i64,
) -> *mut String {
    // SAFETY: the caller's contract.
    let (a, m, b) = crate::text::partition(unsafe { &*s }, unsafe { &*sep });
    str_block([a, m, b][i as usize])
}

/// # Safety
/// `s` is a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_char_count(s: *const String) -> i64 {
    // SAFETY: the caller's contract.
    unsafe { &*s }.chars().count() as i64
}

/// # Safety
/// `s` is a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_byte_len(s: *const String) -> i64 {
    // SAFETY: the caller's contract.
    unsafe { &*s }.len() as i64
}

/// `int(s)`: panics `conversion` unless `s.strip()` is an integer.
///
/// # Safety
/// `s` is a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_int_of(s: *const String) -> i64 {
    // SAFETY: the caller's contract.
    let s = unsafe { &*s };
    match crate::text::parse_int(s.trim()) {
        Some(i) => i as i64,
        None => panic("conversion"),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_trunc_f64(x: f64) -> i64 {
    crate::num::trunc_i64(x).unwrap_or_else(|| panic("conversion"))
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_round_f64(x: f64) -> i64 {
    crate::num::round_i64(x).unwrap_or_else(|| panic("conversion"))
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_int_to_f64(i: i64) -> f64 {
    i as f64
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_abs_i64(i: i64) -> i64 {
    i.checked_abs().unwrap_or_else(|| panic("overflow"))
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_abs_f64(x: f64) -> f64 {
    x.abs()
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_is_nan(x: f64) -> u8 {
    u8::from(x.is_nan())
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_is_inf(x: f64) -> u8 {
    u8::from(x.is_infinite())
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_is_finite(x: f64) -> u8 {
    u8::from(x.is_finite())
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_floor(x: f64) -> f64 {
    x.floor()
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_ceil(x: f64) -> f64 {
    x.ceil()
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_trunc(x: f64) -> f64 {
    x.trunc()
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_next_up(x: f64) -> f64 {
    x.next_up()
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_next_down(x: f64) -> f64 {
    x.next_down()
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_fmod(x: f64, y: f64) -> f64 {
    x % y
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_pow(x: f64, y: f64) -> f64 {
    crate::math::pow(x, y)
}

fn str_list<'a>(parts: impl Iterator<Item = &'a str>) -> *mut List {
    let l = yan_rt_list_new(1);
    for p in parts {
        // SAFETY: `l` is a fresh list of one-word elements that nothing else borrows.
        unsafe { *yan_rt_list_push(l) = str_block(p) as u64 };
    }
    l
}

/// # Safety
/// `s` is a `Str` block; `sep` is one when `tag` is 0.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_str_split(
    s: *const String,
    tag: i64,
    sep: *const String,
) -> *mut List {
    // SAFETY: the caller's contract.
    let s = unsafe { &*s };
    if tag == 0 {
        // SAFETY: the caller's contract.
        str_list(s.split(unsafe { &**sep }))
    } else {
        str_list(crate::text::split_whitespace(s))
    }
}

/// # Safety
/// `s` is a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_str_splitlines(s: *const String) -> *mut List {
    // SAFETY: the caller's contract.
    str_list(crate::text::splitlines(unsafe { &*s }))
}

/// The slots of a new element inserted at `i`, clamped as `list.insert` does (negative counts from
/// the end); later elements shift up. Valid until the list next grows.
///
/// # Safety
/// `l` is a `List` block that nothing else borrows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_list_insert(l: *mut List, i: i64) -> *mut u64 {
    // SAFETY: the caller's contract.
    let l = unsafe { &mut *l };
    let n = l.data.len().checked_div(l.words).unwrap_or(0);
    let at = crate::list::insert_at(i.into(), n) * l.words;
    l.data.splice(at..at, std::iter::repeat_n(0, l.words));
    l.data[at..].as_mut_ptr()
}

/// Removes element `i` of `l`; the caller has taken its value.
///
/// # Safety
/// `l` is a `List` block that nothing else borrows, and `i` is in range.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_list_remove(l: *mut List, i: i64) {
    // SAFETY: the caller's contract.
    let l = unsafe { &mut *l };
    let at = i as usize * l.words;
    l.data.drain(at..at + l.words);
}

/// A new list of elements `i..j`, shallow-copied; panics when out of range.
///
/// # Safety
/// `l` is a `List` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_list_slice_exact(l: *const List, i: i64, j: i64) -> *mut List {
    // SAFETY: the caller's contract.
    let l = unsafe { &*l };
    let w = l.words;
    let n = l.data.len().checked_div(w).unwrap_or(0);
    let (Ok(x), Ok(y)) = (usize::try_from(i), usize::try_from(j)) else {
        panic("bounds")
    };
    if x > y || y > n {
        panic("bounds");
    }
    leak(List {
        words: w,
        data: l.data[x * w..y * w].to_vec(),
        limit: -1,
    })
}

/// Parse `s` as an int into `out[0]`; 1 when it parsed.
///
/// # Safety
/// `s` is a `Str` block; `out` has one word.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_parse_int(s: *const String, out: *mut i64) -> u8 {
    // SAFETY: the caller's contract.
    match crate::text::parse_int(unsafe { &*s }) {
        Some(v) => {
            unsafe { *out = v as i64 };
            1
        }
        None => 0,
    }
}

/// Parse `s` as a float into `out[0]`; 1 when it parsed.
///
/// # Safety
/// `s` is a `Str` block; `out` has one word.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_parse_float(s: *const String, out: *mut f64) -> u8 {
    // SAFETY: the caller's contract.
    match crate::text::parse_float(unsafe { &*s }) {
        Some(v) => {
            unsafe { *out = v };
            1
        }
        None => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_ldexp(x: f64, n: i64) -> f64 {
    crate::math::ldexp(x, n.into())
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_frexp_m(x: f64) -> f64 {
    crate::math::frexp(x).0
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_frexp_e(x: f64) -> i64 {
    crate::math::frexp(x).1.into()
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_math1(op: i64, x: f64) -> f64 {
    crate::math::MATH1[op as usize](x)
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_math2(op: i64, x: f64, y: f64) -> f64 {
    crate::math::MATH2[op as usize](x, y)
}

/// # Safety
/// `xs` is a `List` block of floats; `ys` is one too, or null for a plain sum.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_dot_exact(xs: *const List, ys: *const List, mode: i64) -> f64 {
    let floats = |l: *const List| -> Vec<f64> {
        // SAFETY: the caller's contract.
        unsafe { &*l }
            .data
            .iter()
            .map(|w| f64::from_bits(*w))
            .collect()
    };
    let xs = floats(xs);
    let terms: Vec<(f64, f64)> = if ys.is_null() {
        xs.into_iter().map(|x| (x, 1.0)).collect()
    } else {
        let ys = floats(ys);
        if xs.len() != ys.len() {
            panic("value");
        }
        xs.into_iter().zip(ys).collect()
    };
    crate::math::dot(terms.into_iter(), crate::math::mode_of(mode))
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_rounded(op: i64, mode: i64, x: f64, y: f64, z: f64) -> f64 {
    crate::math::rounded(op, mode, x, y, z)
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_pown(x: f64, n: i64) -> f64 {
    crate::math::pown(x, n.into())
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_copysign(x: f64, y: f64) -> f64 {
    x.copysign(y)
}

/// `s` repeated `n` times, empty for `n <= 0` (`std.text.repeat`).
///
/// # Safety
/// `s` is a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_str_repeat(s: *const String, n: i64) -> *mut String {
    // SAFETY: the caller's contract.
    str_block(&unsafe { &*s }.repeat(n.max(0) as usize))
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
        limit: -1,
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
        let kind = "bounds";
        // SAFETY: a str is valid UTF-8.
        unsafe { yan_rt_panic_ops(kind.as_ptr(), kind.len(), 64, 1, i, n) }
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
        limit: l.limit,
    })
}

/// Reverse the elements of a list in place.
///
/// # Safety
/// `l` is a `List` block nothing else borrows.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_list_reverse(l: *mut List) {
    // SAFETY: the caller's contract.
    let l = unsafe { &mut *l };
    let w = l.words;
    if w == 0 {
        return;
    }
    let n = l.data.len() / w;
    for i in 0..n / 2 {
        for k in 0..w {
            l.data.swap(i * w + k, (n - 1 - i) * w + k);
        }
    }
}

/// End the program with `code` once stdout is flushed: `main` returned or failed (8.1). With
/// `YAN_REPORT_LIVE` set, `live: N` goes to stderr first, as `yan run --report-live` prints it.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_exit(code: i64) -> ! {
    let _ = std::io::stdout().flush();
    crate::actor::end_root();
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
                    limit: -1,
                }) as u64,
                Slot::Map {
                    schema,
                    kw,
                    vw,
                    entries,
                } => {
                    let m = crate::map::Map::new(&schema, kw, vw);
                    for (k, v) in entries {
                        // SAFETY: `k` is `kw` slots of a key not yet in `m`, `v` `vw` slots.
                        unsafe {
                            let at = crate::map::yan_rt_map_push(m, flat(k).as_ptr());
                            for (i, w) in flat(v).into_iter().enumerate() {
                                at.add(i).write(w);
                            }
                        }
                    }
                    m as u64
                }
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

/// The text of a `JsonError`.
///
/// # Safety
/// `v` holds a `JsonError`'s slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_json_error_show(v: *const u64) -> *mut String {
    // SAFETY: the caller's contract.
    let w = unsafe { std::slice::from_raw_parts(v, 14) };
    // SAFETY: as above.
    str_block(&unsafe { crate::jsonslots::show_error(w) })
}

/// `std.cluster.wire`, `wire_reply` and `wire_start`: the value in `v` after the `Wire` round
/// trip (R402) into `out`; 0, or 1 when the codec rejects it. A rejected start argument has no
/// call to fail and panics `wire` (`start` != 0).
///
/// # Safety
/// `v` holds the schema's root type and `out` has room for it; `schema` is a `Str` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_wire_roundtrip(
    out: *mut u64,
    v: *const u64,
    schema: *const String,
    start: i64,
) -> i64 {
    // SAFETY: the caller's contract.
    let sc: crate::wire::Schemas =
        serde_json::from_str(unsafe { &*schema }).expect("the elaborator's schema");
    // SAFETY: as above.
    let w = unsafe { std::slice::from_raw_parts(v, crate::wireslots::root_width(&sc)) };
    // SAFETY: as above.
    let (slots, why) = unsafe { crate::wireslots::roundtrip(&sc, w) };
    match why {
        None => {
            // SAFETY: as above.
            unsafe { store_slots(out, slots) };
            0
        }
        Some(why) if start != 0 => {
            let _ = std::io::stdout().flush();
            eprintln!("panic(wire): a start argument: {why:?}");
            std::process::exit(101)
        }
        Some(_) => {
            // SAFETY: as above.
            unsafe { store_slots(out, slots) };
            1
        }
    }
}

/// `std.csv.parse`: the slots of the `Result` into `out`.
///
/// # Safety
/// `src`, `delim` and `schema` are `Str` blocks; `out` has room for the result's slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_csv_parse(
    out: *mut u64,
    src: *const String,
    delim: *const String,
    schema: *const String,
) {
    // SAFETY: the caller's contract.
    unsafe { store_slots(out, crate::csvslots::parse(&*src, &*delim, &*schema)) }
}

/// `std.csv.dump`: the csv text of the rows in `rows`.
///
/// # Safety
/// `rows` is a `List` of the schema's row type; `delim` and `schema` are `Str` blocks.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_csv_dump(
    rows: *const List,
    delim: *const String,
    schema: *const String,
) -> *mut String {
    // SAFETY: the caller's contract.
    match unsafe { crate::csvslots::dump(&*rows, &*delim, &*schema) } {
        Some(s) => str_block(&s),
        None => panic("value"),
    }
}

/// The text of a `CsvError`.
///
/// # Safety
/// `v` holds a `CsvError`'s slots.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_csv_error_show(v: *const u64) -> *mut String {
    // SAFETY: the caller's contract.
    let w = unsafe { std::slice::from_raw_parts(v, crate::csvslots::ERROR_WIDTH) };
    // SAFETY: as above.
    str_block(&unsafe { crate::csvslots::show_error(w) })
}

#[cfg(test)]
mod panic_ops_tests {
    use super::*;

    #[test]
    fn fields_decode_by_tag() {
        let name = Box::into_raw(Box::new("n".to_owned()));
        let rec = [name as u64, 0, (-3i64) as u64, name as u64, 3, 1];
        // SAFETY: two triples whose names are live `Str` blocks.
        let f = unsafe { decode_fields(2, rec.as_ptr()) };
        assert_eq!(f[0], ("n", PanicOperand::Int(-3)));
        assert_eq!(f[1], ("n", PanicOperand::Bool(true)));
        assert_eq!(unhex(&"0a".repeat(32)), [10; 32]);
    }

    #[test]
    fn ops_fields_match_the_interpreter() {
        let f = ops_fields("overflow", 8, 0, 200, 100);
        assert_eq!(f[0], ("type", PanicOperand::Type("u8".into())));
        assert_eq!(f[1], ("left", PanicOperand::Int(200)));
        let f = ops_fields("bounds", 64, 1, 5, 3);
        assert_eq!(f[0], ("index", PanicOperand::Int(5)));
        assert_eq!(f[1], ("length", PanicOperand::Int(3)));
        let f = ops_fields("div_zero", 64, 0, -1, 0);
        assert_eq!(f[0], ("dividend", PanicOperand::Int(u64::MAX.into())));
        assert_eq!(f[1], ("divisor", PanicOperand::Int(0)));
    }

    #[test]
    fn key_fields_carry_hash_and_type() {
        let mut e = crate::keyhash::Enc::new();
        e.int(7);
        let f = key_fields(e.as_bytes(), "int");
        assert_eq!(f[0], ("key_hash", PanicOperand::Int(i128::from(e.hash()))));
        assert_eq!(f[1], ("key_type", PanicOperand::Type("int".into())));
    }
}

/// The running `cost` counters of native code (R610, R925): the count of each resource (0 steps,
/// 1 allocations), and a stack of (resource, deadline, bound, clause), outermost first.
struct Costs {
    counts: [u64; 2],
    stack: Vec<(usize, u64, i64, i64)>,
    next: [u64; 2],
}

thread_local! {
    static COSTS: std::cell::RefCell<Costs> = const {
        std::cell::RefCell::new(Costs {
            counts: [0; 2],
            stack: Vec::new(),
            next: [u64::MAX; 2],
        })
    };
}

const RESOURCES: [&str; 2] = ["steps", "alloc"];

fn cost_panic(resource: usize, bound: i64, clause: i64) -> ! {
    *PANIC_FIELDS.lock().expect("not poisoned") = vec![
        ("clause", PanicOperand::Int(clause.into())),
        (RESOURCES[resource], PanicOperand::Int(bound.into())),
    ];
    let _ = std::io::stdout().flush();
    eprintln!(
        "panic(contract): a function exceeded its `cost {} <= {bound}`",
        RESOURCES[resource]
    );
    std::process::exit(101)
}

/// Start a `cost <resource> <= bound` counter for the calling frame; the token ends it.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_cost_begin(resource: u64, bound: i64, clause: i64) -> u64 {
    let r = resource as usize;
    if bound < 0 {
        cost_panic(r, bound, clause);
    }
    COSTS.with_borrow_mut(|c| {
        let token = c.stack.len() as u64;
        let deadline = c.counts[r].saturating_add(bound as u64);
        c.stack.push((r, deadline, bound, clause));
        c.next[r] = c.next[r].min(deadline);
        token
    })
}

/// End the counters started since `token` was returned: the frame is leaving.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_cost_end(token: u64) {
    COSTS.with_borrow_mut(|c| {
        c.stack.truncate(token as usize);
        for r in 0..2 {
            c.next[r] = c
                .stack
                .iter()
                .filter(|e| e.0 == r)
                .map(|e| e.1)
                .min()
                .unwrap_or(u64::MAX);
        }
    });
}

/// Count `n` of `resource`; past the tightest bound on it, a `contract` panic.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_cost_count(resource: u64, n: u64) {
    let r = resource as usize;
    let hit = COSTS.with_borrow_mut(|c| {
        c.counts[r] += n;
        (c.counts[r] > c.next[r]).then(|| {
            let e = c
                .stack
                .iter()
                .filter(|e| e.0 == r)
                .min_by_key(|e| e.1)
                .expect("a counter");
            (e.2, e.3)
        })
    });
    if let Some((bound, clause)) = hit {
        cost_panic(r, bound, clause);
    }
}

/// `seeded(seed, algorithm)`: algorithm tag 0 is `ChaCha8`, 1 is `Pcg64Dxsm`.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_rand_seeded(seed: i64, algorithm: i64) -> i64 {
    crate::rand::seeded(seed as u64, algorithm == 1)
}

/// The handler for branch `k` of a `par` under `h`. A handler that cannot split ends the
/// program (E402 at run time, as the interpreter does).
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_rand_split(h: i64, k: i64) -> i64 {
    match crate::rand::branch(h, k as u64) {
        Some(b) => b,
        None => {
            eprintln!("yan: `seeded(seed, Pcg64Dxsm())` in a `par` cannot split (E402)");
            std::process::exit(101)
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_rand_next(h: i64) -> i64 {
    crate::rand::next(h) as i64
}

#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_rand_f64(h: i64) -> f64 {
    crate::rand::f64_from(crate::rand::next(h))
}

/// Uniform in `[0, n)`, `n > 0`.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_rand_below(h: i64, n: i64) -> i64 {
    crate::rand::below(n as u64, || crate::rand::next(h)) as i64
}
