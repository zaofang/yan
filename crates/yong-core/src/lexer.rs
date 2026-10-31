//! Tokens of the Yong text format (spec 3.5).

use yong_diag::Span;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Name(String),
    Int(String),
    Float(String),
    Str(String),
    Char(char),
    Label(String),
    Punct(&'static str),
    Eof,
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug)]
pub struct LexError {
    pub code: &'static str,
    pub span: Span,
    pub start: usize,
    pub end: usize,
    pub message: String,
}

pub struct Lexed {
    /// Comment lines before the first token, kept verbatim: they carry conformance headers.
    pub header: Vec<String>,
    pub tokens: Vec<Token>,
}

// Longest first, so that "..=" wins over ".." and "." and "->" over "-".
const PUNCTS: [&str; 19] = [
    "...", "..=", "..", "->", "=>", "(", ")", "[", "]", "{", "}", ",", ":", ".", "=", "?", "@",
    "|", "+",
];

struct Cursor<'a> {
    src: &'a str,
    pos: usize,
    line: u32,
    col: u32,
}

impl Cursor<'_> {
    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn peek2(&self) -> Option<char> {
        self.src[self.pos..].chars().nth(1)
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

    fn eat_while(&mut self, f: impl Fn(char) -> bool) {
        while self.peek().is_some_and(&f) {
            self.bump();
        }
    }
}

fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

pub fn lex(src: &str) -> Result<Lexed, LexError> {
    let mut cur = Cursor {
        src,
        pos: 0,
        line: 1,
        col: 1,
    };
    let mut header = Vec::new();
    let mut tokens: Vec<Token> = Vec::new();
    loop {
        cur.eat_while(char::is_whitespace);
        let (start, line, col) = (cur.pos, cur.line, cur.col);
        let span_to = |cur: &Cursor| Span {
            line,
            col,
            end_line: cur.line,
            end_col: cur.col,
        };
        let Some(c) = cur.peek() else {
            tokens.push(Token {
                tok: Tok::Eof,
                span: span_to(&cur),
                start,
                end: start,
            });
            return Ok(Lexed { header, tokens });
        };
        let err = |cur: &Cursor, code: &'static str, message: String| LexError {
            code,
            span: span_to(cur),
            start,
            end: cur.pos.max(start + c.len_utf8()),
            message,
        };
        let tok = if c == '-' && cur.peek2() == Some('-') {
            cur.eat_while(|c| c != '\n');
            if tokens.is_empty() {
                header.push(src[start..cur.pos].trim_end().to_owned());
            }
            continue;
        } else if is_name_start(c) {
            cur.eat_while(is_name_char);
            Tok::Name(src[start..cur.pos].to_owned())
        } else if c.is_ascii_digit()
            || (c == '-' && cur.peek2().is_some_and(|d| d.is_ascii_digit()))
        {
            number(&mut cur)
        } else if c == '"' {
            cur.bump();
            let mut s = String::new();
            loop {
                match cur.bump() {
                    None | Some('\n') => return Err(err(&cur, "E102", "unclosed string".into())),
                    Some('"') => break,
                    Some('\\') => s.push(escape(&mut cur).map_err(|m| err(&cur, "E100", m))?),
                    Some(ch) => s.push(ch),
                }
            }
            Tok::Str(s)
        } else if c == '\'' {
            cur.bump();
            match (cur.peek(), cur.peek2()) {
                (Some('\\'), _) => {
                    cur.bump();
                    let ch = escape(&mut cur).map_err(|m| err(&cur, "E100", m))?;
                    if cur.bump() != Some('\'') {
                        return Err(err(&cur, "E102", "unclosed character literal".into()));
                    }
                    Tok::Char(ch)
                }
                (Some(ch), Some('\'')) => {
                    cur.bump();
                    cur.bump();
                    Tok::Char(ch)
                }
                (Some(ch), _) if is_name_start(ch) => {
                    let s = cur.pos;
                    cur.eat_while(is_name_char);
                    Tok::Label(src[s..cur.pos].to_owned())
                }
                _ => {
                    return Err(err(
                        &cur,
                        "E100",
                        "expected a label or a character literal".into(),
                    ));
                }
            }
        } else if let Some(p) = PUNCTS.iter().find(|p| src[start..].starts_with(**p)) {
            for _ in 0..p.len() {
                cur.bump();
            }
            Tok::Punct(p)
        } else {
            cur.bump();
            return Err(err(&cur, "E100", format!("unexpected character `{c}`")));
        };
        tokens.push(Token {
            tok,
            span: span_to(&cur),
            start,
            end: cur.pos,
        });
    }
}

