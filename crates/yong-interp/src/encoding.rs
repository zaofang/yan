//! std.encoding: `bytes`, `bytearray`, UTF-8, hex and base64 (R579 to R585).

use std::rc::Rc;

use yong_std::registry::{Id, id::*};

use super::bind::{Bind, bad_args};
use super::native::opt;
use super::{Value, panic};
use Value::*;

fn bytes(b: impl Into<Rc<[u8]>>) -> Value {
    Bytes(b.into())
}

fn error(at: usize, kind: &str) -> Value {
    Data {
        name: "EncodingError".into(),
        fields: vec![
            ("at".into(), Int(at as i128)),
            (
                "reason".into(),
                Data {
                    name: kind.into(),
                    fields: Vec::new(),
                },
            ),
        ],
    }
}

/// `invalid <kind> at <at>` (R583).
pub(super) fn show_error(fields: &[(super::Name, Value)]) -> String {
    let [(_, Int(at)), (_, Data { name, .. })] = fields else {
        unreachable!("an EncodingError")
    };
    let kind = match name.as_str() {
        "Utf8" => "UTF-8",
        "Hex" => "hex",
        _ => "base64",
    };
    format!("invalid {kind} at {at}")
}

fn byte(v: &Value) -> u8 {
    match v {
        Int(i) => *i as u8,
        _ => unreachable!("a uint8"),
    }
}

/// CPython's repr of a byte string, always single-quoted (R584).
pub(super) fn repr(b: &[u8]) -> String {
    let mut s = String::from("b'");
    for &c in b {
        match c {
            b'\\' => s.push_str("\\\\"),
            b'\'' => s.push_str("\\'"),
            b'\t' => s.push_str("\\t"),
            b'\n' => s.push_str("\\n"),
            b'\r' => s.push_str("\\r"),
            0x20..=0x7e => s.push(c as char),
            _ => s.push_str(&format!("\\x{c:02x}")),
        }
    }
    s.push('\'');
    s
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|c| format!("{c:02x}")).collect()
}

fn hex_decode(s: &str) -> Result<Value, Value> {
    let ds = s
        .chars()
        .enumerate()
        .map(|(i, c)| c.to_digit(16).ok_or_else(|| error(i, "Hex")))
        .collect::<Result<Vec<_>, _>>()?;
    if ds.len() % 2 == 1 {
        return Err(error(ds.len(), "Hex"));
    }
    Ok(bytes(
        ds.chunks(2)
            .map(|d| (d[0] * 16 + d[1]) as u8)
            .collect::<Vec<_>>(),
    ))
}

fn alphabet(url: bool) -> &'static [u8; 64] {
    if url {
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
    } else {
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
    }
}

fn base64(b: &[u8], url: bool, pad: bool) -> String {
    let a = alphabet(url);
    let mut s = String::new();
    for chunk in b.chunks(3) {
        let n = chunk.iter().fold(0u32, |n, &c| n << 8 | c as u32) << (8 * (3 - chunk.len()));
        for k in 0..=chunk.len() {
            s.push(a[(n >> (18 - 6 * k) & 63) as usize] as char);
        }
        if pad {
            s.push_str(&"=="[..3 - chunk.len()]);
        }
    }
    s
}

/// Strict: one accepted spelling per byte string and alphabet (R582).
fn base64_decode(s: &str, url: bool) -> Result<Value, Value> {
    let a = alphabet(url);
    let cs: Vec<char> = s.chars().collect();
    let body = cs.len() - cs.iter().rev().take_while(|c| **c == '=').count();
    let pads = cs.len() - body;
    let mut vals = Vec::with_capacity(body);
    for (i, c) in cs[..body].iter().enumerate() {
        match a.iter().position(|x| *x as char == *c) {
            Some(v) => vals.push(v as u32),
            None => return Err(error(i, "Base64")),
        }
    }
    if body % 4 == 1 || (pads > 0 && (!cs.len().is_multiple_of(4) || pads != 4 - body % 4)) {
        return Err(error(body, "Base64"));
    }
    let mut out = Vec::with_capacity(body * 3 / 4);
    for (g, chunk) in vals.chunks(4).enumerate() {
        let n = chunk.iter().fold(0u32, |n, v| n << 6 | v) << (6 * (4 - chunk.len()));
        let keep = chunk.len() - 1;
        // Bits past the last whole byte must be zero.
        if n & ((1 << (8 * (3 - keep))) - 1) != 0 {
            return Err(error(g * 4 + chunk.len() - 1, "Base64"));
        }
        out.extend((0..keep).map(|k| (n >> (16 - 8 * k)) as u8));
    }
    Ok(bytes(out))
}

macro_rules! f {
    ($id:ident, |$a:ident| { $($pat:pat => $e:expr),+ $(,)? }) => {
        (
            $id,
            Bind::Plain(|$a| {
                Ok(match &mut *$a {
                    $($pat => $e,)+
                    #[allow(unreachable_patterns)]
                    _ => return bad_args($id),
                })
            }),
        )
    };
}

macro_rules! fallible {
    ($id:ident, |$a:ident| { $($pat:pat => $e:expr),+ $(,)? }) => {
        (
            $id,
            Bind::Fallible(|$a| {
                Ok(match $a {
                    $($pat => $e,)+
                    #[allow(unreachable_patterns)]
                    _ => return bad_args($id),
                })
            }),
        )
    };
}

