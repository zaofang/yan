//! The JSON kernel behind `std.json` (17.4, R569 to R575): a strict RFC 8259 parser and a
//! deterministic writer over [`Json`], which is the std `Value` without the interpreter.

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i64),
    /// Always finite (R570).
    Float(f64),
    Str(String),
    Array(Vec<Json>),
    /// In document order, keys unique.
    Object(Vec<(String, Json)>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Reason {
    Syntax(String),
    Depth,
    Duplicate(String),
    Range,
}

/// `line` and `col` start at 1 and `col` counts chars (R573).
#[derive(Clone, Debug, PartialEq)]
pub struct JsonError {
    pub line: usize,
    pub col: usize,
    pub reason: Reason,
}

struct P<'a> {
    s: &'a str,
    at: usize,
    max_depth: usize,
}

type R<T> = Result<T, (usize, Reason)>;

fn syntax<T>(at: usize, msg: &str) -> R<T> {
    Err((at, Reason::Syntax(msg.to_owned())))
}

impl P<'_> {
    fn peek(&self) -> Option<char> {
        self.s[self.at..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.at += c.len_utf8();
        Some(c)
    }

    fn ws(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.at += 1;
        }
    }

    fn expect(&mut self, lit: &str) -> R<()> {
        if self.s[self.at..].starts_with(lit) {
            self.at += lit.len();
            Ok(())
        } else {
            syntax(self.at, &format!("expected `{lit}`"))
        }
    }

    fn value(&mut self, depth: usize) -> R<Json> {
        self.ws();
        match self.peek() {
            None => syntax(self.at, "unexpected end of input"),
            Some('n') => self.expect("null").map(|()| Json::Null),
            Some('t') => self.expect("true").map(|()| Json::Bool(true)),
            Some('f') => self.expect("false").map(|()| Json::Bool(false)),
            Some('"') => self.string().map(Json::Str),
            Some('[') => self.array(depth + 1),
            Some('{') => self.object(depth + 1),
            Some('-' | '0'..='9') => self.number(),
            Some(_) => syntax(self.at, "unexpected character"),
        }
    }

    fn array(&mut self, depth: usize) -> R<Json> {
        if depth > self.max_depth {
            return Err((self.at, Reason::Depth));
        }
        self.at += 1;
        let mut xs = Vec::new();
        self.ws();
        if self.peek() == Some(']') {
            self.at += 1;
            return Ok(Json::Array(xs));
        }
        loop {
            xs.push(self.value(depth)?);
            self.ws();
            match self.peek() {
                Some(',') => self.at += 1,
                Some(']') => {
                    self.at += 1;
                    return Ok(Json::Array(xs));
                }
                _ => return syntax(self.at, "expected `,` or `]`"),
            }
        }
    }

    fn object(&mut self, depth: usize) -> R<Json> {
        if depth > self.max_depth {
            return Err((self.at, Reason::Depth));
        }
        self.at += 1;
        let mut fs: Vec<(String, Json)> = Vec::new();
        self.ws();
        if self.peek() == Some('}') {
            self.at += 1;
            return Ok(Json::Object(fs));
        }
        loop {
            self.ws();
            let key_at = self.at;
            if self.peek() != Some('"') {
                return syntax(self.at, "expected a string key");
            }
            let k = self.string()?;
            if fs.iter().any(|(j, _)| *j == k) {
                return Err((key_at, Reason::Duplicate(k)));
            }
            self.ws();
            if self.peek() != Some(':') {
                return syntax(self.at, "expected `:`");
            }
            self.at += 1;
            fs.push((k, self.value(depth)?));
            self.ws();
            match self.peek() {
                Some(',') => self.at += 1,
                Some('}') => {
                    self.at += 1;
                    return Ok(Json::Object(fs));
                }
                _ => return syntax(self.at, "expected `,` or `}`"),
            }
        }
    }

    fn hex4(&mut self) -> R<u32> {
        let mut n = 0;
        for _ in 0..4 {
            let Some(d) = self.peek().and_then(|c| c.to_digit(16)) else {
                return syntax(self.at, "expected four hex digits");
            };
            n = n * 16 + d;
            self.at += 1;
        }
        Ok(n)
    }

    fn string(&mut self) -> R<String> {
        self.at += 1;
        let mut out = String::new();
        loop {
            let at = self.at;
            match self.bump() {
                None => return syntax(at, "unterminated string"),
                Some('"') => return Ok(out),
                Some('\\') => {
                    let e = self.at;
                    match self.bump() {
                        Some('"') => out.push('"'),
                        Some('\\') => out.push('\\'),
                        Some('/') => out.push('/'),
                        Some('b') => out.push('\u{8}'),
                        Some('f') => out.push('\u{c}'),
                        Some('n') => out.push('\n'),
                        Some('r') => out.push('\r'),
                        Some('t') => out.push('\t'),
                        Some('u') => {
                            let hi = self.hex4()?;
                            let cp = match hi {
                                0xD800..=0xDBFF => {
                                    if !self.s[self.at..].starts_with("\\u") {
                                        return syntax(e, "unpaired surrogate escape");
                                    }
                                    self.at += 2;
                                    let lo = self.hex4()?;
                                    if !(0xDC00..=0xDFFF).contains(&lo) {
                                        return syntax(e, "unpaired surrogate escape");
                                    }
                                    0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                                }
                                0xDC00..=0xDFFF => return syntax(e, "unpaired surrogate escape"),
                                _ => hi,
                            };
                            out.push(char::from_u32(cp).expect("a scalar value"));
                        }
                        _ => return syntax(e, "invalid escape"),
                    }
                }
                Some(c) if c < '\u{20}' => return syntax(at, "control character in string"),
                Some(c) => out.push(c),
            }
        }
    }

    fn digits(&mut self) -> usize {
        let from = self.at;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.at += 1;
        }
        self.at - from
    }

    fn number(&mut self) -> R<Json> {
        let from = self.at;
        if self.peek() == Some('-') {
            self.at += 1;
        }
        match self.peek() {
            Some('0') => {
                self.at += 1;
                if self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    return syntax(self.at, "leading zero");
                }
            }
            Some('1'..='9') => {
                self.digits();
            }
            _ => return syntax(self.at, "expected a digit"),
        }
        let mut integral = true;
        if self.peek() == Some('.') {
            integral = false;
            self.at += 1;
            if self.digits() == 0 {
                return syntax(self.at, "expected a digit after `.`");
            }
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            integral = false;
            self.at += 1;
            if matches!(self.peek(), Some('+' | '-')) {
                self.at += 1;
            }
            if self.digits() == 0 {
                return syntax(self.at, "expected an exponent");
            }
        }
        let text = &self.s[from..self.at];
        if integral {
            return text
                .parse::<i64>()
                .map(Json::Int)
                .map_err(|_| (from, Reason::Range));
        }
        match text.parse::<f64>() {
            Ok(x) if x.is_finite() => Ok(Json::Float(x)),
            _ => Err((from, Reason::Range)),
        }
    }
}

