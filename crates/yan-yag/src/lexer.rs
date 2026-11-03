//! Yan Agent tokens and the layout rules of 13.1: INDENT and DEDENT at line starts outside brackets,
//! newlines joined inside brackets.

use yan_diag::{Severity, Span};

#[derive(Clone, Debug, PartialEq)]
pub enum FPart {
    Lit(String),
    /// An interpolation: its source text, where that text starts, and the format spec.
    Expr {
        src: String,
        line: u32,
        col: u32,
        fmt: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Name(String),
    Int(String),
    Float(String),
    /// `prefix` is the lowercase prefix letters (`r`, `b`, `c`, or none).
    Str {
        prefix: String,
        value: String,
        single_quoted: bool,
    },
    FStr {
        template: bool,
        parts: Vec<FPart>,
    },
    Char(char),
    Op(&'static str),
    Newline,
    Indent,
    Dedent,
    Eof,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
    /// First token on its physical line.
    pub line_start: bool,
}

pub struct Issue {
    pub code: &'static str,
    pub severity: Severity,
    pub span: Span,
    pub message: String,
}

const OPS: [&str; 49] = [
    "**=", "//=", "<<=", ">>=", "...", "->", "**", "//", "<<", ">>", "<=", ">=", "==", "!=", "+=",
    "-=", "*=", "/=", "%=", "&=", "|=", "^=", "@@", "+", "-", "*", "/", "%", "@", "&", "|", "^",
    "~", "<", ">", "=", ".", ",", ":", "(", ")", "[", "]", "{", "}", "?", "!", ";", "\\",
];

/// Bidirectional controls, zero-width characters and tag characters (13.1, E105).
fn forbidden(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200B}'..='\u{200F}' | '\u{2060}' | '\u{FEFF}')
        || ('\u{E0000}'..='\u{E007F}').contains(&c)
}

struct Lexer<'a> {
    src: &'a str,
    i: usize,
    line: u32,
    line_start: usize,
    toks: Vec<Token>,
    issues: Vec<Issue>,
    brackets: Vec<(u8, Span)>,
    indents: Vec<u32>,
    at_line_start: bool,
    fresh_line: bool,
}

pub struct Lexed {
    pub tokens: Vec<Token>,
    pub issues: Vec<Issue>,
    /// A fatal error: the token stream stops before it.
    pub fatal: bool,
}

pub fn lex(src: &str) -> Lexed {
    let mut lx = Lexer {
        src,
        i: 0,
        line: 1,
        line_start: 0,
        toks: Vec::new(),
        issues: Vec::new(),
        brackets: Vec::new(),
        indents: vec![0],
        at_line_start: true,
        fresh_line: true,
    };
    let fatal = lx.run().is_err();
    let at = lx.here();
    if !fatal {
        if !matches!(lx.toks.last().map(|t| &t.tok), None | Some(Tok::Newline)) {
            lx.push_at(Tok::Newline, at);
        }
        while lx.indents.len() > 1 {
            lx.indents.pop();
            lx.push_at(Tok::Dedent, at);
        }
    }
    lx.push_at(Tok::Eof, at);
    Lexed {
        tokens: lx.toks,
        issues: lx.issues,
        fatal,
    }
}

