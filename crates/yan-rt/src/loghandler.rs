//! `log` handlers for native code: logfmt events and spans as child handlers (RFC 0019 D8 to
//! D10). Handler 0 is the default: info and above on stderr, with a timestamp.

use crate::abi::{List, yan_rt_print};
use crate::time::{NS, show_instant};
use std::cell::RefCell;
use std::io::Write as _;

const STDERR: i64 = 0;
const STDOUT: i64 = 1;
const DISCARD: i64 = 2;

#[derive(Clone)]
struct Handler {
    sink: i64,
    min: i64,
    span: String,
    fields: Vec<(String, String)>,
}

thread_local! {
    static HANDLERS: RefCell<Vec<Handler>> = RefCell::new(vec![Handler {
        sink: STDERR,
        min: 1,
        span: String::new(),
        fields: Vec::new(),
    }]);
}

/// The `(key, value)` pairs of a `list[tuple[str, str]]` block.
///
/// # Safety
/// `l` is such a list.
unsafe fn pairs(l: *const List) -> Vec<(String, String)> {
    // SAFETY: the caller's contract; each element is two `Str` blocks.
    unsafe { &*l }
        .data
        .chunks(2)
        .map(|e| unsafe {
            (
                (*(e[0] as *const String)).clone(),
                (*(e[1] as *const String)).clone(),
            )
        })
        .collect()
}

fn quoted(v: &str) -> String {
    if !v.is_empty()
        && !v
            .chars()
            .any(|c| c == ' ' || c == '=' || c == '"' || c.is_control())
    {
        return v.to_owned();
    }
    let mut out = String::from("\"");
    for c in v.chars() {
        match c {
            '"' => out += "\\\"",
            '\\' => out += "\\\\",
            '\n' => out += "\\n",
            c => out.push(c),
        }
    }
    out + "\""
}

fn push_pairs(fields: &[(String, String)], line: &mut String) {
    for (k, v) in fields {
        line.push_str(&format!(" {k}={}", quoted(v)));
    }
}

/// A handler writing to `sink` (stderr 0, stdout 1, discard 2) from level `min` up.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_log_new(sink: i64, min: i64) -> u64 {
    HANDLERS.with(|h| {
        let mut h = h.borrow_mut();
        h.push(Handler {
            sink,
            min,
            span: String::new(),
            fields: Vec::new(),
        });
        h.len() as u64 - 1
    })
}

/// A child of `h` named `name` with more `fields`.
///
/// # Safety
/// `name` is a `Str` block and `fields` a `list[tuple[str, str]]` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_log_span(h: u64, name: *const String, fields: *const List) -> u64 {
    // SAFETY: the caller's contract.
    let (name, fields) = unsafe { ((*name).clone(), pairs(fields)) };
    HANDLERS.with(|hs| {
        let mut hs = hs.borrow_mut();
        let mut child = hs[h as usize].clone();
        child.span = if child.span.is_empty() {
            name
        } else {
            format!("{}/{name}", child.span)
        };
        child.fields.extend(fields);
        hs.push(child);
        hs.len() as u64 - 1
    })
}

/// An event of level `rank` (debug 0 to error 3).
///
/// # Safety
/// `msg` is a `Str` block and `fields` a `list[tuple[str, str]]` block.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn yan_rt_log_event(
    h: u64,
    rank: i64,
    msg: *const String,
    fields: *const List,
) {
    let handler = HANDLERS.with(|hs| hs.borrow()[h as usize].clone());
    if handler.sink == DISCARD || rank < handler.min {
        return;
    }
    // SAFETY: the caller's contract.
    let (msg, fields) = unsafe { ((*msg).clone(), pairs(fields)) };
    let mut line = String::new();
    if handler.sink == STDERR {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as i128);
        let secs = show_instant(now - now % NS);
        line = format!("ts={}.{:06}Z ", secs.trim_end_matches('Z'), now % NS / 1000);
    }
    line += &format!(
        "level={} msg={}",
        ["debug", "info", "warn", "error"][rank.clamp(0, 3) as usize],
        quoted(&msg)
    );
    if !handler.span.is_empty() {
        line += &format!(" span={}", quoted(&handler.span));
    }
    push_pairs(&handler.fields, &mut line);
    push_pairs(&fields, &mut line);
    if handler.sink == STDOUT {
        // SAFETY: a line of UTF-8.
        unsafe { yan_rt_print(line.as_ptr(), line.len()) };
    } else {
        let _ = writeln!(std::io::stderr(), "{line}");
    }
}
