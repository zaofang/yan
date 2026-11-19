//! Prints the Yan Core token stream of each file named on the command line, in the format that
//! `selfhost/s0/core_lex.yn` prints, so the two lexers can be compared byte for byte (#1505).

use yan_core::lexer::{Tok, lex};

fn esc(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            c => out.push(c),
        }
    }
    out
}

fn main() {
    for path in std::env::args().skip(1) {
        let src = std::fs::read_to_string(&path).expect("readable file");
        println!("file {path}");
        match lex(&src) {
            Err(e) => println!(
                "error {} {}:{}-{}:{} {}..{} {}",
                e.code,
                e.span.line,
                e.span.col,
                e.span.end_line,
                e.span.end_col,
                e.start,
                e.end,
                e.message
            ),
            Ok(l) => {
                for h in &l.header {
                    println!("header {}", esc(h));
                }
                for t in &l.tokens {
                    let (kind, text) = match &t.tok {
                        Tok::Name(s) => ("name", s.clone()),
                        Tok::Int(s) => ("int", s.clone()),
                        Tok::Float(s) => ("float", s.clone()),
                        Tok::Str(s) => ("str", s.clone()),
                        Tok::Char(c) => ("char", c.to_string()),
                        Tok::Label(s) => ("label", s.clone()),
                        Tok::Punct(s) => ("punct", (*s).to_owned()),
                        Tok::Eof => ("eof", String::new()),
                    };
                    println!(
                        "{kind} \"{}\" {}:{}-{}:{} {}..{}",
                        esc(&text),
                        t.span.line,
                        t.span.col,
                        t.span.end_line,
                        t.span.end_col,
                        t.start,
                        t.end
                    );
                }
            }
        }
    }
}