pub(super) static BINDS: &[(Id, Bind)] = &[
    f!(STD_ENCODING_FROM_LIST, |a| { [List(xs)] => bytes(xs.iter().map(byte).collect::<Vec<_>>()) }),
    f!(STD_TEXT_BYTES, |a| { [Str(s)] => bytes(s.as_bytes()) }),
    f!(STD_ENCODING_LEN, |a| { [Bytes(b)] => Int(b.len() as i128) }),
    f!(STD_ENCODING_AT, |a| {
        [Bytes(b), Int(i)] => opt(usize::try_from(*i).ok().and_then(|i| b.get(i)).map(|c| Int(*c as i128))),
    }),
    f!(STD_ENCODING_SLICE_EXACT, |a| {
        [Bytes(b), Int(x), Int(y)] => match (usize::try_from(*x), usize::try_from(*y)) {
            (Ok(x), Ok(y)) if x <= y && y <= b.len() => bytes(&b[x..y]),
            _ => return panic("bounds"),
        },
    }),
    f!(STD_ENCODING_CONCAT, |a| { [Bytes(x), Bytes(y)] => bytes([&x[..], &y[..]].concat()) }),
    f!(STD_ENCODING_TO_LIST, |a| { [Bytes(b)] => List(b.iter().map(|c| Int(*c as i128)).collect()) }),
    f!(STD_ENCODING_STARTS_WITH, |a| { [Bytes(b), Bytes(p)] => Bool(b.starts_with(p)) }),
    f!(STD_ENCODING_ENDS_WITH, |a| { [Bytes(b), Bytes(p)] => Bool(b.ends_with(p)) }),
    f!(STD_ENCODING_FIND, |a| {
        [Bytes(b), Bytes(sub)] => opt(if sub.is_empty() {
            Some(Int(0))
        } else {
            b.windows(sub.len()).position(|w| w == &sub[..]).map(|i| Int(i as i128))
        }),
    }),
    fallible!(STD_ENCODING_DECODE_UTF8, |a| {
        [Bytes(b)] => std::str::from_utf8(b)
            .map(|s| Str(s.into()))
            .map_err(|e| error(e.valid_up_to(), "Utf8")),
    }),
    f!(STD_ENCODING_DECODE_UTF8_LOSSY, |a| { [Bytes(b)] => Str(String::from_utf8_lossy(b).into()) }),
    f!(STD_ENCODING_HEX, |a| { [Bytes(b)] => Str(hex(b).into()) }),
    fallible!(STD_ENCODING_HEX_DECODE, |a| { [Str(s)] => hex_decode(s) }),
    f!(STD_ENCODING_BASE64, |a| {
        [Bytes(b), Bool(url), Bool(pad)] => Str(base64(b, *url, *pad).into()),
    }),
    fallible!(STD_ENCODING_BASE64_DECODE, |a| { [Str(s), Bool(url)] => base64_decode(s, *url) }),
    f!(STD_PRELUDE_BYTEARRAY, |a| { [] => ByteArray(Vec::new()) }),
    f!(STD_ENCODING_BYTEARRAY_FROM_BYTES, |a| { [Bytes(b)] => ByteArray(b.to_vec()) }),
    f!(STD_ENCODING_BYTEARRAY_LEN, |a| { [ByteArray(b)] => Int(b.len() as i128) }),
    f!(STD_ENCODING_BYTEARRAY_APPEND, |a| {
        [ByteArray(b), x] => {
            b.push(byte(x));
            Unit
        }
    }),
    f!(STD_ENCODING_BYTEARRAY_EXTEND, |a| {
        [ByteArray(b), Bytes(x)] => {
            b.extend_from_slice(x);
            Unit
        }
    }),
    f!(STD_ENCODING_BYTEARRAY_POP, |a| {
        [ByteArray(b)] => match b.pop() {
            Some(c) => Int(c as i128),
            None => return panic("bounds"),
        },
    }),
    f!(STD_ENCODING_BYTEARRAY_CLEAR, |a| {
        [ByteArray(b)] => {
            b.clear();
            Unit
        }
    }),
    f!(STD_ENCODING_BYTEARRAY_TO_BYTES, |a| { [ByteArray(b)] => bytes(&b[..]) }),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(s: &str, url: bool) -> Option<Vec<u8>> {
        match base64_decode(s, url) {
            Ok(Bytes(b)) => Some(b.to_vec()),
            _ => None,
        }
    }

    #[test]
    fn base64_round_trips_and_is_strict() {
        for n in 0..10 {
            let b: Vec<u8> = (0..n).map(|i| (i * 37 + 250) as u8).collect();
            for url in [false, true] {
                for pad in [false, true] {
                    assert_eq!(dec(&base64(&b, url, pad), url), Some(b.clone()));
                }
            }
        }
        assert_eq!(base64(b"foob", false, true), "Zm9vYg==");
        assert_eq!(dec("Zm9vYh==", false), None, "non-zero trailing bits");
        assert_eq!(dec("Zm9vYg=", false), None, "short padding");
        assert_eq!(dec("Zm9v Yg==", false), None, "whitespace");
        assert_eq!(dec("Zg==Zg==", false), None, "padding in the middle");
        assert_eq!(dec("-_8=", false), None, "the other alphabet");
        assert_eq!(dec("-_8=", true), Some(vec![0xfb, 0xff]));
        assert_eq!(dec("Z", false), None);
    }

    #[test]
    fn repr_is_cpython_style() {
        assert_eq!(repr(b"a\x00'\\\n\xff"), r"b'a\x00\'\\\n\xff'");
    }
}
