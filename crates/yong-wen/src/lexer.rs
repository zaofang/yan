//! Wen tokens and layout (spec 15.1).
//!
//! Pass one scans raw tokens and tracks brackets, which decides SIGN and SPLPAREN inside
//! `[...]`. Pass two applies the layout rules: NEWLINE, INDENT and DEDENT, with continuation
//! lines joined.

use std::collections::HashSet;

use yong_diag::{Diagnostic, Span};

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    /// A value name or a soft keyword.
    Name(String),
    TypeName(String),
    /// A reserved word, `True`, `False` or `None`.
    Kw(&'static str),
    Int(String),
    Float(String),
    UnitNum {
        num: String,
        unit: String,
    },
    Temporal {
        kind: TemporalKind,
        text: String,
    },
    Str(LexStr),
    Char(String),
    Punct(&'static str),
    /// `-` or `+` inside a grid, after a space and before a non-space.
    Sign(&'static str),
    /// `(` inside a grid after a space.
    SplParen,
    Newline,
    Indent,
    Dedent,
    Eof,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemporalKind {
    Date,
    Time,
    DateTime,
    Instant,
    Zoned,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrKind {
    Plain,
    Raw,
    Bytes,
    C,
    Template,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LexStr {
    pub kind: StrKind,
    pub parts: Vec<LexPart>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LexPart {
    Text(String),
    Interp {
        toks: Vec<Token>,
        spec: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
    pub start: usize,
    pub end: usize,
}

pub struct Lexed {
    pub tokens: Vec<Token>,
    /// `---` doc comments: line and text.
    pub docs: Vec<(u32, String)>,
}

pub const RESERVED: [&str; 41] = [
    "let",
    "var",
    "take",
    "pub",
    "type",
    "trait",
    "impl",
    "dyn",
    "use",
    "as",
    "try",
    "if",
    "then",
    "else",
    "match",
    "is",
    "for",
    "in",
    "while",
    "loop",
    "break",
    "continue",
    "return",
    "par",
    "with",
    "unsafe",
    "extern",
    "and",
    "or",
    "not",
    "copy",
    "share",
    "self",
    "assert",
    "unreachable",
    "become",
    "fail",
    "True",
    "False",
    "None",
    "_",
];

// Longest first.
const PUNCTS: [&str; 49] = [
    "**=", "//=", "<<=", ">>=", "..=", "->", "==", "!=", "<=", ">=", "+=", "-=", "*=", "/=", "%=",
    "&=", "|=", "^=", "**", "//", "<<", ">>", "..", "@@", "(", ")", "[", "]", "{", "}", ",", ":",
    ";", ".", "+", "-", "*", "/", "%", "@", "&", "|", "^", "~", "<", ">", "=", "?", "#",
];

pub fn forbidden(c: char) -> bool {
    matches!(c,
        '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}' | '\u{061C}'
        | '\u{200B}'..='\u{200D}' | '\u{2060}' | '\u{FEFF}' | '\u{E0000}'..='\u{E007F}')
}

fn value_name_ok(s: &str) -> bool {
    let b = s.as_bytes();
    if !b[0].is_ascii_lowercase() || s.ends_with('_') {
        return false;
    }
    !s.contains("__")
        && b.iter()
            .all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

struct Raw {
    tok: Tok,
    span: Span,
    start: usize,
    end: usize,
    /// Indentation when this is the first token of a physical line outside brackets.
    bol: Option<u32>,
}

struct Lexer<'a> {
    file: &'a str,
    src: &'a str,
    pos: usize,
    end: usize,
    line: u32,
    col: u32,
    brackets: Vec<(char, Span)>,
    raws: Vec<Raw>,
    docs: Vec<(u32, String)>,
    diags: Vec<Diagnostic>,
    bad_names: HashSet<String>,
}

impl<'a> Lexer<'a> {
    fn peek(&self) -> Option<char> {
        self.src[self.pos..self.end].chars().next()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.src[self.pos..self.end].chars().nth(n)
    }

    fn rest(&self) -> &'a str {
        &self.src[self.pos..self.end]
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += c.len_utf8() as u32;
        }
        Some(c)
    }

    fn here(&self) -> (u32, u32) {
        (self.line, self.col)
    }

    fn span_from(&self, (line, col): (u32, u32)) -> Span {
        Span {
            line,
            col,
            end_line: self.line,
            end_col: self.col,
        }
    }

    fn error(&mut self, code: &str, span: Span, quote: &str, msg: impl Into<String>) {
        self.diags
            .push(Diagnostic::error(code, self.file, span, quote, msg));
    }

    fn run(&mut self) {
        let mut at_bol = self.brackets.is_empty();
        let mut indent = 0u32;
        let mut tab_reported = false;
        while let Some(c) = self.peek() {
            match c {
                '\n' => {
                    self.bump();
                    at_bol = true;
                    indent = 0;
                    tab_reported = false;
                }
                ' ' | '\r' => {
                    self.bump();
                    if at_bol {
                        indent += 1;
                    }
                }
                '\t' => {
                    let start = self.here();
                    self.bump();
                    if at_bol && self.brackets.is_empty() {
                        indent += 1;
                        if !tab_reported {
                            tab_reported = true;
                            let span = self.span_from(start);
                            self.error(
                                "E133",
                                span,
                                "\t",
                                "tab in indentation; indent with 2 spaces",
                            );
                        }
                    }
                }
                '-' if self.peek_at(1) == Some('-') => self.comment(),
                _ => {
                    let bol = (at_bol && self.brackets.is_empty()).then_some(indent);
                    at_bol = false;
                    self.token(bol);
                }
            }
        }
        for (open, span) in std::mem::take(&mut self.brackets) {
            if open != 'v' {
                self.error("E102", span, &open.to_string(), "unclosed bracket");
            }
        }
    }

    fn comment(&mut self) {
        let start = self.here();
        let lo = self.pos;
        let before_ok = lo == 0 || matches!(self.src.as_bytes()[lo - 1], b' ' | b'\n' | b'\t');
        let after = self.peek_at(2);
        let spaced = |c: Option<char>| matches!(c, None | Some(' ') | Some('\n'));
        let doc = after == Some('-') && spaced(self.peek_at(3));
        let ok = before_ok && (spaced(after) || doc || after == Some('>'));
        if !ok {
            let span = Span {
                end_col: start.1 + 2,
                ..self.span_from(start)
            };
            self.error(
                "E132",
                span,
                "--",
                "`--` starts a comment only when a space follows it",
            );
        }
        while let Some(c) = self.peek() {
            if c == '\n' {
                break;
            }
            let at = self.here();
            self.bump();
            if forbidden(c) {
                let span = self.span_from(at);
                self.error(
                    "E105",
                    span,
                    &format!("{:?}", c),
                    "forbidden invisible or bidirectional character",
                );
            }
        }
        if doc && ok {
            let text = self.src[lo + 3..self.pos].trim().to_owned();
            self.docs.push((start.0, text));
        }
    }

    fn push(&mut self, tok: Tok, start: (u32, u32), lo: usize, bol: Option<u32>) {
        let span = self.span_from(start);
        self.raws.push(Raw {
            tok,
            span,
            start: lo,
            end: self.pos,
            bol,
        });
    }

    fn in_grid(&self) -> bool {
        self.brackets.last().is_some_and(|(c, _)| *c == '[')
    }

    fn space_before(&self, lo: usize) -> bool {
        lo > 0 && matches!(self.src.as_bytes()[lo - 1], b' ' | b'\t' | b'\n')
    }

    fn token(&mut self, bol: Option<u32>) {
        let start = self.here();
        let lo = self.pos;
        let c = self.peek().expect("caller checked");
        if c.is_ascii_alphabetic() || c == '_' {
            return self.word(start, lo, bol);
        }
        if c.is_ascii_digit() {
            let tok = self.number();
            return self.push(tok, start, lo, bol);
        }
        if c == '"' {
            let tok = self.string(StrKind::Plain, start, lo);
            return self.push(tok, start, lo, bol);
        }
        if c == '`' {
            self.bump();
            while self.peek().is_some_and(|c| c != '`' && c != '\n') {
                self.bump();
            }
            if self.peek() == Some('`') {
                self.bump();
                let name = self.src[lo + 1..self.pos - 1].to_owned();
                return self.push(Tok::Name(name), start, lo, bol);
            }
            let span = self.span_from(start);
            return self.error("E102", span, "`", "unclosed quoted name");
        }
        if !c.is_ascii() {
            self.bump();
            let span = self.span_from(start);
            let quote = c.to_string();
            if forbidden(c) {
                self.error(
                    "E105",
                    span,
                    &format!("{c:?}"),
                    "forbidden invisible or bidirectional character",
                );
            } else {
                self.error(
                    "E100",
                    span,
                    &quote,
                    "non-ASCII character outside a string or comment",
                );
            }
            return;
        }
        let Some(&p) = PUNCTS.iter().find(|p| self.rest().starts_with(**p)) else {
            self.bump();
            let span = self.span_from(start);
            return self.error("E100", span, &c.to_string(), "unexpected character");
        };
        let grid = self.in_grid();
        let spaced = self.space_before(lo);
        for _ in 0..p.len() {
            self.bump();
        }
        let tok = match p {
            "(" | "[" | "{" => {
                let span = self.span_from(start);
                let split = p == "(" && grid && spaced;
                self.brackets.push((p.chars().next().unwrap(), span));
                if split { Tok::SplParen } else { Tok::Punct(p) }
            }
            ")" | "]" | "}" => {
                let want = match p {
                    ")" => '(',
                    "]" => '[',
                    _ => '{',
                };
                match self.brackets.last() {
                    Some((open, _)) if *open == want => {
                        self.brackets.pop();
                    }
                    _ => {
                        let span = self.span_from(start);
                        self.error("E100", span, p, "unmatched closing bracket");
                        return;
                    }
                }
                Tok::Punct(p)
            }
            "-" | "+" if grid && spaced && self.peek().is_some_and(|c| !c.is_whitespace()) => {
                Tok::Sign(p)
            }
            "#" => Tok::Kw("#"),
            _ => Tok::Punct(p),
        };
        self.push(tok, start, lo, bol);
    }

    fn word(&mut self, start: (u32, u32), lo: usize, bol: Option<u32>) {
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            self.bump();
        }
        let text = &self.src[lo..self.pos];
        let kind = match (text, self.peek()) {
            ("r", Some('"')) => Some(StrKind::Raw),
            ("b", Some('"')) => Some(StrKind::Bytes),
            ("c", Some('"')) => Some(StrKind::C),
            ("t", Some('"')) => Some(StrKind::Template),
            _ => None,
        };
        if let Some(kind) = kind {
            let tok = self.string(kind, start, lo);
            return self.push(tok, start, lo, bol);
        }
        if text == "c" && self.peek() == Some('\'') {
            let tok = self.char_lit(start);
            return self.push(tok, start, lo, bol);
        }
        let tok = if let Some(kw) = RESERVED.iter().find(|k| **k == text) {
            Tok::Kw(kw)
        } else if text.as_bytes()[0].is_ascii_uppercase() {
            let b = text.as_bytes();
            // A one-letter first word is allowed: `CStr`, `XAxis`.
            let doubled = b.windows(2).enumerate().any(|(i, w)| {
                w[0].is_ascii_uppercase()
                    && w[1].is_ascii_uppercase()
                    && !(i == 0 && b.get(2).is_some_and(u8::is_ascii_lowercase))
            });
            if doubled || text.contains('_') {
                let (code, msg) = if doubled {
                    ("E112", "type names have no two consecutive capitals")
                } else {
                    ("E110", "type names are PascalCase without underscores")
                };
                if self.bad_names.insert(text.to_owned()) {
                    let span = self.span_from(start);
                    self.error(code, span, text, msg);
                }
            }
            Tok::TypeName(text.to_owned())
        } else {
            if !value_name_ok(text) && self.bad_names.insert(text.to_owned()) {
                let span = self.span_from(start);
                self.error(
                    "E110",
                    span,
                    text,
                    "value names are lowercase words joined by single underscores",
                );
            }
            Tok::Name(text.to_owned())
        };
        self.push(tok, start, lo, bol);
    }

    fn digits(&mut self, radix: u32) {
        while self.peek().is_some_and(|c| c.is_digit(radix) || c == '_') {
            self.bump();
        }
    }

    /// Exactly `n` ASCII digits starting `at` chars ahead.
    fn digits_at(&self, at: usize, n: usize) -> bool {
        (0..n).all(|i| self.peek_at(at + i).is_some_and(|c| c.is_ascii_digit()))
            && !self.peek_at(at + n).is_some_and(|c| c.is_ascii_digit())
    }

    fn number(&mut self) -> Tok {
        let lo = self.pos;
        let radix = match (self.peek(), self.peek_at(1)) {
            (Some('0'), Some('x')) => 16,
            (Some('0'), Some('o')) => 8,
            (Some('0'), Some('b')) => 2,
            _ => 10,
        };
        if radix != 10 {
            self.bump();
            self.bump();
            self.digits(radix);
            return Tok::Int(self.src[lo..self.pos].to_owned());
        }
        let len = self.rest().bytes().take_while(u8::is_ascii_digit).count();
        if len == 4
            && self.peek_at(4) == Some('-')
            && self.digits_at(5, 2)
            && self.peek_at(7) == Some('-')
            && self.digits_at(8, 2)
        {
            return self.temporal();
        }
        if len == 2
            && self.peek_at(2) == Some(':')
            && self.digits_at(3, 2)
            && self.peek_at(5) == Some(':')
            && self.digits_at(6, 2)
        {
            self.clock();
            return Tok::Temporal {
                kind: TemporalKind::Time,
                text: self.src[lo..self.pos].to_owned(),
            };
        }
        self.digits(10);
        let mut float = false;
        if self.peek() == Some('.') && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) {
            float = true;
            self.bump();
            self.digits(10);
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            let d = match self.peek_at(1) {
                Some('+' | '-') => 2,
                _ => 1,
            };
            if self.peek_at(d).is_some_and(|c| c.is_ascii_digit()) {
                float = true;
                for _ in 0..d {
                    self.bump();
                }
                self.digits(10);
            }
        }
        let num = self.src[lo..self.pos].to_owned();
        if self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            let ulo = self.pos;
            while self
                .peek()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                self.bump();
            }
            return Tok::UnitNum {
                num,
                unit: self.src[ulo..self.pos].to_owned(),
            };
        }
        if float {
            Tok::Float(num)
        } else {
            Tok::Int(num)
        }
    }

    /// `hh:mm[:ss[.frac]]`
    fn clock(&mut self) {
        for _ in 0..5 {
            self.bump();
        }
        if self.peek() == Some(':') && self.digits_at(1, 2) {
            for _ in 0..3 {
                self.bump();
            }
            if self.peek() == Some('.') && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) {
                self.bump();
                self.digits(10);
            }
        }
    }

    fn temporal(&mut self) -> Tok {
        let lo = self.pos;
        for _ in 0..10 {
            self.bump();
        }
        let mut kind = TemporalKind::Date;
        if self.peek() == Some('T') && self.digits_at(1, 2) && self.peek_at(3) == Some(':') {
            self.bump();
            self.clock();
            kind = TemporalKind::DateTime;
            match self.peek() {
                Some('Z') => {
                    self.bump();
                    kind = TemporalKind::Instant;
                }
                Some('+' | '-') if self.digits_at(1, 2) && self.peek_at(3) == Some(':') => {
                    for _ in 0..6 {
                        self.bump();
                    }
                    kind = TemporalKind::Instant;
                }
                _ => {}
            }
            if self.peek() == Some('[') {
                let save = (self.pos, self.line, self.col);
                self.bump();
                while self
                    .peek()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || "_/+-".contains(c))
                {
                    self.bump();
                }
                if self.peek() == Some(']') {
                    self.bump();
                    kind = TemporalKind::Zoned;
                } else {
                    (self.pos, self.line, self.col) = save;
                }
            }
        }
        Tok::Temporal {
            kind,
            text: self.src[lo..self.pos].to_owned(),
        }
    }

    fn char_lit(&mut self, start: (u32, u32)) -> Tok {
        self.bump();
        let mut out = String::new();
        loop {
            match self.peek() {
                None | Some('\n') => {
                    let span = self.span_from(start);
                    self.error("E102", span, "c'", "unclosed char literal");
                    return Tok::Char(out);
                }
                Some('\'') => {
                    self.bump();
                    return Tok::Char(out);
                }
                Some('\\') => {
                    if let Some(c) = self.escape() {
                        out.push(c);
                    }
                }
                Some(c) => {
                    self.bump();
                    out.push(c);
                }
            }
        }
    }

    /// At a backslash: decode a simple escape. Interpolation is handled by the caller.
    fn escape(&mut self) -> Option<char> {
        let start = self.here();
        self.bump();
        let c = self.bump()?;
        Some(match c {
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            '0' => '\0',
            '\\' | '"' | '\'' | '{' | '}' => c,
            'u' if self.peek() == Some('{') => {
                self.bump();
                let lo = self.pos;
                while self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                    self.bump();
                }
                let hex = &self.src[lo..self.pos];
                let v = u32::from_str_radix(hex, 16).ok().and_then(char::from_u32);
                if self.peek() == Some('}') && v.is_some() {
                    self.bump();
                    return v;
                }
                let span = self.span_from(start);
                self.error("E100", span, "\\u", "malformed unicode escape");
                return None;
            }
            _ => {
                let span = self.span_from(start);
                self.error("E100", span, &format!("\\{c}"), "unknown escape");
                return None;
            }
        })
    }

    fn string(&mut self, kind: StrKind, start: (u32, u32), lo: usize) -> Tok {
        let triple = self.rest().starts_with("\"\"\"");
        let quote_len = if triple { 3 } else { 1 };
        for _ in 0..quote_len {
            self.bump();
        }
        let mut parts = Vec::new();
        let mut text = String::new();
        loop {
            let Some(c) = self.peek() else {
                let span = self.span_from(start);
                self.error(
                    "E102",
                    span,
                    &self.src[lo..self.pos.min(lo + 1)],
                    "unclosed string",
                );
                break;
            };
            if c == '"' && (!triple || self.rest().starts_with("\"\"\"")) {
                for _ in 0..quote_len {
                    self.bump();
                }
                break;
            }
            if c == '\n' && !triple {
                let span = self.span_from(start);
                self.error("E102", span, "\"", "unclosed string");
                break;
            }
            let interp_open = match kind {
                StrKind::Plain | StrKind::C => c == '\\' && self.peek_at(1) == Some('('),
                StrKind::Template => c == '{' && self.peek_at(1) != Some('{'),
                _ => false,
            };
            if interp_open {
                if !text.is_empty() {
                    parts.push(LexPart::Text(std::mem::take(&mut text)));
                }
                let part = self.interpolation(kind == StrKind::Template);
                parts.push(part);
                continue;
            }
            match (kind, c) {
                (StrKind::Raw, _) => {
                    self.bump();
                    text.push(c);
                }
                (StrKind::Template, '{' | '}') if self.peek_at(1) == Some(c) => {
                    self.bump();
                    self.bump();
                    text.push(c);
                }
                (_, '\\') => {
                    let at = self.here();
                    if let Some(e) = self.escape() {
                        if e == '\0' && kind == StrKind::C {
                            let span = self.span_from(at);
                            self.error("E140", span, "\\0", "interior NUL in a C string");
                        }
                        text.push(e);
                    }
                }
                _ => {
                    self.bump();
                    text.push(c);
                }
            }
        }
        if !text.is_empty() || parts.is_empty() {
            parts.push(LexPart::Text(text));
        }
        Tok::Str(LexStr { kind, parts })
    }

    /// At `\(` (or `{` in a template): lex the expression up to the matching close and an
    /// optional `:spec`.
    fn interpolation(&mut self, template: bool) -> LexPart {
        let start = self.here();
        self.bump();
        if !template {
            self.bump();
        }
        let close = if template { '}' } else { ')' };
        let body = self.pos;
        let (line, col) = self.here();
        let mut depth = 0i32;
        let mut colon = None;
        loop {
            match self.peek() {
                None | Some('\n') => {
                    let span = self.span_from(start);
                    self.error("E102", span, "\\(", "unclosed interpolation");
                    return LexPart::Text(String::new());
                }
                Some('"') => {
                    self.bump();
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.bump();
                        if c == '\\' {
                            self.bump();
                        } else if c == '"' {
                            break;
                        }
                    }
                }
                Some(c) if depth == 0 && c == close => break,
                Some(':') if depth == 0 && colon.is_none() => {
                    colon = Some(self.pos);
                    self.bump();
                }
                Some(c) => {
                    if "([{".contains(c) {
                        depth += 1;
                    } else if ")]}".contains(c) {
                        depth -= 1;
                    }
                    self.bump();
                }
            }
        }
        let expr_end = colon.unwrap_or(self.pos);
        let spec = colon.map(|c| self.src[c + 1..self.pos].to_owned());
        self.bump();
        let mut sub = Lexer {
            file: self.file,
            src: self.src,
            pos: body,
            end: expr_end,
            line,
            col,
            brackets: vec![('v', self.span_from(start))],
            raws: Vec::new(),
            docs: Vec::new(),
            diags: Vec::new(),
            bad_names: std::mem::take(&mut self.bad_names),
        };
        sub.run();
        self.bad_names = sub.bad_names;
        self.diags.append(&mut sub.diags);
        let eof = Span {
            line: sub.line,
            col: sub.col,
            end_line: sub.line,
            end_col: sub.col,
        };
        let mut toks: Vec<Token> = sub
            .raws
            .into_iter()
            .map(|r| Token {
                tok: r.tok,
                span: r.span,
                start: r.start,
                end: r.end,
            })
            .collect();
        toks.push(Token {
            tok: Tok::Eof,
            span: eof,
            start: expr_end,
            end: expr_end,
        });
        LexPart::Interp { toks, spec }
    }
}

