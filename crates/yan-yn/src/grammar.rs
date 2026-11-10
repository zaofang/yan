//! The Yan grammar (15.2): its EBNF, token classes and lexical rules for `yan-ebnf`.

use crate::lexer::{StrKind, Tok, lex};
use yan_ebnf::{Grammar, Surface, Term, Tk};

pub fn grammar() -> Grammar {
    Grammar::of(&SURFACE)
}

static SURFACE: Surface = Surface {
    title: "Yan",
    cmd: "yn",
    path: "crates/yan-yn/grammar/yn.ebnf",
    ebnf: include_str!("../grammar/yn.ebnf"),
    notes: "Indentation is unrolled to eight levels. Not expressed: reserved words excluded from\n`name`, comments, lines broken inside brackets, interpolations, the sign and `(` that a grid\nreads by their spacing, and the checks the parser makes after parsing. Soft keywords are\nordinary names.",
    lexical: LEXICAL,
    lark_lexical: LARK_LEXICAL,
    tokens,
};

fn tokens(src: &str) -> Option<Vec<Tk>> {
    let (lexed, diags) = lex("t.yn", src);
    if diags
        .iter()
        .any(|d| d.severity == yan_diag::Severity::Error)
    {
        return None;
    }
    Some(
        lexed
            .tokens
            .iter()
            .map(|t| &t.tok)
            .take_while(|t| !matches!(t, Tok::Eof))
            .map(classify)
            .collect(),
    )
}

fn classify(tok: &Tok) -> Tk {
    let (lit, terms) = match tok {
        Tok::Name(n) => (Some(n.clone()), vec![Term::Name]),
        Tok::TypeName(n) => (Some(n.clone()), vec![Term::TypeName]),
        Tok::Kw(k) => (Some((*k).to_owned()), vec![]),
        Tok::Punct(p) => (Some((*p).to_owned()), vec![]),
        Tok::Int(_) => (None, vec![Term::Int]),
        Tok::Float(_) => (None, vec![Term::Float]),
        Tok::UnitNum { .. } => (None, vec![Term::Unit]),
        Tok::Temporal { kind, .. } => (
            None,
            vec![match kind {
                crate::lexer::TemporalKind::Date => Term::Date,
                crate::lexer::TemporalKind::Time => Term::Time,
                crate::lexer::TemporalKind::DateTime | crate::lexer::TemporalKind::Instant => {
                    Term::Instant
                }
                crate::lexer::TemporalKind::Zoned => Term::Zoned,
            }],
        ),
        Tok::Str(s) => (
            None,
            vec![match s.kind {
                StrKind::Bytes => Term::Bytes,
                StrKind::Template => Term::TStr,
                _ => Term::Str,
            }],
        ),
        Tok::Char(_) => (None, vec![Term::Char]),
        Tok::Sign(_) => (None, vec![Term::Sign]),
        Tok::SplParen => (None, vec![Term::SplParen]),
        Tok::Newline => (None, vec![Term::Newline]),
        Tok::Indent => (None, vec![Term::Indent]),
        Tok::Dedent => (None, vec![Term::Dedent]),
        Tok::Eof => (None, vec![]),
    };
    Tk { lit, terms }
}

