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
    eprintln!("panic: {kind}");
    std::process::exit(101)
}
