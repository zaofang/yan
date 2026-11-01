//! std.text kernels.

/// `s` split at the first `sep`: before, the separator, after; `(s, "", "")` when absent.
pub fn partition<'a>(s: &'a str, sep: &'a str) -> (&'a str, &'a str, &'a str) {
    match s.split_once(sep) {
        Some((a, b)) => (a, sep, b),
        None => (s, "", ""),
    }
}

/// Python's `str.splitlines()` boundaries, with no empty last line.
pub fn splitlines(s: &str) -> impl Iterator<Item = &str> {
    let mut rest = s;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let end = rest
            .find([
                '\n', '\r', '\x0b', '\x0c', '\x1c', '\x1d', '\x1e', '\u{85}', '\u{2028}',
                '\u{2029}',
            ])
            .unwrap_or(rest.len());
        let line = &rest[..end];
        let skip = match rest[end..].chars().next() {
            None => 0,
            Some('\r') if rest[end + 1..].starts_with('\n') => 2,
            Some(c) => c.len_utf8(),
        };
        rest = &rest[end + skip..];
        Some(line)
    })
}

/// An optional sign then ASCII digits, within `int` (i64).
pub fn parse_int(s: &str) -> Option<i128> {
    let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse::<i64>().ok().map(i128::from)
}

pub fn parse_float(s: &str) -> Option<f64> {
    let body = s.strip_prefix(['+', '-']).unwrap_or(s);
    let named = ["inf", "infinity", "nan"].contains(&body.to_ascii_lowercase().as_str());
    if !named
        && !body
            .bytes()
            .all(|b| b.is_ascii_digit() || b"._eE+-".contains(&b))
    {
        return None;
    }
    s.parse().ok()
}

#[cfg(test)]
mod selftest {
    use super::*;

    #[test]
    fn kernels() {
        assert_eq!(
            splitlines("a\r\nb\x0bc\n").collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
        assert_eq!(partition("k=v=w", "="), ("k", "=", "v=w"));
        assert_eq!(partition("kv", "="), ("kv", "", ""));
        assert_eq!(parse_int("-12"), Some(-12));
        assert_eq!(parse_int("1_0"), None);
        assert_eq!(parse_int("9223372036854775808"), None);
        assert_eq!(parse_float("1e3"), Some(1000.0));
        assert_eq!(parse_float("0x1"), None);
    }
}