const LEXICAL: &str = r#"
ws ::= " "*
sp ::= " "+
nl ::= "\n"
name ::= [a-z] [a-z0-9]* ("_" [a-z0-9]+)*
typename ::= [A-Z] [a-z0-9]* ([A-Z] [a-z0-9]*)*
digits ::= [0-9] ("_"? [0-9])*
int ::= "0x" [0-9a-fA-F_]+ | "0o" [0-7_]+ | "0b" [01_]+ | digits
float ::= digits "." digits? exp? | digits exp | "." digits exp?
exp ::= [eE] [+-]? digits
unit ::= (float | int) [a-zA-Z] [a-zA-Z0-9]*
date ::= [0-9] [0-9] [0-9] [0-9] "-" [0-9] [0-9] "-" [0-9] [0-9]
time ::= [0-9] [0-9] ":" [0-9] [0-9] (":" [0-9] [0-9] ("." [0-9]+)?)?
offset ::= "Z" | [+-] [0-9] [0-9] ":" [0-9] [0-9]
instant ::= date "T" time offset
zoned ::= date "T" time offset? "[" [A-Za-z0-9_/+-]+ "]"
sign ::= "-" | "+"
splparen ::= "("
strbody ::= ([^"\\\n] | "\\" [^\n])*
string ::= "r"? "\"" strbody "\"" | "c\"" strbody "\"" | "r"? "\"\"\"" tqbody "\"\"\""
tqbody ::= ([^"\\] | "\\" . | "\"" [^"] | "\"\"" [^"])*
bytes ::= "b\"" strbody "\""
char ::= "c'" ([^'\\\n] | "\\" [^\n]) "'"
fbody ::= ([^"\\{}\n] | "\\" [^\n] | "{{" | "}}" | "{" [^{}\n]+ "}")*
tstring ::= "t\"" fbody "\""
"#;

const LARK_LEXICAL: &str = r#"
ws: /[ ]*/
sp: /[ ]+/
nl: "\n"
name: /[a-z][a-z0-9]*(_[a-z0-9]+)*/
typename: /[A-Z][a-z0-9]*([A-Z][a-z0-9]*)*/
digits: /[0-9](_?[0-9])*/
int: /0x[0-9a-fA-F_]+|0o[0-7_]+|0b[01_]+/ | digits
float: digits "." digits? exp? | digits exp | "." digits exp?
exp: /[eE][+-]?/ digits
unit: (float | int) /[a-zA-Z][a-zA-Z0-9]*/
date: /[0-9]{4}-[0-9]{2}-[0-9]{2}/
time: /[0-9]{2}:[0-9]{2}(:[0-9]{2}(\.[0-9]+)?)?/
offset: "Z" | /[+-][0-9]{2}:[0-9]{2}/
instant: date "T" time offset
zoned: date "T" time offset? "[" /[A-Za-z0-9_\/+-]+/ "]"
sign: "-" | "+"
splparen: "("
strbody: /([^"\\\n]|\\[^\n])*/
string: "r"? "\"" strbody "\"" | "c\"" strbody "\"" | "r"? "\"\"\"" tqbody "\"\"\""
tqbody: /([^"\\]|\\(?s:.)|"[^"]|""[^"])*/
bytes: "b\"" strbody "\""
char: "c'" /[^'\\\n]|\\[^\n]/ "'"
fbody: /([^"\\{}\n]|\\[^\n]|\{\{|\}\}|\{[^{}\n]+\})*/
tstring: "t\"" fbody "\""
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn files(dir: &str, ext: &str) -> Vec<PathBuf> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../conformance")
            .join(dir);
        let mut out = Vec::new();
        let mut stack = vec![root];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == ext) {
                    out.push(p);
                }
            }
        }
        out.sort();
        out
    }

    fn parses(src: &str) -> bool {
        crate::parse("t.yn", src)
            .1
            .iter()
            .all(|d| d.severity != yan_diag::Severity::Error)
    }

    /// Rejected only by 13.2's "mixes that require parentheses" (E101), which the grammar does
    /// not encode: `x else e -1` is a grammatical fallback whose right side needs parentheses.
    fn only_mixes(src: &str) -> bool {
        let errors: Vec<_> = crate::parse("t.yn", src)
            .1
            .into_iter()
            .filter(|d| d.severity == yan_diag::Severity::Error)
            .collect();
        !errors.is_empty() && errors.iter().all(|d| d.code == "E101")
    }

    fn corpus() -> Vec<(PathBuf, String)> {
        ["yn/pass", "yn/run", "golden"]
            .iter()
            .flat_map(|d| files(d, "yn"))
            .map(|p| {
                let s = std::fs::read_to_string(&p).unwrap();
                (p, s)
            })
            .filter(|(_, s)| parses(s))
            .collect()
    }

    #[test]
    fn recognizer_accepts_what_the_parser_accepts() {
        let g = grammar();
        let corpus = corpus();
        assert!(corpus.len() > 20, "{} files", corpus.len());
        let rejected: Vec<_> = corpus
            .iter()
            .filter_map(|(p, s)| {
                let at = g.check_source(s).err()?;
                let toks = lex("t.yn", s).0.tokens;
                Some(format!(
                    "{} token {at}: {:?}",
                    p.display(),
                    toks.get(at).map(|t| (&t.tok, t.span.line))
                ))
            })
            .collect();
        assert!(rejected.is_empty(), "{rejected:#?}");
    }

    /// Token deleted, adjacent tokens swapped, or one line's indent broken, at deterministic places.
    fn mutants(src: &str, seed: usize) -> Vec<String> {
        let lines: Vec<&str> = src.lines().collect();
        let mut out = Vec::new();
        let code: Vec<usize> = (0..lines.len())
            .filter(|&i| {
                let t = lines[i].trim_start();
                // Interpolations are parsed by the parser, not by this grammar's single FSTRING token.
                !t.is_empty()
                    && !t.starts_with('#')
                    && !t.starts_with('"')
                    && !t.contains("f\"")
                    && !t.contains("t\"")
            })
            .collect();
        if code.is_empty() {
            return out;
        }
        let li = code[seed % code.len()];
        let words: Vec<&str> = lines[li].split_whitespace().collect();
        let join = |l: &str| {
            let mut v = lines.clone();
            let owned = l.to_owned();
            v[li] = &owned;
            v.join("\n") + "\n"
        };
        if words.len() > 1 {
            let k = seed % words.len();
            let mut w = words.clone();
            w.remove(k);
            out.push(join(&format!(
                "{}{}",
                &lines[li][..lines[li].len() - lines[li].trim_start().len()],
                w.join(" ")
            )));
            let k = seed % (words.len() - 1);
            let mut w = words.clone();
            w.swap(k, k + 1);
            out.push(join(&format!(
                "{}{}",
                &lines[li][..lines[li].len() - lines[li].trim_start().len()],
                w.join(" ")
            )));
        }
        out.push(join(&format!("  {}", lines[li])));
        out
    }

    #[test]
    fn recognizer_rejects_mutants_the_parser_rejects() {
        let g = grammar();
        let (mut rejected, mut disagree) = (0, Vec::new());
        for (n, (_, src)) in corpus().iter().enumerate() {
            for m in mutants(src, n * 7 + 3) {
                if parses(&m) || only_mixes(&m) {
                    continue;
                }
                if g.accepts(&m) {
                    disagree.push(m);
                } else {
                    rejected += 1;
                }
            }
        }
        assert!(rejected >= 20, "{rejected} rejected");
        assert!(
            disagree.is_empty(),
            "{} accepted:\n{}",
            disagree.len(),
            disagree.join("\n---\n")
        );
    }

    #[test]
    fn structure() {
        use yan_ebnf::checks::*;
        let g = grammar();
        no_left_recursion(&g);
        gbnf_is_closed_and_reachable(&g);
        llguidance_is_closed_and_reachable(&g);
        xgrammar_differs_from_gbnf_only_in_header_and_dot(&g);
        output_is_byte_stable(&g, &grammar());
    }
}
