//! The Yan Agent grammar (13.2) as data: an EBNF reader, a recognizer over lexer tokens for tests,
//! and the GBNF emitter for constrained decoding (21.3). Other formats build on `Grammar`.

use crate::lexer::{Tok, lex};
use std::collections::{HashMap, HashSet};

const SOURCE: &str = include_str!("../grammar/yag.ebnf");

/// Indentation levels the text grammars unroll; deeper nesting is not generatable.
const MAX_DEPTH: usize = 8;

/// Words the lexer cannot offer as a NAME (13.1). Soft keywords are not listed.
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Term {
    Name,
    TypeName,
    Int,
    Float,
    Str,
    Bytes,
    Char,
    FStr,
    TStr,
    Newline,
    Indent,
    Dedent,
    /// Start of a physical line; carries the indentation in text grammars.
    Bol,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Lit(String),
    Term(Term),
    Ref(String),
    Seq(Vec<Expr>),
    Alt(Vec<Expr>),
    Opt(Box<Expr>),
    Rep(Box<Expr>),
}

#[derive(Clone, Debug)]
pub struct Grammar {
    /// In source order; the first rule is the start.
    pub rules: Vec<(String, Expr)>,
}

impl Grammar {
    pub fn yag() -> Grammar {
        Grammar::parse(SOURCE).expect("embedded grammar")
    }

    pub fn parse(src: &str) -> Result<Grammar, String> {
        let toks = tokenize(src)?;
        let mut p = Reader { toks, i: 0 };
        let mut rules = Vec::new();
        while p.i < p.toks.len() {
            let name = match p.next() {
                Some(Gt::Id(n)) if n.chars().all(|c| c.is_ascii_lowercase()) => n,
                t => return Err(format!("rule name expected, found {t:?}")),
            };
            p.expect(Gt::Eq)?;
            let e = p.alts()?;
            p.expect(Gt::Semi)?;
            rules.push((name, e));
        }
        let g = Grammar { rules };
        g.check()?;
        Ok(g)
    }

    fn check(&self) -> Result<(), String> {
        let names: HashSet<&str> = self.rules.iter().map(|(n, _)| n.as_str()).collect();
        if names.len() != self.rules.len() {
            return Err("duplicate rule".into());
        }
        fn walk(e: &Expr, names: &HashSet<&str>) -> Result<(), String> {
            match e {
                Expr::Ref(r) if !names.contains(r.as_str()) => Err(format!("undefined rule {r}")),
                Expr::Seq(v) | Expr::Alt(v) => v.iter().try_for_each(|e| walk(e, names)),
                Expr::Opt(e) | Expr::Rep(e) => walk(e, names),
                _ => Ok(()),
            }
        }
        self.rules.iter().try_for_each(|(_, e)| walk(e, &names))
    }

    /// Whether `src` is a Yan Agent module of the grammar, judged on the lexer's tokens.
    pub fn accepts(&self, src: &str) -> bool {
        self.check_source(src).is_ok()
    }

    /// Like `accepts`; on rejection, the index of the first token no derivation can continue with.
    pub fn check_source(&self, src: &str) -> Result<(), usize> {
        Recognizer::new(self).check(src)
    }

