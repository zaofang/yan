//! The playground's analysis and run calls, over one long-lived query database, with a C ABI for
//! the page: the page writes a request into memory from `alloc`, calls `view`, `run` or
//! `hover`, and reads the JSON reply at `reply_ptr` for `reply_len` bytes.

use std::cell::RefCell;
use std::path::Path;

use serde_json::{Value, json};
use yong_db::{Database, Loaded};
use yong_diag::{DEFAULT_MAX_ERRORS, Severity, finish};
use yong_ide::ViewError;
use yong_interp::Stop;
use yong_names::Surface;

const SURFACES: [Surface; 3] = [Surface::Yong, Surface::Xin, Surface::Wen];

thread_local! {
    static DB: RefCell<Database> = RefCell::new(Database::new(false, false));
    static REPLY: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

fn path(surface: &str) -> String {
    format!("main.{surface}")
}

/// Diagnostics and the module printed in every surface. `surface` is the source's extension.
pub fn view(surface: &str, src: &str) -> Value {
    DB.with_borrow_mut(|db| {
        let path = path(surface);
        let f = db.file(Path::new(&path), src);
        let diagnostics = match yong_ide::diagnostics(db, f) {
            Ok(d) => json!(d),
            Err(msg) => json!([{ "severity": "tool", "message": msg }]),
        };
        let views: serde_json::Map<_, _> = SURFACES
            .iter()
            .map(|&s| {
                let name = yong_ide::surface_name(s);
                let text = if name == surface && s != Surface::Yong {
                    yong_ide::canonical(Path::new(&path), src)
                } else {
                    yong_ide::view(db, f, s)
                };
                (name.to_owned(), printed(text))
            })
            .collect();
        json!({ "diagnostics": diagnostics, "views": views })
    })
}

fn printed(r: Result<String, ViewError>) -> Value {
    match r {
        Ok(text) => json!({ "text": text }),
        Err(ViewError::Diagnostics(_)) => json!({ "error": "the module has errors" }),
        Err(ViewError::Outside(what)) => json!({ "error": format!("outside the subset: {what}") }),
        Err(ViewError::Tool(msg)) => json!({ "error": msg }),
    }
}

/// Check and run `fn main` in the interpreter, as `yong run` does.
pub fn run(surface: &str, src: &str) -> Value {
    DB.with_borrow_mut(|db| {
        let f = db.file(Path::new(&path(surface)), src);
        let module = match yong_db::load(db, f) {
            Ok(Loaded::Ready(m, _)) => {
                let diags = finish(
                    yong_ide::diagnostics(db, f).unwrap_or_default(),
                    DEFAULT_MAX_ERRORS,
                );
                if diags.iter().any(|d| d.severity == Severity::Error) {
                    return json!({ "error": "the module has errors" });
                }
                m
            }
            Ok(Loaded::Stopped(_)) => return json!({ "error": "the module has errors" }),
            Ok(Loaded::Unsupported(what, _)) => {
                return json!({ "error": format!("not supported yet: {what}") });
            }
            Err(msg) => return json!({ "error": msg }),
        };
        let mut out = Vec::new();
        let result = yong_interp::run(module, &mut out);
        let stdout = String::from_utf8_lossy(&out).into_owned();
        match result {
            Ok(status) => json!({ "stdout": stdout, "status": status }),
            Err(Stop::Panic(kind)) => json!({ "stdout": stdout, "panic": kind }),
            Err(Stop::Unsupported(what)) => {
                json!({ "stdout": stdout, "error": format!("not supported yet: {what}") })
            }
            Err(Stop::Limit(what)) => {
                json!({ "stdout": stdout, "error": format!("stage-0 {what} limit") })
            }
        }
    })
}

/// The type or signature at 1-based `line` and `col`.
pub fn hover(surface: &str, src: &str, line: u32, col: u32) -> Value {
    DB.with_borrow_mut(|db| {
        let f = db.file(Path::new(&path(surface)), src);
        json!(yong_ide::hover(db, f, line, col))
    })
}

/// A request is `surface\nsource`; hover's surface field is `surface:line:col`.
fn request(ptr: *mut u8, len: usize) -> String {
    // SAFETY: the page wrote `len` bytes at `ptr`, from `alloc(len)`, which it gives up here.
    #[allow(unsafe_code)]
    let bytes = unsafe { Vec::from_raw_parts(ptr, len, len) };
    String::from_utf8_lossy(&bytes).into_owned()
}

fn reply(v: &Value) {
    REPLY.with_borrow_mut(|r| *r = v.to_string().into_bytes());
}

fn split(req: &str) -> (&str, &str) {
    req.split_once('\n').unwrap_or((req, ""))
}

#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut v = std::mem::ManuallyDrop::new(Vec::<u8>::with_capacity(len));
    v.as_mut_ptr()
}

#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn reply_ptr() -> *const u8 {
    REPLY.with_borrow(|r| r.as_ptr())
}

#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn reply_len() -> usize {
    REPLY.with_borrow(|r| r.len())
}

#[allow(unsafe_code)]
#[unsafe(export_name = "view")]
pub extern "C" fn view_c(ptr: *mut u8, len: usize) {
    let req = request(ptr, len);
    let (surface, src) = split(&req);
    reply(&view(surface, src));
}

#[allow(unsafe_code)]
#[unsafe(export_name = "run")]
pub extern "C" fn run_c(ptr: *mut u8, len: usize) {
    let req = request(ptr, len);
    let (surface, src) = split(&req);
    reply(&run(surface, src));
}

#[allow(unsafe_code)]
#[unsafe(export_name = "hover")]
pub extern "C" fn hover_c(ptr: *mut u8, len: usize) {
    let req = request(ptr, len);
    let (head, src) = split(&req);
    let mut parts = head.split(':');
    let surface = parts.next().unwrap_or("xin");
    let mut num = || parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
    let (line, col) = (num(), num());
    reply(&hover(surface, src, line, col));
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELLO: &str = "pub def main() uses io:\n    io.print(\"hello, world\")\n";

    #[test]
    fn views_xin_in_every_printed_surface() {
        let v = view("xin", HELLO);
        assert_eq!(v["views"]["xin"]["text"], HELLO);
        assert!(
            v["views"]["yong"]["text"]
                .as_str()
                .unwrap()
                .contains("std.io.print")
        );
        assert!(v["views"]["wen"]["error"].is_string());
    }

    #[test]
    fn runs_main() {
        let v = run("xin", HELLO);
        assert_eq!(v["stdout"], "hello, world\n");
        assert_eq!(v["status"], 0);
    }

    #[test]
    fn reports_errors_without_running() {
        let v = run("xin", "pub def main() uses io:\n    io.print(x)\n");
        assert!(v["error"].is_string());
        assert!(
            !view("xin", "pub def main() uses io:\n    io.print(x)\n")["diagnostics"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}