fn locate(s: &str, at: usize, reason: Reason) -> JsonError {
    let before = &s[..at];
    JsonError {
        line: before.matches('\n').count() + 1,
        col: before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1,
        reason,
    }
}

fn run<T>(s: &str, max_depth: usize, f: impl FnOnce(&mut P) -> R<T>) -> Result<T, JsonError> {
    let mut p = P {
        s,
        at: 0,
        max_depth,
    };
    let v = f(&mut p).and_then(|v| {
        p.ws();
        if p.at < s.len() {
            syntax(p.at, "trailing content")
        } else {
            Ok(v)
        }
    });
    v.map_err(|(at, r)| locate(s, at, r))
}

/// `parse_value(s, max_depth)` (R571).
pub fn parse(s: &str, max_depth: usize) -> Result<Json, JsonError> {
    run(s, max_depth, |p| p.value(0))
}

/// `parse_float(s)`: a document that is exactly one number (R575). An integer widens exactly or
/// fails with `Range`.
pub fn parse_float(s: &str, max_depth: usize) -> Result<f64, JsonError> {
    let n = run(s, max_depth, |p| {
        p.ws();
        match p.peek() {
            Some('-' | '0'..='9') => p.number(),
            None => syntax(p.at, "unexpected end of input"),
            Some(_) => syntax(p.at, "expected a number"),
        }
    })?;
    match n {
        Json::Float(x) => Ok(x),
        Json::Int(i) if (i as f64) as i128 == i128::from(i) && i.unsigned_abs() <= 1 << 53 => {
            Ok(i as f64)
        }
        _ => Err(JsonError {
            line: 1,
            col: 1,
            reason: Reason::Range,
        }),
    }
}

/// A non-finite float reached the writer; the caller panics with `value` (R570).
#[derive(Debug, PartialEq)]
pub struct NonFinite;

