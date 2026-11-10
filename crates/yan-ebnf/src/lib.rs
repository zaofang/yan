//! A surface grammar as data: an EBNF reader, a recognizer over lexer tokens for tests, and the
//! text emitters for constrained decoding (21.3): GBNF, XGrammar EBNF and llguidance Lark. Each
//! surface (Yan Agent, Yan) supplies its EBNF text, its token classes and its lexical rules.

pub mod checks;
use std::collections::{HashMap, HashSet};

/// Indentation levels the text grammars unroll; deeper nesting is not generatable.
const MAX_DEPTH: usize = 8;

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
    Unit,
    Date,
    Time,
    Instant,
    Zoned,
    /// A `-` or `+` attached to a grid element.
    Sign,
    /// A `(` after a space inside a grid.
    SplParen,
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

/// One lexer token as the recognizer sees it: its literal spelling, if it has one, and the
/// terminal classes it belongs to.
pub struct Tk {
    pub lit: Option<String>,
    pub terms: Vec<Term>,
}

/// What a surface contributes to the shared grammar machinery.
pub struct Surface {
    /// Display name, as in "Yan Agent".
    pub title: &'static str,
    /// The `--as` value.
    pub cmd: &'static str,
    /// Repository path of the EBNF, named in the generated header.
    pub path: &'static str,
    pub ebnf: &'static str,
    /// Header lines saying what the text grammars do not express.
    pub notes: &'static str,
    /// Lexical rules of the GBNF and XGrammar texts, and of the Lark text.
    pub lexical: &'static str,
    pub lark_lexical: &'static str,
    /// The lexer's tokens up to the end of input, None when lexing failed fatally.
    pub tokens: fn(&str) -> Option<Vec<Tk>>,
}

#[derive(Clone)]
pub struct Grammar {
    /// In source order; the first rule is the start.
    pub rules: Vec<(String, Expr)>,
    pub surface: &'static Surface,
}

impl Grammar {
    pub fn of(surface: &'static Surface) -> Grammar {
        Grammar::parse(surface.ebnf, surface).expect("embedded grammar")
    }