fn number(cur: &mut Cursor) -> Tok {
    let start = cur.pos;
    if cur.peek() == Some('-') {
        cur.bump();
    }
    if cur.peek() == Some('0') && matches!(cur.peek2(), Some('x' | 'o' | 'b')) {
        cur.bump();
        cur.bump();
        cur.eat_while(|c| c.is_ascii_hexdigit() || c == '_');
        return Tok::Int(cur.src[start..cur.pos].to_owned());
    }
    let digits = |c: char| c.is_ascii_digit() || c == '_';
    cur.eat_while(digits);
    let mut float = false;
    if cur.peek() == Some('.') && cur.peek2().is_some_and(|c| c.is_ascii_digit()) {
        float = true;
        cur.bump();
        cur.eat_while(digits);
    }
    if matches!(cur.peek(), Some('e' | 'E')) {
        let rest = &cur.src[cur.pos + 1..];
        let signed = rest.starts_with(['+', '-']);
        if rest[usize::from(signed)..].starts_with(|c: char| c.is_ascii_digit()) {
            float = true;
            cur.bump();
            if signed {
                cur.bump();
            }
            cur.eat_while(digits);
        }
    }
    let text = cur.src[start..cur.pos].to_owned();
    if float {
        Tok::Float(text)
    } else {
        Tok::Int(text)
    }
}

fn escape(cur: &mut Cursor) -> Result<char, String> {
    Ok(match cur.bump() {
        Some('n') => '\n',
        Some('r') => '\r',
        Some('t') => '\t',
        Some('0') => '\0',
        Some(c @ ('\\' | '"' | '\'')) => c,
        Some('u') if cur.peek() == Some('{') => {
            cur.bump();
            let s = cur.pos;
            cur.eat_while(|c| c.is_ascii_hexdigit());
            let hex = &cur.src[s..cur.pos];
            if cur.bump() != Some('}') {
                return Err("expected `}` after a \\u escape".into());
            }
            u32::from_str_radix(hex, 16)
                .ok()
                .and_then(char::from_u32)
                .ok_or_else(|| format!("`\\u{{{hex}}}` is not a Unicode scalar value"))?
        }
        Some(c) => return Err(format!("unknown escape `\\{c}`")),
        None => return Err("unclosed escape".into()),
    })
}

#[cfg(test)]
mod selftest {
    use super::*;

    fn toks(src: &str) -> Vec<Tok> {
        lex(src)
            .unwrap()
            .tokens
            .into_iter()
            .map(|t| t.tok)
            .collect()
    }

    #[test]
    fn punctuation_is_longest_match() {
        let p = |s| Tok::Punct(s);
        assert_eq!(
            toks("1..=2 -> =>"),
            [
                Tok::Int("1".into()),
                p("..="),
                Tok::Int("2".into()),
                p("->"),
                p("=>"),
                Tok::Eof
            ]
        );
    }

    #[test]
    fn numbers() {
        assert_eq!(
            toks("-5 0xff_ff 1.5e-3 2e8 x.0")[..4],
            [
                Tok::Int("-5".into()),
                Tok::Int("0xff_ff".into()),
                Tok::Float("1.5e-3".into()),
                Tok::Float("2e8".into()),
            ]
        );
    }

    #[test]
    fn chars_labels_and_strings() {
        assert_eq!(
            toks(r#"'a' '\n' 'outer "a\"\u{1F600}""#)[..4],
            [
                Tok::Char('a'),
                Tok::Char('\n'),
                Tok::Label("outer".into()),
                Tok::Str("a\"\u{1F600}".into()),
            ]
        );
    }

    #[test]
    fn header_comments_are_kept() {
        let l = lex("-- expect: E100 at 2:1\n--\nmodule m -- trailing\n").unwrap();
        assert_eq!(l.header, ["-- expect: E100 at 2:1", "--"]);
        assert_eq!(l.tokens.len(), 3);
    }

    #[test]
    fn unclosed_string() {
        let e = lex("\"abc").err().unwrap();
        assert_eq!((e.code, e.span.line, e.span.col), ("E102", 1, 1));
    }

    #[test]
    fn spans_are_one_based_bytes() {
        let t = &lex("module\n  ab").unwrap().tokens[1];
        assert_eq!((t.span.line, t.span.col, t.span.end_col), (2, 3, 5));
    }
}