fn continues_line(tok: &Tok) -> bool {
    match tok {
        Tok::Punct(p) => matches!(
            *p,
            "." | "+"
                | "-"
                | "*"
                | "/"
                | "//"
                | "%"
                | "**"
                | "&"
                | "|"
                | "^"
                | "<<"
                | ">>"
                | "=="
                | "!="
                | "<"
                | "<="
                | ">"
                | ">="
        ),
        Tok::Kw(k) => matches!(*k, "and" | "or"),
        _ => false,
    }
}

/// Whether the logical line `line` is a function signature that has not reached its `=`.
fn open_signature(line: &[Token]) -> bool {
    let mut i = 0;
    while let Some(t) = line.get(i) {
        match &t.tok {
            Tok::Kw("pub") | Tok::Kw("unsafe") => i += 1,
            Tok::Name(w)
                if matches!(w.as_str(), "gen" | "sub" | "lemma" | "safe")
                    && matches!(line.get(i + 1).map(|t| &t.tok), Some(Tok::Name(_))) =>
            {
                i += 1
            }
            _ => break,
        }
    }
    let named = matches!(line.get(i).map(|t| &t.tok), Some(Tok::Name(_)))
        && matches!(line.get(i + 1).map(|t| &t.tok), Some(Tok::Punct("(" | "[")));
    let mut depth = 0i32;
    for t in line {
        match t.tok {
            Tok::Punct("(" | "[" | "{") | Tok::SplParen => depth += 1,
            Tok::Punct(")" | "]" | "}") => depth -= 1,
            Tok::Punct("=") if depth == 0 => return false,
            _ => {}
        }
    }
    named && !matches!(line.last().map(|t| &t.tok), Some(Tok::Punct("->")))
}