impl Lexer<'_> {
    fn col(&self, at: usize) -> u32 {
        (at - self.line_start) as u32 + 1
    }

    fn here(&self) -> Span {
        let c = self.col(self.i);
        Span {
            line: self.line,
            col: c,
            end_line: self.line,
            end_col: c,
        }
    }

    fn span_from(&self, start: usize, line: u32, line_start: usize) -> Span {
        Span {
            line,
            col: (start - line_start) as u32 + 1,
            end_line: self.line,
            end_col: self.col(self.i),
        }
    }

    fn issue(
        &mut self,
        code: &'static str,
        severity: Severity,
        span: Span,
        message: impl Into<String>,
    ) {
        self.issues.push(Issue {
            code,
            severity,
            span,
            message: message.into(),
        });
    }

    fn push_at(&mut self, tok: Tok, span: Span) {
        let line_start = self.fresh_line;
        self.fresh_line = false;
        self.toks.push(Token {
            tok,
            span,
            line_start,
        });
    }

    fn peek(&self) -> Option<char> {
        self.src[self.i..].chars().next()
    }

    fn peek_at(&self, n: usize) -> Option<char> {
        self.src[self.i..].chars().nth(n)
    }

    fn newline(&mut self) {
        self.i += 1;
        self.line += 1;
        self.line_start = self.i;
        self.fresh_line = true;
    }

    fn run(&mut self) -> Result<(), ()> {
        loop {
            if self.at_line_start && self.brackets.is_empty() {
                self.layout();
                self.at_line_start = false;
            }
            let Some(c) = self.peek() else { break };
            let start = self.i;
            let (line, ls) = (self.line, self.line_start);
            match c {
                ' ' | '\t' | '\r' => self.i += 1,
                '\n' => {
                    if self.brackets.is_empty()
                        && !matches!(self.toks.last().map(|t| &t.tok), None | Some(Tok::Newline))
                    {
                        let at = self.here();
                        self.toks.push(Token {
                            tok: Tok::Newline,
                            span: at,
                            line_start: false,
                        });
                    }
                    self.newline();
                    self.at_line_start = self.brackets.is_empty();
                }
                '#' => self.comment(),
                '0'..='9' => self.number(),
                '.' if self.peek_at(1).is_some_and(|d| d.is_ascii_digit()) => self.number(),
                c if c.is_ascii_alphabetic() || c == '_' => {
                    while self
                        .peek()
                        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
                    {
                        self.i += 1;
                    }
                    let word = &self.src[start..self.i];
                    let lower = word.to_ascii_lowercase();
                    let prefix_ok = word.len() <= 2
                        && lower.chars().all(|c| "rbcft".contains(c))
                        && matches!(self.peek(), Some('"' | '\''));
                    if prefix_ok {
                        self.string(start, &lower)?;
                    } else {
                        let tok = Tok::Name(word.to_owned());
                        let sp = self.span_from(start, line, ls);
                        self.push_at(tok, sp);
                    }
                }
                '"' | '\'' => self.string(start, "")?,
                c if forbidden(c) => {
                    let sp = self.here();
                    self.issue(
                        "E105",
                        Severity::Error,
                        sp,
                        format!("forbidden character U+{:04X} outside a string", c as u32),
                    );
                    self.i += c.len_utf8();
                }
                _ => {
                    let Some(op) = OPS.iter().find(|op| self.src[self.i..].starts_with(**op))
                    else {
                        let sp = self.here();
                        self.issue(
                            "E100",
                            Severity::Error,
                            sp,
                            format!("unexpected character `{c}`"),
                        );
                        self.i += c.len_utf8();
                        continue;
                    };
                    self.i += op.len();
                    let sp = self.span_from(start, line, ls);
                    match *op {
                        "\\" => {
                            self.issue(
                                "E103",
                                Severity::Error,
                                sp,
                                "backslash continuation; break the line inside brackets",
                            );
                            if self.src[self.i..].starts_with('\n') {
                                self.newline();
                            }
                            continue;
                        }
                        ";" => {
                            self.issue(
                                "E103",
                                Severity::Error,
                                sp,
                                "`;` between statements; put each on its own line",
                            );
                            if self.brackets.is_empty() {
                                self.toks.push(Token {
                                    tok: Tok::Newline,
                                    span: sp,
                                    line_start: false,
                                });
                                self.fresh_line = true;
                            }
                            continue;
                        }
                        "(" | "[" | "{" => self.brackets.push((op.as_bytes()[0], sp)),
                        ")" | "]" | "}" => {
                            self.brackets.pop();
                        }
                        _ => {}
                    }
                    self.push_at(Tok::Op(op), sp);
                }
            }
        }
        if let Some((_, sp)) = self.brackets.first().copied() {
            self.issue("E102", Severity::Error, sp, "this bracket is never closed");
            return Err(());
        }
        Ok(())
    }

    /// At the start of a logical line: skip blank and comment lines, then emit INDENT or DEDENT.
    fn layout(&mut self) {
        loop {
            let mut width = 0;
            let mut j = self.i;
            while let Some(c) = self.src[j..].chars().next() {
                match c {
                    ' ' => width += 1,
                    '\t' => width = (width / 8 + 1) * 8,
                    _ => break,
                }
                j += 1;
            }
            match self.src[j..].chars().next() {
                None => {
                    self.i = j;
                    return;
                }
                Some('\n') | Some('\r') => {
                    self.i = j;
                    while self.peek() == Some('\r') {
                        self.i += 1;
                    }
                    if self.peek() == Some('\n') {
                        self.newline();
                    }
                }
                Some('#') => {
                    self.i = j;
                    self.comment();
                    if self.peek() == Some('\n') {
                        self.newline();
                    }
                }
                Some(_) => {
                    self.i = j;
                    let at = self.here();
                    let top = *self.indents.last().expect("indent stack is never empty");
                    if width > top {
                        self.indents.push(width);
                        self.toks.push(Token {
                            tok: Tok::Indent,
                            span: at,
                            line_start: false,
                        });
                    } else {
                        while width < *self.indents.last().expect("never empty") {
                            self.indents.pop();
                            self.toks.push(Token {
                                tok: Tok::Dedent,
                                span: at,
                                line_start: false,
                            });
                        }
                        if width != *self.indents.last().expect("never empty") {
                            self.issue(
                                "E100",
                                Severity::Error,
                                at,
                                "unindent does not match any outer indentation level",
                            );
                            self.indents.push(width);
                        }
                    }
                    self.fresh_line = true;
                    return;
                }
            }
        }
    }

    fn comment(&mut self) {
        while let Some(c) = self.peek() {
            if c == '\n' {
                break;
            }
            if forbidden(c) {
                let sp = self.here();
                self.issue(
                    "E105",
                    Severity::Error,
                    sp,
                    format!("forbidden character U+{:04X} in a comment", c as u32),
                );
            }
            self.i += c.len_utf8();
        }
    }

    fn number(&mut self) {
        let (start, line, ls) = (self.i, self.line, self.line_start);
        let radix = self.src[self.i..].get(..2).map(|p| p.to_ascii_lowercase());
        let mut float = false;
        if matches!(radix.as_deref(), Some("0x" | "0o" | "0b")) {
            self.i += 2;
            while self
                .peek()
                .is_some_and(|c| c.is_ascii_hexdigit() || c == '_')
            {
                self.i += 1;
            }
        } else {
            let digits = |lx: &mut Self| {
                while lx.peek().is_some_and(|c| c.is_ascii_digit() || c == '_') {
                    lx.i += 1;
                }
            };
            digits(self);
            if self.peek() == Some('.')
                && self
                    .peek_at(1)
                    .is_none_or(|c| c.is_ascii_digit() || !c.is_ascii_alphabetic() && c != '.')
            {
                float = true;
                self.i += 1;
                digits(self);
            }
            if matches!(self.peek(), Some('e' | 'E'))
                && (self.peek_at(1).is_some_and(|c| c.is_ascii_digit())
                    || matches!(self.peek_at(1), Some('+' | '-'))
                        && self.peek_at(2).is_some_and(|c| c.is_ascii_digit()))
            {
                float = true;
                self.i += 2;
                digits(self);
            }
        }
        let text = self.src[start..self.i].to_owned();
        let sp = self.span_from(start, line, ls);
        self.push_at(
            if float {
                Tok::Float(text)
            } else {
                Tok::Int(text)
            },
            sp,
        );
    }

    fn string(&mut self, start: usize, prefix: &str) -> Result<(), ()> {
        let (line, ls) = (self.line, self.line_start);
        let quote = self.peek().expect("caller saw a quote");
        let triple = self.src[self.i..].starts_with(&quote.to_string().repeat(3));
        self.i += if triple { 3 } else { 1 };
        let raw = prefix.contains('r');
        let interp = prefix.contains('f') || prefix.contains('t');
        let body_start = self.i;
        let (body_line, body_ls) = (self.line, self.line_start);
        loop {
            match self.peek() {
                None => return self.unclosed(start, line, ls),
                Some('\n') if !triple => return self.unclosed(start, line, ls),
                Some('\n') => self.newline(),
                Some('\\') => {
                    self.i += 1;
                    if let Some(c) = self.peek() {
                        if c == '\n' {
                            self.newline();
                        } else {
                            self.i += c.len_utf8();
                        }
                    }
                }
                Some(c) if c == quote => {
                    let closes =
                        !triple || self.src[self.i..].starts_with(&quote.to_string().repeat(3));
                    if closes {
                        break;
                    }
                    self.i += 1;
                }
                Some(c) => self.i += c.len_utf8(),
            }
        }
        let body = &self.src[body_start..self.i];
        self.i += if triple { 3 } else { 1 };
        let sp = self.span_from(start, line, ls);
        let single_quoted = quote == '\'';
        if prefix == "c" && single_quoted {
            let v = unescape(body);
            let mut cs = v.chars();
            return match (cs.next(), cs.next()) {
                (Some(c), None) => {
                    self.push_at(Tok::Char(c), sp);
                    Ok(())
                }
                _ => {
                    self.issue(
                        "E100",
                        Severity::Error,
                        sp,
                        "a char literal holds exactly one character",
                    );
                    self.push_at(Tok::Char('\0'), sp);
                    Ok(())
                }
            };
        }
        let tok = if interp {
            Tok::FStr {
                template: prefix.contains('t'),
                parts: self.fparts(body, body_line, body_start - body_ls + 1, raw),
            }
        } else {
            let value = if raw { body.to_owned() } else { unescape(body) };
            if prefix.contains('c') && value.contains('\0') {
                self.issue(
                    "E140",
                    Severity::Error,
                    sp,
                    "a C string literal cannot contain NUL",
                );
            }
            Tok::Str {
                prefix: prefix.replace(['f', 't'], ""),
                value,
                single_quoted,
            }
        };
        if single_quoted && !interp {
            self.issue(
                "N03",
                Severity::Note,
                sp,
                "single-quoted string; printed double-quoted",
            );
        }
        self.push_at(tok, sp);
        Ok(())
    }

    fn unclosed(&mut self, start: usize, line: u32, ls: usize) -> Result<(), ()> {
        let c = (start - ls) as u32 + 1;
        let sp = Span {
            line,
            col: c,
            end_line: line,
            end_col: c,
        };
        self.issue("E102", Severity::Error, sp, "this string is never closed");
        Err(())
    }

    /// Split an f-string body at `{...}` interpolations; `{{` and `}}` are literal braces.
    fn fparts(&mut self, body: &str, line: u32, col: usize, raw: bool) -> Vec<FPart> {
        let mut parts = Vec::new();
        let mut lit = String::new();
        let b = body.as_bytes();
        let mut k = 0;
        while k < b.len() {
            match b[k] {
                b'{' if b.get(k + 1) == Some(&b'{') => {
                    lit.push('{');
                    k += 2;
                }
                b'}' if b.get(k + 1) == Some(&b'}') => {
                    lit.push('}');
                    k += 2;
                }
                b'{' => {
                    if !lit.is_empty() {
                        parts.push(FPart::Lit(if raw {
                            mem_take(&mut lit)
                        } else {
                            unescape(&mem_take(&mut lit))
                        }));
                    }
                    let open = k;
                    let mut depth = 0;
                    let mut colon = None;
                    let mut quote: Option<u8> = None;
                    k += 1;
                    while k < b.len() {
                        let c = b[k];
                        match quote {
                            Some(q) if c == q => quote = None,
                            Some(_) => {}
                            None => match c {
                                b'"' | b'\'' => quote = Some(c),
                                b'(' | b'[' | b'{' => depth += 1,
                                b')' | b']' => depth -= 1,
                                b'}' if depth == 0 => break,
                                b'}' => depth -= 1,
                                b':' if depth == 0 && colon.is_none() => colon = Some(k),
                                _ => {}
                            },
                        }
                        k += 1;
                    }
                    let end = colon.unwrap_or(k);
                    parts.push(FPart::Expr {
                        src: body[open + 1..end].to_owned(),
                        line,
                        col: (col + open + 1) as u32,
                        fmt: colon.map(|c| body[c + 1..k.min(body.len())].to_owned()),
                    });
                    k += 1;
                }
                _ => {
                    let ch = body[k..].chars().next().expect("in bounds");
                    lit.push(ch);
                    k += ch.len_utf8();
                }
            }
        }
        if !lit.is_empty() {
            parts.push(FPart::Lit(if raw { lit } else { unescape(&lit) }));
        }
        parts
    }
}

fn mem_take(s: &mut String) -> String {
    std::mem::take(s)
}

pub fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut cs = s.chars().peekable();
    while let Some(c) = cs.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match cs.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('0') => out.push('\0'),
            Some('\n') => {}
            Some('x') => {
                let h: String = cs.by_ref().take(2).collect();
                out.extend(u32::from_str_radix(&h, 16).ok().and_then(char::from_u32));
            }
            Some('u') if cs.peek() == Some(&'{') => {
                cs.next();
                let h: String = cs.by_ref().take_while(|c| *c != '}').collect();
                out.extend(u32::from_str_radix(&h, 16).ok().and_then(char::from_u32));
            }
            Some(c) => out.push(c),
            None => out.push('\\'),
        }
    }
    out
}