    /// GBNF for llama.cpp, with indentation unrolled to `MAX_DEPTH` levels.
    pub fn gbnf(&self) -> String {
        Gbnf::new(self).emit()
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Gt {
    Id(String),
    Lit(String),
    Eq,
    Semi,
    Bar,
    Open(char),
    Close(char),
}

fn tokenize(src: &str) -> Result<Vec<Gt>, String> {
    let cs: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < cs.len() {
        let c = cs[i];
        match c {
            c if c.is_whitespace() => i += 1,
            '#' => {
                while i < cs.len() && cs[i] != '\n' {
                    i += 1;
                }
            }
            '"' => {
                let start = i + 1;
                i = start;
                while i < cs.len() && cs[i] != '"' {
                    i += 1;
                }
                if i >= cs.len() {
                    return Err("unterminated literal".into());
                }
                out.push(Gt::Lit(cs[start..i].iter().collect()));
                i += 1;
            }
            '=' => {
                out.push(Gt::Eq);
                i += 1;
            }
            ';' => {
                out.push(Gt::Semi);
                i += 1;
            }
            '|' => {
                out.push(Gt::Bar);
                i += 1;
            }
            '(' | '[' | '{' => {
                out.push(Gt::Open(c));
                i += 1;
            }
            ')' | ']' | '}' => {
                out.push(Gt::Close(c));
                i += 1;
            }
            c if c.is_ascii_alphabetic() => {
                let start = i;
                while i < cs.len() && cs[i].is_ascii_alphanumeric() {
                    i += 1;
                }
                out.push(Gt::Id(cs[start..i].iter().collect()));
            }
            c => return Err(format!("unexpected {c:?} in grammar")),
        }
    }
    Ok(out)
}

struct Reader {
    toks: Vec<Gt>,
    i: usize,
}

impl Reader {
    fn next(&mut self) -> Option<Gt> {
        let t = self.toks.get(self.i).cloned();
        self.i += 1;
        t
    }

    fn peek(&self) -> Option<&Gt> {
        self.toks.get(self.i)
    }

    fn expect(&mut self, t: Gt) -> Result<(), String> {
        match self.next() {
            Some(x) if x == t => Ok(()),
            x => Err(format!("expected {t:?}, found {x:?}")),
        }
    }

    fn alts(&mut self) -> Result<Expr, String> {
        let mut alts = vec![self.seq()?];
        while self.peek() == Some(&Gt::Bar) {
            self.i += 1;
            alts.push(self.seq()?);
        }
        Ok(if alts.len() == 1 {
            alts.remove(0)
        } else {
            Expr::Alt(alts)
        })
    }

    fn seq(&mut self) -> Result<Expr, String> {
        let mut items = Vec::new();
        while let Some(t) = self.peek() {
            if matches!(t, Gt::Bar | Gt::Semi | Gt::Close(_)) {
                break;
            }
            items.push(self.atom()?);
        }
        if items.is_empty() {
            return Err("empty alternative".into());
        }
        Ok(if items.len() == 1 {
            items.remove(0)
        } else {
            Expr::Seq(items)
        })
    }

    fn atom(&mut self) -> Result<Expr, String> {
        match self.next() {
            Some(Gt::Lit(s)) => Ok(Expr::Lit(s)),
            Some(Gt::Id(n)) => Ok(match n.as_str() {
                "NAME" => Expr::Term(Term::Name),
                "TYPENAME" => Expr::Term(Term::TypeName),
                "INT" => Expr::Term(Term::Int),
                "FLOAT" => Expr::Term(Term::Float),
                "STRING" => Expr::Term(Term::Str),
                "BYTES" => Expr::Term(Term::Bytes),
                "CHAR" => Expr::Term(Term::Char),
                "FSTRING" => Expr::Term(Term::FStr),
                "TSTRING" => Expr::Term(Term::TStr),
                "NEWLINE" => Expr::Term(Term::Newline),
                "INDENT" => Expr::Term(Term::Indent),
                "DEDENT" => Expr::Term(Term::Dedent),
                "BOL" => Expr::Term(Term::Bol),
                _ if n.chars().any(|c| c.is_ascii_uppercase()) => {
                    return Err(format!("unknown terminal {n}"));
                }
                _ => Expr::Ref(n),
            }),
            Some(Gt::Open(o)) => {
                let e = self.alts()?;
                let want = match o {
                    '(' => ')',
                    '[' => ']',
                    _ => '}',
                };
                self.expect(Gt::Close(want))?;
                Ok(match o {
                    '(' => e,
                    '[' => Expr::Opt(Box::new(e)),
                    _ => Expr::Rep(Box::new(e)),
                })
            }
            t => Err(format!("unexpected {t:?}")),
        }
    }
}

// ---- Recognizer: Earley over the lexer's tokens, so any context-free grammar works. ----

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Sym {
    N(usize),
    Lit(String),
    T(Term),
}

struct Prod {
    lhs: usize,
    rhs: Vec<Sym>,
}

struct Recognizer {
    prods: Vec<Prod>,
    by_lhs: Vec<Vec<usize>>,
    nullable: Vec<bool>,
}

impl Recognizer {
    fn new(g: &Grammar) -> Recognizer {
        let mut ids: HashMap<&str, usize> = HashMap::new();
        for (n, _) in &g.rules {
            ids.insert(n, ids.len());
        }
        let mut b = Builder {
            ids,
            prods: Vec::new(),
            n: g.rules.len(),
        };
        for (i, (_, e)) in g.rules.iter().enumerate() {
            let rhs = b.alts_of(e);
            for r in rhs {
                b.prods.push(Prod { lhs: i, rhs: r });
            }
        }
        let n = b.n;
        let mut by_lhs = vec![Vec::new(); n];
        for (i, p) in b.prods.iter().enumerate() {
            by_lhs[p.lhs].push(i);
        }
        let mut nullable = vec![false; n];
        loop {
            let mut changed = false;
            for p in &b.prods {
                if !nullable[p.lhs]
                    && p.rhs.iter().all(|s| match s {
                        Sym::N(x) => nullable[*x],
                        _ => false,
                    })
                {
                    nullable[p.lhs] = true;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        Recognizer {
            prods: b.prods,
            by_lhs,
            nullable,
        }
    }

    fn check(&self, src: &str) -> Result<(), usize> {
        let lexed = lex(src);
        if lexed.fatal {
            return Err(0);
        }
        let toks: Vec<&Tok> = lexed
            .tokens
            .iter()
            .map(|t| &t.tok)
            .take_while(|t| !matches!(t, Tok::Eof))
            .collect();
        type Item = (usize, usize, usize);
        let mut sets: Vec<Vec<Item>> = Vec::with_capacity(toks.len() + 1);
        let mut seen: Vec<HashSet<Item>> = Vec::new();
        let add = |sets: &mut Vec<Vec<Item>>, seen: &mut Vec<HashSet<Item>>, k: usize, it: Item| {
            if seen[k].insert(it) {
                sets[k].push(it);
            }
        };
        sets.push(Vec::new());
        seen.push(HashSet::new());
        for &p in &self.by_lhs[0] {
            add(&mut sets, &mut seen, 0, (p, 0, 0));
        }
        for k in 0..=toks.len() {
            let mut j = 0;
            while j < sets[k].len() {
                let (p, dot, origin) = sets[k][j];
                j += 1;
                match self.prods[p].rhs.get(dot) {
                    Some(Sym::N(x)) => {
                        for &q in &self.by_lhs[*x] {
                            add(&mut sets, &mut seen, k, (q, 0, k));
                        }
                        if self.nullable[*x] {
                            add(&mut sets, &mut seen, k, (p, dot + 1, origin));
                        }
                    }
                    Some(sym) => {
                        if let Some(t) = toks.get(k)
                            && matches(sym, t)
                        {
                            if sets.len() == k + 1 {
                                sets.push(Vec::new());
                                seen.push(HashSet::new());
                            }
                            add(&mut sets, &mut seen, k + 1, (p, dot + 1, origin));
                        }
                    }
                    None => {
                        let lhs = self.prods[p].lhs;
                        let mut m = 0;
                        while m < sets[origin].len() {
                            let (p2, d2, o2) = sets[origin][m];
                            m += 1;
                            if self.prods[p2].rhs.get(d2) == Some(&Sym::N(lhs)) {
                                add(&mut sets, &mut seen, k, (p2, d2 + 1, o2));
                            }
                        }
                    }
                }
            }
            if sets.len() == k + 1 && k < toks.len() {
                return Err(k);
            }
        }
        let done = sets[toks.len()].iter().any(|&(p, dot, origin)| {
            origin == 0 && self.prods[p].lhs == 0 && dot == self.prods[p].rhs.len()
        });
        if done { Ok(()) } else { Err(toks.len()) }
    }
}

struct Builder<'a> {
    ids: HashMap<&'a str, usize>,
    prods: Vec<Prod>,
    n: usize,
}

impl Builder<'_> {
    fn fresh(&mut self, alts: Vec<Vec<Sym>>) -> Sym {
        let id = self.n;
        self.n += 1;
        for rhs in alts {
            self.prods.push(Prod { lhs: id, rhs });
        }
        Sym::N(id)
    }

    fn alts_of(&mut self, e: &Expr) -> Vec<Vec<Sym>> {
        match e {
            Expr::Alt(v) => v.iter().map(|e| self.seq_of(e)).collect(),
            e => vec![self.seq_of(e)],
        }
    }

    fn seq_of(&mut self, e: &Expr) -> Vec<Sym> {
        match e {
            Expr::Seq(v) => v.iter().flat_map(|e| self.seq_of(e)).collect(),
            Expr::Lit(s) => vec![Sym::Lit(s.clone())],
            Expr::Term(Term::Bol) => vec![],
            Expr::Term(t) => vec![Sym::T(*t)],
            Expr::Ref(r) => vec![Sym::N(self.ids[r.as_str()])],
            Expr::Alt(_) => {
                let alts = self.alts_of(e);
                vec![self.fresh(alts)]
            }
            Expr::Opt(x) => {
                let mut alts = self.alts_of(x);
                alts.push(vec![]);
                vec![self.fresh(alts)]
            }
            Expr::Rep(x) => {
                let id = self.n;
                self.n += 1;
                let mut rec = vec![Sym::N(id)];
                rec.extend(self.seq_of(x));
                self.prods.push(Prod {
                    lhs: id,
                    rhs: vec![],
                });
                self.prods.push(Prod { lhs: id, rhs: rec });
                vec![Sym::N(id)]
            }
        }
    }
}

fn is_name(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_lowercase()) && !RESERVED.contains(&s)
}

fn is_typename(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_uppercase()) && !matches!(s, "True" | "False" | "None")
}

fn matches(sym: &Sym, tok: &Tok) -> bool {
    match (sym, tok) {
        (Sym::Lit(l), Tok::Op(o)) => l == o,
        (Sym::Lit(l), Tok::Name(n)) => l == n,
        (Sym::T(Term::Name), Tok::Name(n)) => is_name(n),
        (Sym::T(Term::TypeName), Tok::Name(n)) => is_typename(n),
        (Sym::T(Term::Int), Tok::Int(_)) => true,
        (Sym::T(Term::Float), Tok::Float(_)) => true,
        (Sym::T(Term::Str), Tok::Str { prefix, .. }) => prefix != "b",
        (Sym::T(Term::Bytes), Tok::Str { prefix, .. }) => prefix == "b",
        (Sym::T(Term::Char), Tok::Char(_)) => true,
        (Sym::T(Term::FStr), Tok::FStr { template, .. }) => !template,
        (Sym::T(Term::TStr), Tok::FStr { template, .. }) => *template,
        (Sym::T(Term::Newline), Tok::Newline) => true,
        (Sym::T(Term::Indent), Tok::Indent) => true,
        (Sym::T(Term::Dedent), Tok::Dedent) => true,
        _ => false,
    }
}

// ---- GBNF ----

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Edge {
    Word,
    Punct,
}

fn first_of<'a>(
    e: &'a Expr,
    known: &HashMap<&'a str, (HashSet<Edge>, bool)>,
) -> (HashSet<Edge>, bool) {
    let edge = |word| HashSet::from([if word { Edge::Word } else { Edge::Punct }]);
    match e {
        Expr::Lit(l) => (
            edge(l.starts_with(|c: char| c.is_ascii_alphabetic())),
            false,
        ),
        Expr::Term(t) => match t {
            Term::Name | Term::TypeName | Term::Int | Term::Float => (edge(true), false),
            Term::Bol | Term::Indent | Term::Dedent => (HashSet::new(), true),
            _ => (edge(false), false),
        },
        Expr::Ref(r) => known.get(r.as_str()).cloned().unwrap_or_default(),
        Expr::Seq(v) => {
            let mut set = HashSet::new();
            for e in v {
                let (f, n) = first_of(e, known);
                set.extend(f);
                if !n {
                    return (set, false);
                }
            }
            (set, true)
        }
        Expr::Alt(v) => {
            let mut set = HashSet::new();
            let mut null = false;
            for e in v {
                let (f, n) = first_of(e, known);
                set.extend(f);
                null |= n;
            }
            (set, null)
        }
        Expr::Opt(e) | Expr::Rep(e) => (first_of(e, known).0, true),
    }
}

