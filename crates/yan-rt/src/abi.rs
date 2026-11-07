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