    pub fn parse(src: &str, surface: &'static Surface) -> Result<Grammar, String> {
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
        let g = Grammar { rules, surface };
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

    /// Whether `src` is a module of the grammar, judged on the lexer's tokens.
    pub fn accepts(&self, src: &str) -> bool {
        self.check_source(src).is_ok()
    }

    /// Like `accepts`; on rejection, the index of the first token no derivation can continue with.
    pub fn check_source(&self, src: &str) -> Result<(), usize> {
        Recognizer::new(self).check(src, self.surface.tokens)
    }

    /// GBNF for llama.cpp, with indentation unrolled to `MAX_DEPTH` levels.
    pub fn gbnf(&self) -> String {
        Gbnf::new(self, Dialect::Gbnf).emit()
    }

    /// XGrammar EBNF, with indentation unrolled to `MAX_DEPTH` levels.
    pub fn xgrammar(&self) -> String {
        Gbnf::new(self, Dialect::XGrammar).emit()
    }

    /// llguidance Lark grammar, with indentation unrolled to `MAX_DEPTH` levels.
    pub fn llguidance(&self) -> String {
        Gbnf::new(self, Dialect::Lark).emit()
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
                "UNIT" => Expr::Term(Term::Unit),
                "DATE" => Expr::Term(Term::Date),
                "TIME" => Expr::Term(Term::Time),
                "INSTANT" => Expr::Term(Term::Instant),
                "ZONED" => Expr::Term(Term::Zoned),
                "SIGN" => Expr::Term(Term::Sign),
                "SPLPAREN" => Expr::Term(Term::SplParen),
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

fn matches(sym: &Sym, tok: &Tk) -> bool {
    match sym {
        Sym::Lit(l) => tok.lit.as_deref() == Some(l),
        Sym::T(t) => tok.terms.contains(t),
        Sym::N(_) => false,
    }
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

    fn check(&self, src: &str, tokens: fn(&str) -> Option<Vec<Tk>>) -> Result<(), usize> {
        let toks = tokens(src).ok_or(0usize)?;
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
            Term::Name
            | Term::TypeName
            | Term::Int
            | Term::Float
            | Term::Unit
            | Term::Date
            | Term::Time
            | Term::Instant
            | Term::Zoned => (edge(true), false),
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

#[derive(Clone, Copy, PartialEq)]
enum Dialect {
    Gbnf,
    XGrammar,
    Lark,
}

impl Dialect {
    fn define(self) -> &'static str {
        match self {
            Dialect::Lark => ": ",
            _ => " ::= ",
        }
    }

    fn header(self, s: &Surface) -> String {
        let (format, comment) = match self {
            Dialect::Gbnf => ("gbnf", "#"),
            Dialect::XGrammar => ("xgrammar", "#"),
            Dialect::Lark => ("llguidance", "//"),
        };
        let mut out = format!(
            "{comment} {}, generated by `yan grammar --as {} --format {format}` from {}.\n",
            s.title, s.cmd, s.path
        );
        for l in s.notes.lines() {
            out.push_str(&format!("{comment} {l}\n"));
        }
        out
    }

    fn start(self) -> &'static str {
        match self {
            Dialect::Lark => "start: module_d0\n",
            _ => "root ::= module-d0\n",
        }
    }

    fn lexical(self, s: &Surface) -> String {
        match self {
            Dialect::Gbnf => s.lexical.to_owned(),
            Dialect::XGrammar => s.lexical.replace("\"\\\\\" . |", "\"\\\\\" [^\\x00] |"),
            Dialect::Lark => s.lark_lexical.to_owned(),
        }
    }
}

struct Gbnf<'a> {
    dialect: Dialect,
    g: &'a Grammar,
    layout: HashSet<&'a str>,
    /// First-token classes and nullability per rule.
    first: HashMap<&'a str, (HashSet<Edge>, bool)>,
}

impl<'a> Gbnf<'a> {
    fn new(g: &'a Grammar, dialect: Dialect) -> Self {
        let mut s = Gbnf {
            dialect,
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

    fn name(&self, rule: &str, depth: Option<usize>) -> String {
        match (depth, self.dialect) {
            (Some(d), Dialect::Lark) => format!("{rule}_d{d}"),
            (Some(d), _) => format!("{rule}-d{d}"),
            (None, _) => rule.to_owned(),
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
                    Term::Unit => "unit",
                    Term::Date => "date",
                    Term::Time => "time",
                    Term::Instant => "instant",
                    Term::Zoned => "zoned",
                    Term::Sign => "sign",
                    Term::SplParen => "splparen",
                    Term::Newline => "nl",
                    Term::Indent | Term::Dedent => "",
                    Term::Bol => return Some(format!("\"{}\"", " ".repeat(4 * depth))),
                }
                .to_owned(),
            ),
            Expr::Ref(r) => {
                let d = self.layout.contains(r.as_str()).then_some(depth);
                let n = self.name(r, d);
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
                Expr::Term(
                    Term::Name
                    | Term::TypeName
                    | Term::Int
                    | Term::Float
                    | Term::Unit
                    | Term::Date
                    | Term::Time
                    | Term::Instant
                    | Term::Zoned,
                ) => Some(true),
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
                    let name = self.name(n, d);
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
        let mut out = self.dialect.header(self.g.surface);
        out.push_str(self.dialect.start());
        for (n, e) in &self.g.rules {
            for d in instances(n) {
                let name = self.name(n, d);
                if !viable.contains(&name) {
                    continue;
                }
                let body = self.render(e, d.unwrap_or(0), &viable).expect("viable");
                out.push_str(&format!("{name}{}{body}\n", self.dialect.define()));
            }
        }
        out.push_str(&self.dialect.lexical(self.g.surface));
        out
    }
}