fn escape(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < '\u{20}' => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn write(v: &Json, indent: Option<usize>, level: usize, out: &mut String) -> Result<(), NonFinite> {
    let newline = |out: &mut String, level: usize| {
        if let Some(n) = indent {
            out.push('\n');
            out.extend(std::iter::repeat_n(' ', n * level));
        }
    };
    match v {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Int(i) => out.push_str(&i.to_string()),
        Json::Float(x) if !x.is_finite() => return Err(NonFinite),
        // Debug is the shortest round-trip form and always has a `.` or an exponent.
        Json::Float(x) => out.push_str(&format!("{x:?}")),
        Json::Str(s) => escape(s, out),
        Json::Array(xs) if xs.is_empty() => out.push_str("[]"),
        Json::Array(xs) => {
            out.push('[');
            for (i, x) in xs.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, level + 1);
                write(x, indent, level + 1, out)?;
            }
            newline(out, level);
            out.push(']');
        }
        Json::Object(fs) if fs.is_empty() => out.push_str("{}"),
        Json::Object(fs) => {
            out.push('{');
            for (i, (k, x)) in fs.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, level + 1);
                escape(k, out);
                out.push_str(if indent.is_some() { ": " } else { ":" });
                write(x, indent, level + 1, out)?;
            }
            newline(out, level);
            out.push('}');
        }
    }
    Ok(())
}

