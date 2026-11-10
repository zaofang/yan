//! The Yan Agent grammar (13.2): its EBNF, token classes and lexical rules for `yan-ebnf`.

use crate::lexer::{Tok, lex};
use crate::parser::AUGOPS;
use yan_ebnf::{Grammar, Surface, Term, Tk};

pub fn grammar() -> Grammar {
    Grammar::of(&SURFACE)
}

static SURFACE: Surface = Surface {
    title: "Yan Agent",
    cmd: "yag",
    path: "crates/yan-yag/grammar/yag.ebnf",
    ebnf: include_str!("../grammar/yag.ebnf"),
    notes: "Indentation is unrolled to eight levels. Not expressed: reserved words excluded from\n`name`, comments, and lines broken inside brackets. Soft keywords are ordinary names.",
    lexical: LEXICAL,
    lark_lexical: LARK_LEXICAL,
    tokens,
};

const RESERVED: &[&str] = &[
    "def",
    "class",
    "return",
    "if",
    "elif",
    "else",
    "for",
    "while",
    "in",
    "not",
    "and",
    "or",
    "is",
    "break",
    "continue",
    "pass",
    "lambda",
    "assert",
    "True",
    "False",
    "None",
    "as",
    "from",
    "with",
    "pub",
    "par",
    "fail",
    "become",
    "unreachable",
    "try",
    "except",
    "finally",
    "raise",
    "global",
    "nonlocal",
    "del",
    "yield",
    "async",
    "await",
];

fn is_name(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_lowercase()) && !RESERVED.contains(&s)
}

fn is_typename(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_uppercase()) && !matches!(s, "True" | "False" | "None")
}

/// Marks the contract words that 13.1 makes keywords: those opening a line of a body's contract region
/// (the lines before its first other statement) unless `=` or an augmented assignment follows (R462).
/// A context-free rule cannot say that a name is excluded there, so the recognizer does.
fn contract_keywords(toks: &[&Tok]) -> Vec<bool> {
    let mut out = vec![false; toks.len()];
    let (mut head, mut last_head): (Option<&str>, Option<&str>) = (None, None);
    let mut region: &[&str] = &[];
    let mut bol = true;
    for (i, t) in toks.iter().enumerate() {
        match t {
            Tok::Newline => {
                last_head = head.take();
                bol = true;
                continue;
            }
            Tok::Indent => {
                region = match last_head {
                    Some("def" | "lemma") => &["pre", "post", "ex", "decreases", "cost"],
                    Some("while" | "for") => &["invariant", "decreases"],
                    _ => &[],
                };
                continue;
            }
            Tok::Dedent => {
                region = &[];
                bol = true;
                continue;
            }
            _ => {}
        }
        if head.is_none() && !matches!(t, Tok::Name(n) if n == "pub") {
            head = Some(match t {
                Tok::Name(n) => n,
                _ => "",
            });
        }
        if bol && !region.is_empty() {
            let word = match t {
                Tok::Name(n) => Some(n.as_str()),
                _ => None,
            };
            let assigned =
                matches!(toks.get(i + 1), Some(Tok::Op(o)) if *o == "=" || AUGOPS.contains(o));
            match word {
                Some(w) if region.contains(&w) && !assigned => out[i] = true,
                _ if matches!(t, Tok::Str { .. })
                    && matches!(toks.get(i + 1), Some(Tok::Newline)) => {}
                _ => region = &[],
            }
        }
        bol = false;
    }
    out
}

fn tokens(src: &str) -> Option<Vec<Tk>> {
    let lexed = lex(src);
    if lexed.fatal {
        return None;
    }
    let toks: Vec<&Tok> = lexed
        .tokens
        .iter()
        .map(|t| &t.tok)
        .take_while(|t| !matches!(t, Tok::Eof))
        .collect();
    let keyword = contract_keywords(&toks);
    Some(
        toks.iter()
            .zip(keyword)
            .map(|(t, kw)| classify(t, kw))
            .collect(),
    )
}