struct Gbnf<'a> {
    g: &'a Grammar,
    layout: HashSet<&'a str>,
    /// First-token classes and nullability per rule.
    first: HashMap<&'a str, (HashSet<Edge>, bool)>,
}

impl<'a> Gbnf<'a> {
    fn new(g: &'a Grammar) -> Self {
        let mut s = Gbnf {
            g,
            layout: HashSet::new(),
            first: HashMap::new(),
        };
        s.layout_rules();
        s.first_sets();
        s
    }

    fn layout_rules(&mut self) {
        fn direct(e: &Expr, layout: &HashSet<&str>) -> bool {
            match e {
                Expr::Term(Term::Bol | Term::Indent | Term::Dedent) => true,
                Expr::Ref(r) => layout.contains(r.as_str()),
                Expr::Seq(v) | Expr::Alt(v) => v.iter().any(|e| direct(e, layout)),
                Expr::Opt(e) | Expr::Rep(e) => direct(e, layout),
                _ => false,
            }
        }
        loop {
            let mut changed = false;
            for (n, e) in &self.g.rules {
                if !self.layout.contains(n.as_str()) && direct(e, &self.layout) {
                    self.layout.insert(n.as_str());
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    fn first_sets(&mut self) {
        loop {
            let mut changed = false;
            for (n, e) in &self.g.rules {
                let v = first_of(e, &self.first);
                if self.first.get(n.as_str()) != Some(&v) {
                    self.first.insert(n.as_str(), v);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// Whether the symbols in `items` begin only with word characters.
    fn word_next(&self, items: &[Expr]) -> bool {
        let mut edges = HashSet::new();
        for e in items {
            let (f, n) = first_of(e, &self.first);
            edges.extend(f);
            if !n {
                return edges.iter().all(|x| *x == Edge::Word) && !edges.is_empty();
            }
        }
        false
    }

    fn name(rule: &str, depth: Option<usize>) -> String {
        match depth {
            Some(d) => format!("{rule}-d{d}"),
            None => rule.to_owned(),
        }
    }

    /// Render `e` at indentation `depth`; None when it cannot be produced within `MAX_DEPTH`.
    fn render(&self, e: &Expr, depth: usize, viable: &HashSet<String>) -> Option<String> {
        match e {
            Expr::Lit(l) => Some(format!(
                "\"{}\"",
                l.replace('\\', "\\\\").replace('"', "\\\"")
            )),
            Expr::Term(t) => Some(
                match t {
                    Term::Name => "name",
                    Term::TypeName => "typename",
                    Term::Int => "int",
                    Term::Float => "float",
                    Term::Str => "string",
                    Term::Bytes => "bytes",
                    Term::Char => "char",
                    Term::FStr => "fstring",
                    Term::TStr => "tstring",
                    Term::Newline => "nl",
                    Term::Indent | Term::Dedent => "",
                    Term::Bol => return Some(format!("\"{}\"", " ".repeat(4 * depth))),
                }
                .to_owned(),
            ),
            Expr::Ref(r) => {
                let d = self.layout.contains(r.as_str()).then_some(depth);
                let n = Self::name(r, d);
                viable.contains(&n).then_some(n)
            }
            Expr::Alt(v) => {
                let parts: Vec<String> = v
                    .iter()
                    .filter_map(|e| self.render(e, depth, viable))
                    .collect();
                (!parts.is_empty()).then(|| format!("({})", parts.join(" | ")))
            }
            Expr::Opt(x) => Some(match self.render(x, depth, viable) {
                Some(s) => format!("({s})?"),
                None => "\"\"".to_owned(),
            }),
            Expr::Rep(x) => Some(match self.render(x, depth, viable) {
                Some(s) => format!("({s})*"),
                None => "\"\"".to_owned(),
            }),
            Expr::Seq(items) => self.render_seq(items, depth, viable),
        }
    }

    fn render_seq(&self, items: &[Expr], depth: usize, viable: &HashSet<String>) -> Option<String> {
        let mut d = depth;
        let mut out = Vec::new();
        for (i, e) in items.iter().enumerate() {
            match e {
                Expr::Term(Term::Indent) => {
                    d += 1;
                    if d > MAX_DEPTH {
                        return None;
                    }
                    continue;
                }
                Expr::Term(Term::Dedent) => {
                    d = d.checked_sub(1)?;
                    continue;
                }
                Expr::Term(Term::Bol) if d == 0 => continue,
                _ => {}
            }
            let mut s = self.render(e, d, viable)?;
            let spaced = match e {
                Expr::Lit(l) => Some(l.starts_with(|c: char| c.is_ascii_alphabetic())),
                Expr::Term(Term::Name | Term::TypeName | Term::Int | Term::Float) => Some(true),
                Expr::Term(Term::Newline | Term::Bol) => None,
                Expr::Term(_) => Some(false),
                _ => None,
            };
            if let Some(word) = spaced {
                s.push_str(if word && self.word_next(&items[i + 1..]) {
                    " sp"
                } else {
                    " ws"
                });
            }
            out.push(s);
        }
        Some(if out.is_empty() {
            "\"\"".to_owned()
        } else {
            out.join(" ")
        })
    }

    fn emit(&self) -> String {
        let mut viable: HashSet<String> = HashSet::new();
        let instances = |n: &str| -> Vec<Option<usize>> {
            if self.layout.contains(n) {
                (0..=MAX_DEPTH).map(Some).collect()
            } else {
                vec![None]
            }
        };
        loop {
            let mut changed = false;
            for (n, e) in &self.g.rules {
                for d in instances(n) {
                    let name = Self::name(n, d);
                    if !viable.contains(&name) && self.render(e, d.unwrap_or(0), &viable).is_some()
                    {
                        viable.insert(name);
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        let mut out = String::from(
            "# Yan Agent, generated by `yan grammar --format gbnf` from crates/yan-yag/grammar/yag.ebnf.\n\
             # Indentation is unrolled to eight levels. Not expressed: reserved words excluded from\n\
             # `name`, comments, and lines broken inside brackets. Soft keywords are ordinary names.\n",
        );
        out.push_str("root ::= module-d0\n");
        for (n, e) in &self.g.rules {
            for d in instances(n) {
                let name = Self::name(n, d);
                if !viable.contains(&name) {
                    continue;
                }
                let body = self.render(e, d.unwrap_or(0), &viable).expect("viable");
                out.push_str(&format!("{name} ::= {body}\n"));
            }
        }
        out.push_str(LEXICAL);
        out
    }
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
        let g = Grammar::yag();
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
        let g = Grammar::yag();
        let (mut rejected, mut disagree) = (0, Vec::new());
        for (n, (_, src)) in corpus().iter().enumerate() {
            for m in mutants(src, n * 7 + 3) {
                if parses(&m) {
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
    fn grammar_has_no_left_recursion() {
        let g = Grammar::yag();
        fn nullable(e: &Expr, n: &HashSet<&str>) -> bool {
            match e {
                Expr::Lit(_)
                | Expr::Term(Term::Name | Term::TypeName | Term::Int | Term::Float) => false,
                Expr::Term(t) => !matches!(
                    t,
                    Term::Str | Term::Bytes | Term::Char | Term::FStr | Term::TStr | Term::Newline
                ),
                Expr::Ref(r) => n.contains(r.as_str()),
                Expr::Seq(v) => v.iter().all(|e| nullable(e, n)),
                Expr::Alt(v) => v.iter().any(|e| nullable(e, n)),
                Expr::Opt(_) | Expr::Rep(_) => true,
            }
        }
        let mut null = HashSet::new();
        while let Some(r) = g
            .rules
            .iter()
            .find(|(r, e)| !null.contains(r.as_str()) && nullable(e, &null))
        {
            null.insert(r.0.as_str());
        }
        fn left<'a>(e: &'a Expr, n: &HashSet<&str>, out: &mut Vec<&'a str>) {
            match e {
                Expr::Ref(r) => out.push(r),
                Expr::Seq(v) => {
                    for e in v {
                        left(e, n, out);
                        if !nullable(e, n) {
                            break;
                        }
                    }
                }
                Expr::Alt(v) => v.iter().for_each(|e| left(e, n, out)),
                Expr::Opt(e) | Expr::Rep(e) => left(e, n, out),
                _ => {}
            }
        }
        let edges: HashMap<&str, Vec<&str>> = g
            .rules
            .iter()
            .map(|(r, e)| {
                let mut v = Vec::new();
                left(e, &null, &mut v);
                (r.as_str(), v)
            })
            .collect();
        for start in edges.keys() {
            let mut seen = HashSet::new();
            let mut stack = edges[start].clone();
            while let Some(r) = stack.pop() {
                assert_ne!(r, *start, "left recursion through {start}");
                if seen.insert(r) {
                    stack.extend(edges[r].iter().copied());
                }
            }
        }
    }

    #[test]
    fn gbnf_is_closed_and_reachable() {
        let text = Grammar::yag().gbnf();
        let defs: HashSet<&str> = text
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| l.split_once(" ::= ").map(|x| x.0))
            .collect();
        let mut used = HashSet::new();
        for l in text.lines().filter(|l| !l.starts_with('#')) {
            let Some((_, body)) = l.split_once(" ::= ") else {
                continue;
            };
            let mut in_str = false;
            let mut in_class = false;
            let mut esc = false;
            let mut word = String::new();
            for c in body.chars().chain([' ']) {
                if esc {
                    esc = false;
                    continue;
                }
                match c {
                    '\\' => esc = true,
                    '"' if !in_class => in_str = !in_str,
                    '[' if !in_str => in_class = true,
                    ']' if !in_str => in_class = false,
                    c if !in_str && !in_class && (c.is_ascii_alphanumeric() || c == '-') => {
                        word.push(c)
                    }
                    _ => {
                        if !word.is_empty() {
                            used.insert(std::mem::take(&mut word));
                        }
                    }
                }
            }
        }
        let undefined: Vec<_> = used.iter().filter(|u| !defs.contains(u.as_str())).collect();
        assert!(undefined.is_empty(), "{undefined:?}");
        assert!(defs.contains("module-d0"));
        assert!(defs.contains("stmt-d8") && !defs.contains("stmt-d9"));
    }
}