/// `dump_value(v, indent)` (R572): compact when `indent` is `None`, else one member per line.
pub fn dump(v: &Json, indent: Option<usize>) -> Result<String, NonFinite> {
    let mut out = String::new();
    write(v, indent, 0, &mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use Json::*;

    fn ok(s: &str) -> Json {
        parse(s, 128).unwrap_or_else(|e| panic!("{s:?}: {e:?}"))
    }

    fn err(s: &str) -> JsonError {
        parse(s, 128).expect_err(s)
    }

    fn syntax_at(s: &str, line: usize, col: usize) {
        let e = err(s);
        assert!(
            matches!(e.reason, Reason::Syntax(_)) && (e.line, e.col) == (line, col),
            "{s:?}: {e:?}"
        );
    }

    #[test]
    fn valid_documents() {
        for (s, v) in [
            ("null", Null),
            ("true", Bool(true)),
            ("false", Bool(false)),
            ("0", Int(0)),
            ("-0", Int(0)),
            ("42", Int(42)),
            ("-17", Int(-17)),
            ("1.5", Float(1.5)),
            ("-1.5e2", Float(-150.0)),
            ("1E2", Float(100.0)),
            ("1e+2", Float(100.0)),
            ("1.0", Float(1.0)),
            ("0.1", Float(0.1)),
            ("-0.0", Float(-0.0)),
            ("\"\"", Str(String::new())),
            ("\"abc\"", Str("abc".into())),
            ("[]", Array(vec![])),
            ("{}", Object(vec![])),
            ("[1,2]", Array(vec![Int(1), Int(2)])),
            (" \t\r\n[ 1 , 2 ]\n", Array(vec![Int(1), Int(2)])),
            (
                "{\"a\":1,\"b\":[true,null]}",
                Object(vec![
                    ("a".into(), Int(1)),
                    ("b".into(), Array(vec![Bool(true), Null])),
                ]),
            ),
            ("[[[]]]", Array(vec![Array(vec![Array(vec![])])])),
        ] {
            assert_eq!(ok(s), v, "{s:?}");
        }
    }

    #[test]
    fn key_order_is_document_order() {
        assert_eq!(
            ok("{\"b\":1,\"a\":2,\"c\":3}"),
            Object(vec![
                ("b".into(), Int(1)),
                ("a".into(), Int(2)),
                ("c".into(), Int(3))
            ])
        );
    }

    #[test]
    fn string_escapes() {
        for (s, want) in [
            (r#""\"\\\/\b\f\n\r\t""#, "\"\\/\u{8}\u{c}\n\r\t"),
            (r#""\u0041""#, "A"),
            (r#""\u00e9""#, "é"),
            (r#""\u20AC""#, "€"),
            (r#""\ud83d\ude00""#, "😀"),
            (r#""\u0000""#, "\0"),
            ("\"héllo 語 😀\"", "héllo 語 😀"),
            ("\"\u{7f}\"", "\u{7f}"),
        ] {
            assert_eq!(ok(s), Str(want.into()), "{s:?}");
        }
    }

    #[test]
    fn number_edge_cases() {
        assert_eq!(ok("9223372036854775807"), Int(i64::MAX));
        assert_eq!(ok("-9223372036854775808"), Int(i64::MIN));
        assert_eq!(err("9223372036854775808").reason, Reason::Range);
        assert_eq!(err("-9223372036854775809").reason, Reason::Range);
        assert_eq!(err("123456789012345678901234567890").reason, Reason::Range);
        assert_eq!(ok("1e2"), Float(100.0));
        assert_eq!(ok("5e-324"), Float(5e-324));
        assert_eq!(ok("1.7976931348623157e308"), Float(f64::MAX));
        assert_eq!(ok("0.30000000000000004"), Float(0.1 + 0.2));
        assert_eq!(ok("9007199254740993.0"), Float(9007199254740992.0));
        assert_eq!(ok("100000000000000000000.0"), Float(1e20));
        assert_eq!(err("1e999").reason, Reason::Range);
        assert_eq!(err("-1e999").reason, Reason::Range);
        assert_eq!(ok("1e-999"), Float(0.0));
    }

    #[test]
    fn invalid_numbers() {
        for s in [
            "01",
            "-01",
            "00",
            "+1",
            ".5",
            "5.",
            "1.e2",
            "1e",
            "1e+",
            "-",
            "--1",
            "0x10",
            "1_000",
            "NaN",
            "Infinity",
            "-Infinity",
            "1 2",
            "0.0.0",
        ] {
            assert!(
                matches!(err(s).reason, Reason::Syntax(_)),
                "{s:?}: {:?}",
                err(s)
            );
        }
    }

    #[test]
    fn invalid_documents() {
        for s in [
            "",
            "   ",
            "[",
            "]",
            "[1,]",
            "[,1]",
            "[1 2]",
            "{",
            "{\"a\"}",
            "{\"a\":}",
            "{\"a\":1,}",
            "{a:1}",
            "{'a':1}",
            "'a'",
            "\"abc",
            "tru",
            "nul",
            "True",
            "[1]]",
            "{} {}",
            "1 // c",
            "/* c */ 1",
            "[1, /* c */ 2]",
            "{\"a\":1}x",
            "\u{feff}1",
            "\u{a0}1",
            "\u{b}1",
            "\u{c}1",
            "\"a\nb\"",
            "\"a\tb\"",
            "\"\u{1f}\"",
        ] {
            assert!(matches!(err(s).reason, Reason::Syntax(_)), "{s:?}");
        }
    }

    #[test]
    fn invalid_escapes() {
        for s in [
            r#""\x""#,
            r#""\'""#,
            r#""\u12""#,
            r#""\u12G4""#,
            r#""\ud83d""#,
            r#""\ud83dx""#,
            r#""\ud83d\u0041""#,
            r#""\ude00""#,
            r#""\ude00\ud83d""#,
            "\"\\",
        ] {
            assert!(matches!(err(s).reason, Reason::Syntax(_)), "{s:?}");
        }
    }

    #[test]
    fn duplicate_keys() {
        let e = err("{\"a\":1,\"b\":2,\"a\":3}");
        assert_eq!(e.reason, Reason::Duplicate("a".into()));
        assert_eq!((e.line, e.col), (1, 14));
        assert_eq!(
            err("{\"a\":{\"x\":1,\"x\":2}}").reason,
            Reason::Duplicate("x".into())
        );
        // the same key at different levels is fine
        assert!(parse("{\"a\":{\"a\":1}}", 128).is_ok());
        // keys that differ after unescaping are the same key
        assert_eq!(
            err(r#"{"a":1,"\u0061":2}"#).reason,
            Reason::Duplicate("a".into())
        );
    }

    #[test]
    fn depth_limit() {
        let nest = |n: usize| format!("{}{}", "[".repeat(n), "]".repeat(n));
        assert!(parse(&nest(128), 128).is_ok());
        assert_eq!(parse(&nest(129), 128).unwrap_err().reason, Reason::Depth);
        assert!(parse(&nest(3), 3).is_ok());
        assert_eq!(parse(&nest(4), 3).unwrap_err().reason, Reason::Depth);
        assert_eq!(parse("1", 0).unwrap(), Int(1));
        assert_eq!(parse("[]", 0).unwrap_err().reason, Reason::Depth);
        assert_eq!(
            parse(&"{\"a\":".repeat(5), 4).unwrap_err().reason,
            Reason::Depth
        );
        // a deep document is refused without exhausting the stack
        assert_eq!(
            parse(&nest(100_000), 128).unwrap_err().reason,
            Reason::Depth
        );
    }

    #[test]
    fn error_positions() {
        syntax_at("", 1, 1);
        syntax_at("[1,]", 1, 4);
        syntax_at("[1\n,\n]", 3, 1);
        syntax_at("{\n  \"a\": x\n}", 2, 8);
        syntax_at("1 2", 1, 3);
        syntax_at("\"é\" x", 1, 5);
        syntax_at("\"😀\n\"", 1, 3);
        syntax_at("\n\n  ?", 3, 3);
        syntax_at("01", 1, 2);
    }

    #[test]
    fn writer_compact() {
        assert_eq!(
            dump(&ok("{ \"a\" : [ 1 , 2.5 , null ] }"), None).unwrap(),
            "{\"a\":[1,2.5,null]}"
        );
        assert_eq!(dump(&Array(vec![]), None).unwrap(), "[]");
        assert_eq!(dump(&Object(vec![]), None).unwrap(), "{}");
        assert_eq!(dump(&Null, None).unwrap(), "null");
    }

    #[test]
    fn writer_indented() {
        let v = ok("{\"a\":[1,{\"b\":true}],\"c\":{},\"d\":[]}");
        assert_eq!(
            dump(&v, Some(2)).unwrap(),
            "{\n  \"a\": [\n    1,\n    {\n      \"b\": true\n    }\n  ],\n  \"c\": {},\n  \"d\": []\n}"
        );
        assert_eq!(dump(&Array(vec![Int(1)]), Some(0)).unwrap(), "[\n1\n]");
        assert_eq!(dump(&Int(5), Some(4)).unwrap(), "5");
    }

    #[test]
    fn writer_floats() {
        for (x, s) in [
            (1.0, "1.0"),
            (-0.0, "-0.0"),
            (0.5, "0.5"),
            (0.1, "0.1"),
            (1e21, "1e21"),
            (1.5e300, "1.5e300"),
            (1e-7, "1e-7"),
            (123456.789, "123456.789"),
            (f64::MAX, "1.7976931348623157e308"),
            (5e-324, "5e-324"),
        ] {
            assert_eq!(dump(&Float(x), None).unwrap(), s);
            assert_eq!(ok(s), Float(x));
        }
        for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(dump(&Float(x), None), Err(NonFinite));
            assert_eq!(dump(&Array(vec![Float(x)]), None), Err(NonFinite));
        }
    }

    #[test]
    fn writer_strings() {
        assert_eq!(
            dump(
                &Str("a\"b\\c\n\r\t\u{8}\u{c}\u{1}\u{1f}/é語😀\u{7f}".into()),
                None
            )
            .unwrap(),
            "\"a\\\"b\\\\c\\n\\r\\t\\b\\f\\u0001\\u001f/é語😀\u{7f}\""
        );
        assert_eq!(dump(&Int(i64::MIN), None).unwrap(), "-9223372036854775808");
    }

    #[test]
    fn round_trips() {
        for s in [
            "null",
            "[1,2.5,\"x\",null,true,false]",
            "{\"k\":{\"z\":1,\"a\":[]},\"é\":\"\\u0001\"}",
            "[[],[[]],{}]",
            "-9223372036854775808",
        ] {
            assert_eq!(dump(&ok(s), None).unwrap(), s);
        }
        // pretty output parses back to the same value
        let v = ok("{\"a\":[1,2,{\"b\":null}],\"c\":\"d\"}");
        assert_eq!(ok(&dump(&v, Some(3)).unwrap()), v);
    }

    #[test]
    fn parse_float_documents() {
        let pf = |s: &str| parse_float(s, 128);
        assert_eq!(pf("1.5"), Ok(1.5));
        assert_eq!(pf(" 2 "), Ok(2.0));
        assert_eq!(pf("-0"), Ok(0.0));
        assert_eq!(pf("1e3"), Ok(1000.0));
        assert_eq!(pf("9007199254740992"), Ok(9007199254740992.0));
        assert_eq!(pf("9007199254740993").unwrap_err().reason, Reason::Range);
        assert_eq!(
            pf("99999999999999999999").unwrap_err().reason,
            Reason::Range
        );
        assert_eq!(pf("1e999").unwrap_err().reason, Reason::Range);
        for s in ["", "[1]", "\"1\"", "true", "1 2", "1,", "NaN", "+1", ".5"] {
            assert!(
                matches!(pf(s).unwrap_err().reason, Reason::Syntax(_)),
                "{s:?}"
            );
        }
    }
}