fn layout(raws: Vec<Raw>, file: &str, diags: &mut Vec<Diagnostic>) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::new();
    let mut stack = vec![0u32];
    let mut line_start = 0usize;
    let mut line_indent = 0u32;
    let marker = |span: Span, tok: Tok| Token {
        tok,
        span: Span {
            line: span.end_line,
            col: span.end_col,
            end_line: span.end_line,
            end_col: span.end_col,
        },
        start: 0,
        end: 0,
    };
    for raw in raws {
        let tok = Token {
            tok: raw.tok,
            span: raw.span,
            start: raw.start,
            end: raw.end,
        };
        if let Some(ind) = raw.bol {
            if let Some(prev) = out.last() {
                let sig = tok.tok == Tok::Kw("fail") || tok.tok == Tok::Name("uses".into());
                if continues_line(&tok.tok)
                    || (sig && ind > line_indent && open_signature(&out[line_start..]))
                {
                    out.push(tok);
                    continue;
                }
                let span = prev.span;
                out.push(marker(span, Tok::Newline));
            }
            let top = *stack.last().expect("stack has the base level");
            if ind > top {
                stack.push(ind);
                out.push(marker(tok.span, Tok::Indent));
            } else {
                while ind < *stack.last().expect("base level") {
                    stack.pop();
                    out.push(marker(tok.span, Tok::Dedent));
                }
                if ind > *stack.last().expect("base level") {
                    diags.push(Diagnostic::error(
                        "E100",
                        file,
                        tok.span,
                        "",
                        "indentation does not match any enclosing block",
                    ));
                    stack.push(ind);
                }
            }
            line_start = out.len();
            line_indent = ind;
        }
        out.push(tok);
    }
    let end = out.last().map(|t| t.span).unwrap_or(Span {
        line: 1,
        col: 1,
        end_line: 1,
        end_col: 1,
    });
    if !out.is_empty() {
        out.push(marker(end, Tok::Newline));
    }
    for _ in 1..stack.len() {
        out.push(marker(end, Tok::Dedent));
    }
    out.push(marker(end, Tok::Eof));
    out
}

pub fn lex(file: &str, src: &str) -> (Lexed, Vec<Diagnostic>) {
    let mut lx = Lexer {
        file,
        src,
        pos: 0,
        end: src.len(),
        line: 1,
        col: 1,
        brackets: Vec::new(),
        raws: Vec::new(),
        docs: Vec::new(),
        diags: Vec::new(),
        bad_names: HashSet::new(),
    };
    if src.starts_with('\u{FEFF}') {
        let span = Span {
            line: 1,
            col: 1,
            end_line: 1,
            end_col: 4,
        };
        lx.error("E105", span, "\\u{feff}", "byte order mark; run yong fmt");
        lx.pos = 3;
        lx.col = 4;
    }
    lx.run();
    let mut diags = lx.diags;
    let tokens = layout(lx.raws, file, &mut diags);
    (
        Lexed {
            tokens,
            docs: lx.docs,
        },
        diags,
    )
}
