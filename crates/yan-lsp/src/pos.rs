//! LSP positions (0-based line, character in the negotiated encoding) against Yan spans
//! (1-based line, 1-based byte column). Both readers count columns in bytes, so `utf-8` is
//! the identity and `utf-16` converts through the line's text.

use std::path::PathBuf;

use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf16,
}

impl Encoding {
    pub fn name(self) -> &'static str {
        match self {
            Encoding::Utf8 => "utf-8",
            Encoding::Utf16 => "utf-16",
        }
    }
}

fn line_of(text: &str, line: u32) -> &str {
    text.split('\n')
        .nth(line as usize)
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .unwrap_or("")
}

/// The LSP position of 1-based `line` and byte `col`.
pub fn to_lsp(text: &str, line: u32, col: u32, enc: Encoding) -> Value {
    let line0 = line.saturating_sub(1);
    let byte = col.saturating_sub(1) as usize;
    let character = match enc {
        Encoding::Utf8 => byte,
        Encoding::Utf16 => {
            let l = line_of(text, line0);
            let cut = byte.min(l.len());
            l.char_indices()
                .take_while(|(i, _)| *i < cut)
                .map(|(_, c)| c.len_utf16())
                .sum()
        }
    };
    json!({ "line": line0, "character": character })
}

/// The 1-based line and byte column of an LSP position.
pub fn from_lsp(text: &str, p: &Value, enc: Encoding) -> (u32, u32) {
    let line0 = p["line"].as_u64().unwrap_or(0) as u32;
    let ch = p["character"].as_u64().unwrap_or(0) as usize;
    let byte = match enc {
        Encoding::Utf8 => ch,
        Encoding::Utf16 => {
            let mut units = 0;
            let mut byte = 0;
            for c in line_of(text, line0).chars() {
                if units >= ch {
                    break;
                }
                units += c.len_utf16();
                byte += c.len_utf8();
            }
            byte
        }
    };
    (line0 + 1, byte as u32 + 1)
}

/// The position just past the last character of `text`.
pub fn end_of(text: &str, enc: Encoding) -> Value {
    let line0 = text.matches('\n').count() as u32;
    let last = line_of(text, line0);
    let line = last.len() as u32 + 1;
    to_lsp(text, line0 + 1, line, enc)
}

/// The path a document URI names. A `file:` URI is percent-decoded; any other URI keeps
/// its text, so the extension still picks the surface.
pub fn uri_to_path(uri: &str) -> PathBuf {
    let Some(rest) = uri.strip_prefix("file://") else {
        return PathBuf::from(uri);
    };
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    PathBuf::from(String::from_utf8_lossy(&out).into_owned())
}