fn classify(tok: &Tok, keyword: bool) -> Tk {
    let (lit, terms) = match tok {
        Tok::Op(o) => (Some((*o).to_owned()), vec![]),
        Tok::Name(n) => {
            let mut terms = vec![];
            if is_name(n) && !keyword {
                terms.push(Term::Name);
            }
            if is_typename(n) {
                terms.push(Term::TypeName);
            }
            (Some(n.clone()), terms)
        }
        Tok::Int(_) => (None, vec![Term::Int]),
        Tok::Float(_) => (None, vec![Term::Float]),
        Tok::Str { prefix, .. } => (
            None,
            vec![if prefix == "b" {
                Term::Bytes
            } else {
                Term::Str
            }],
        ),
        Tok::Char(_) => (None, vec![Term::Char]),
        Tok::FStr { template, .. } => (None, vec![if *template { Term::TStr } else { Term::FStr }]),
        Tok::Newline => (None, vec![Term::Newline]),
        Tok::Indent => (None, vec![Term::Indent]),
        Tok::Dedent => (None, vec![Term::Dedent]),
        _ => (None, vec![]),
    };
    Tk { lit, terms }
}

const LEXICAL: &str = r#"
ws ::= " "*
sp ::= " "+
nl ::= "\n"
name ::= [a-z] [a-z0-9]* ("_" [a-z0-9]+)* | "_"
typename ::= [A-Z] [a-z0-9]* ([A-Z] [a-z0-9]*)*
digits ::= [0-9] ("_"? [0-9])*
int ::= "0x" [0-9a-fA-F_]+ | "0o" [0-7_]+ | "0b" [01_]+ | digits
float ::= digits "." digits? exp? | digits exp | "." digits exp?
exp ::= [eE] [+-]? digits
strbody ::= ([^"\\\n] | "\\" [^\n])*
string ::= "r"? "\"" strbody "\"" | "c\"" strbody "\"" | "r"? "\"\"\"" tqbody "\"\"\""
tqbody ::= ([^"\\] | "\\" . | "\"" [^"] | "\"\"" [^"])*
bytes ::= "b\"" strbody "\""
char ::= "c'" ([^'\\\n] | "\\" [^\n]) "'"
fbody ::= ([^"\\{}\n] | "\\" [^\n] | "{{" | "}}" | "{" [^{}\n]+ "}")*
fstring ::= "f\"" fbody "\""
tstring ::= "t\"" fbody "\""
"#;

const LARK_LEXICAL: &str = r#"
ws: /[ ]*/
sp: /[ ]+/
nl: "\n"
name: /[a-z][a-z0-9]*(_[a-z0-9]+)*|_/
typename: /[A-Z][a-z0-9]*([A-Z][a-z0-9]*)*/
digits: /[0-9](_?[0-9])*/
int: /0x[0-9a-fA-F_]+|0o[0-7_]+|0b[01_]+/ | digits
float: digits "." digits? exp? | digits exp | "." digits exp?
exp: /[eE][+-]?/ digits
strbody: /([^"\\\n]|\\[^\n])*/
string: "r"? "\"" strbody "\"" | "c\"" strbody "\"" | "r"? "\"\"\"" tqbody "\"\"\""
tqbody: /([^"\\]|\\(?s:.)|"[^"]|""[^"])*/
bytes: "b\"" strbody "\""
char: "c'" /[^'\\\n]|\\[^\n]/ "'"
fbody: /([^"\\{}\n]|\\[^\n]|\{\{|\}\}|\{[^{}\n]+\})*/
fstring: "f\"" fbody "\""
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
        crate::parse("t.yag", src, true)
            .1
            .iter()
            .all(|d| d.severity != yan_diag::Severity::Error)
    }

    /// Rejected only by 13.2's "mixes that require parentheses" (E101), which the grammar does
    /// not encode: `x else e -1` is a grammatical fallback whose right side needs parentheses.
    fn only_mixes(src: &str) -> bool {
        let errors: Vec<_> = crate::parse("t.yag", src, true)
            .1
            .into_iter()
            .filter(|d| d.severity == yan_diag::Severity::Error)
            .collect();
        !errors.is_empty() && errors.iter().all(|d| d.code == "E101")
    }

    fn corpus() -> Vec<(PathBuf, String)> {
        ["yag/pass", "yag/run", "golden"]
            .iter()
            .flat_map(|d| files(d, "yag"))
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
                let toks = lex(s).tokens;
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
