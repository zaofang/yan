//! Recursive-descent parser for the factored Yan Agent grammar of 13.2, with the lenient-reader
//! normalizations that are purely syntactic (14.4). Errors recover at item boundaries (22.3).

use std::collections::{HashMap, HashSet};

use yan_cst::{Builder, Cst, Kind, LineIndex};
use yan_diag::{Severity, Span};
use yan_names::{Surface, TYPES, type_row};

use crate::ast::*;
use crate::lexer::{FPart, Issue, Rename, Tok, Token, lex};

const RESERVED: [&str; 34] = [
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
    "fail",
    "par",
    "become",
    "unreachable",
    "try",
    "except",
    "finally",
    "raise",
    "yield",
];
const REJECTED_E120: [&str; 4] = ["try", "except", "finally", "raise"];
const REJECTED_E121: [&str; 6] = ["global", "nonlocal", "del", "yield", "async", "await"];
pub(crate) const AUGOPS: [&str; 12] = [
    "+=", "-=", "*=", "/=", "//=", "%=", "**=", "&=", "|=", "^=", "<<=", ">>=",
];
const CONTRACT_WORDS: [&str; 5] = ["pre", "post", "ex", "decreases", "cost"];
const BITWISE: [&str; 5] = ["|", "^", "&", "<<", ">>"];
const ARITH: [&str; 7] = ["+", "-", "*", "/", "//", "%", "@"];

struct PErr {
    code: &'static str,
    span: Span,
    message: String,
}

type P<T> = Result<T, PErr>;

struct Parser {
    toks: Vec<Token>,
    pos: usize,
    strict: bool,
    pub issues: Vec<Issue>,
    /// N09: methods lifted out of the class being read, emitted after it.
    lifted: Vec<Item>,
    /// N09: the type an unannotated `self` parameter takes.
    self_ty: Option<Type>,
    /// Syntax nodes as token index ranges, for the lossless tree (R995).
    nodes: Vec<(Kind, usize, usize)>,
    /// N45: statements a simple statement's multi-`for` comprehensions were rewritten to, which
    /// run before it.
    hoisted: Vec<Stmt>,
    /// Reading a simple statement, where a multi-`for` comprehension can be hoisted.
    in_simple: bool,
    comps: usize,
}

/// Parse a `.yag` file. The module is `None` only when the lexer stopped early.
pub fn parse(src: &str, strict: bool) -> (Option<Module>, Vec<Issue>) {
    let (m, issues, _) = parse_with_syntax(src, strict);
    (m, issues)
}

/// `parse` and the lossless tree over `src` (R995), built even when lexing stops early.
pub fn parse_with_syntax(src: &str, strict: bool) -> (Option<Module>, Vec<Issue>, Cst) {
    let lexed = lex(src);
    let mut p = Parser {
        toks: lexed.tokens,
        pos: 0,
        strict,
        issues: lexed.issues,
        lifted: Vec::new(),
        self_ty: None,
        nodes: Vec::new(),
        hoisted: Vec::new(),
        in_simple: false,
        comps: 0,
    };
    let m = (!lexed.fatal).then(|| {
        p.check_names();
        if !strict {
            let notes = crate::nested::lift(&mut p.toks);
            p.issues.extend(notes);
        }
        let header = src
            .lines()
            .take_while(|l| l.starts_with('#'))
            .map(str::to_owned)
            .collect();
        p.module(header)
    });
    let cst = syntax(src, &p.toks, &lexed.comments, &p.nodes);
    (m, p.issues, cst)
}

fn syntax(
    src: &str,
    toks: &[Token],
    comments: &[(usize, usize)],
    nodes: &[(Kind, usize, usize)],
) -> Cst {
    let lines = LineIndex::new(src);
    let range = |t: &Token| {
        (
            lines.offset(t.span.line, t.span.col),
            lines.offset(t.span.end_line, t.span.end_col),
        )
    };
    let code = |t: &&Token| !matches!(t.tok, Tok::Newline | Tok::Indent | Tok::Dedent | Tok::Eof);
    let mut b = Builder::new(src);
    for t in toks.iter().filter(code) {
        let (lo, hi) = range(t);
        b.token(lo, hi);
    }
    for &(lo, hi) in comments {
        b.comment(lo, hi, false);
    }
    for &(kind, from, to) in nodes {
        let span = toks[from..to]
            .iter()
            .filter(code)
            .map(range)
            .reduce(|(lo, hi), (l, h)| (lo.min(l), hi.max(h)));
        if let Some((lo, hi)) = span {
            b.node(kind, lo, hi);
        }
    }
    b.finish()
}

/// `myValue` as `my_value`; `None` when that is not a valid name or changes nothing.
fn snake_case(n: &str) -> Option<String> {
    let mut out = String::new();
    let mut prev: Option<char> = None;
    for c in n.chars() {
        if c.is_ascii_uppercase() {
            if prev.is_some_and(|p| p.is_ascii_lowercase() || p.is_ascii_digit()) {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else if c == '_' && out.ends_with('_') {
            continue;
        } else {
            out.push(c);
        }
        prev = Some(c);
    }
    let out = out.trim_end_matches('_').to_owned();
    let ok = out.starts_with(|c: char| c.is_ascii_lowercase())
        && out
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    (ok && out != n).then_some(out)
}

fn is_type_name(n: &str) -> bool {
    n.starts_with(|c: char| c.is_ascii_uppercase())
}

fn bin(op: &'static str, l: Expr, r: Expr) -> Expr {
    let span = l.span;
    Expr {
        kind: ExprKind::Binary(op, Box::new(l), Box::new(r)),
        span,
        paren: false,
    }
}

fn op_of(e: &Expr) -> Option<&'static str> {
    match &e.kind {
        ExprKind::Binary(op, ..) if !e.paren => Some(op),
        _ => None,
    }
}

impl Parser {
    fn node<T>(&mut self, kind: Kind, f: impl FnOnce(&mut Self) -> P<T>) -> P<T> {
        let from = self.pos;
        let r = f(self);
        if r.is_ok() {
            self.nodes.push((kind, from, self.pos));
        }
        r
    }

    // ---- tokens ----

    fn tok(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn tok_at(&self, n: usize) -> &Tok {
        &self.toks[(self.pos + n).min(self.toks.len() - 1)].tok
    }

    fn span(&self) -> Span {
        self.toks[self.pos].span
    }

    fn bump(&mut self) -> Tok {
        let t = self.tok().clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn is_op(&self, op: &str) -> bool {
        matches!(self.tok(), Tok::Op(o) if *o == op)
    }

    fn is_op_at(&self, n: usize, op: &str) -> bool {
        matches!(self.tok_at(n), Tok::Op(o) if *o == op)
    }

    fn eat_op(&mut self, op: &str) -> bool {
        let yes = self.is_op(op);
        if yes {
            self.bump();
        }
        yes
    }

    fn is_kw(&self, kw: &str) -> bool {
        matches!(self.tok(), Tok::Name(n) if n == kw)
    }

    fn is_kw_at(&self, n: usize, kw: &str) -> bool {
        matches!(self.tok_at(n), Tok::Name(x) if x == kw)
    }

    fn eat_kw(&mut self, kw: &str) -> bool {
        let yes = self.is_kw(kw);
        if yes {
            self.bump();
        }
        yes
    }

    /// R462: a soft keyword is the keyword unless the next token is `=` or an augmented assignment.
    fn soft(&self, kw: &str) -> bool {
        self.is_kw(kw) && !self.is_op_at(1, "=") && !AUGOPS.iter().any(|a| self.is_op_at(1, a))
    }

    fn err<T>(&self, code: &'static str, message: impl Into<String>) -> P<T> {
        Err(PErr {
            code,
            span: self.span(),
            message: message.into(),
        })
    }

    fn unexpected<T>(&self, wanted: &str) -> P<T> {
        let found = match self.tok() {
            Tok::Name(n) => format!("`{n}`"),
            Tok::Int(s) | Tok::Float(s) => format!("`{s}`"),
            Tok::Str { .. } | Tok::FStr { .. } => "a string".into(),
            Tok::Char(_) => "a char".into(),
            Tok::Op(o) => format!("`{o}`"),
            Tok::Newline => "end of line".into(),
            Tok::Indent => "an indented block".into(),
            Tok::Dedent => "the end of the block".into(),
            Tok::Eof => "end of file".into(),
        };
        self.err("E100", format!("unexpected {found}; expected {wanted}"))
    }

    fn expect_op(&mut self, op: &str) -> P<()> {
        if self.eat_op(op) {
            Ok(())
        } else {
            self.unexpected(&format!("`{op}`"))
        }
    }

    fn expect_kw(&mut self, kw: &str) -> P<()> {
        if self.eat_kw(kw) {
            Ok(())
        } else {
            self.unexpected(&format!("`{kw}`"))
        }
    }

    fn expect_newline(&mut self) -> P<()> {
        if matches!(self.tok(), Tok::Newline) {
            self.bump();
            Ok(())
        } else if matches!(self.tok(), Tok::Dedent | Tok::Eof) {
            Ok(())
        } else {
            self.unexpected("end of line")
        }
    }

    fn name(&mut self) -> P<Name> {
        match self.tok() {
            Tok::Name(n) if !RESERVED.contains(&n.as_str()) => {
                let n = n.clone();
                self.bump();
                Ok(n)
            }
            _ => self.unexpected("a name"),
        }
    }

    fn path(&mut self) -> P<Path> {
        let mut p = vec![self.name()?];
        while self.is_op(".") && matches!(self.tok_at(1), Tok::Name(_)) {
            self.bump();
            p.push(self.name()?);
        }
        Ok(p)
    }

    fn note(&mut self, code: &'static str, span: Span, message: impl Into<String>) {
        self.issues.push(Issue {
            code,
            severity: Severity::Note,
            span,
            message: message.into(),
            fix: None,
        });
    }

    fn error(&mut self, code: &'static str, span: Span, message: impl Into<String>) {
        self.issues.push(Issue {
            code,
            severity: Severity::Error,
            span,
            message: message.into(),
            fix: None,
        });
    }

    /// E101 in strict mode, N06 under the lenient reader (which inserts the parentheses).
    fn mix(&mut self, span: Span, what: &str) {
        if self.strict {
            self.error("E101", span, format!("{what} needs parentheses"));
        } else {
            self.note(
                "N06",
                span,
                format!("{what}; parentheses inserted by Python precedence"),
            );
        }
    }

    /// 13.1 naming rules, once per distinct name at its first occurrence.
    fn check_names(&mut self) {
        let mut seen = HashSet::new();
        let mut found = Vec::new();
        let mut at: HashMap<&str, Vec<(Span, bool)>> = HashMap::new();
        for (i, t) in self.toks.iter().enumerate() {
            if let Tok::Name(n) = &t.tok {
                let attr = i > 0 && matches!(self.toks[i - 1].tok, Tok::Op("."));
                at.entry(n).or_default().push((t.span, attr));
            }
        }
        for t in &self.toks {
            let Tok::Name(n) = &t.tok else { continue };
            if !seen.insert(n.clone()) || n == "_" || (n.starts_with("__") && n.ends_with("__")) {
                continue;
            }
            if is_type_name(n) {
                let b = n.as_bytes();
                if b.windows(2)
                    .any(|w| w[0].is_ascii_uppercase() && w[1].is_ascii_uppercase())
                {
                    found.push((if self.strict { "E112" } else { "N07" }, t.span, n.clone()));
                }
                continue;
            }
            let ok = n.starts_with(|c: char| c.is_ascii_lowercase())
                && !n.contains("__")
                && !n.ends_with('_')
                && n.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
            if !ok {
                found.push(("E110", t.span, n.clone()));
            } else if n.len() > 32 {
                found.push(("W111", t.span, n.clone()));
            }
        }
        for (code, span, n) in found {
            let (sev, msg) = match code {
                "E110" => (
                    Severity::Error,
                    format!("`{n}` is not a lowercase snake_case name"),
                ),
                "W111" => (
                    Severity::Warning,
                    format!("`{n}` is longer than 32 characters"),
                ),
                "E112" => (Severity::Error, format!("`{n}` has consecutive capitals")),
                _ => (
                    Severity::Note,
                    format!("`{n}` has consecutive capitals; renamed throughout the module"),
                ),
            };
            // A rename is offered when the snake_case spelling is free; it is automatic unless
            // the name is also written as an attribute, which may belong to another module.
            let fix = (code == "E110")
                .then(|| snake_case(&n))
                .flatten()
                .filter(|s| !at.contains_key(s.as_str()))
                .map(|to| {
                    let uses = &at[n.as_str()];
                    let mut spans: Vec<Span> = uses.iter().map(|(s, _)| *s).collect();
                    let (inner, ambiguous) = self.interpolated_uses(&n);
                    spans.extend(inner);
                    Rename {
                        spans,
                        from: n.clone(),
                        to,
                        auto: !ambiguous && !uses.iter().any(|(_, attr)| *attr),
                    }
                });
            self.issues.push(Issue {
                code,
                severity: sev,
                span,
                message: msg,
                fix,
            });
        }
    }

    /// Spans of `name` inside f-string interpolations, and whether any use there is not a plain
    /// single-line, non-attribute word (so the rename is not safe to apply by itself).
    fn interpolated_uses(&self, name: &str) -> (Vec<Span>, bool) {
        let (mut spans, mut ambiguous) = (Vec::new(), false);
        for t in &self.toks {
            let Tok::FStr { parts, .. } = &t.tok else {
                continue;
            };
            for part in parts {
                let FPart::Expr { src, line, col, .. } = part else {
                    continue;
                };
                let b = src.as_bytes();
                let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
                for (i, _) in src.match_indices(name) {
                    let end = i + name.len();
                    if (i > 0 && word(b[i - 1])) || b.get(end).is_some_and(|c| word(*c)) {
                        continue;
                    }
                    if src.contains('\n')
                        || src.contains(['\'', '"'])
                        || (i > 0 && b[i - 1] == b'.')
                    {
                        ambiguous = true;
                        continue;
                    }
                    let at = col + i as u32;
                    spans.push(Span {
                        line: *line,
                        col: at,
                        end_line: *line,
                        end_col: at + name.len() as u32,
                    });
                }
            }
        }
        (spans, ambiguous)
    }

    // ---- module and items ----

    fn module(&mut self, header: Vec<String>) -> Module {
        let mut m = Module {
            header,
            attrs: Vec::new(),
            doc: None,
            items: Vec::new(),
        };
        while self.is_op("@@") {
            self.bump();
            match self.decorator_rest() {
                Ok(d) => m.attrs.push(d),
                Err(e) => self.recover(e),
            }
        }
        if let Tok::Str { value, .. } = self.tok()
            && matches!(self.tok_at(1), Tok::Newline)
        {
            m.doc = Some(value.clone());
            self.bump();
            self.bump();
        }
        while !matches!(self.tok(), Tok::Eof) {
            if matches!(self.tok(), Tok::Newline | Tok::Dedent | Tok::Indent) {
                self.bump();
                continue;
            }
            if self.is_kw("import") || (self.is_kw("from") && self.toks[self.pos].line_start) {
                let sp = self.span();
                self.note("N01", sp, "import removed; uses become qualified names");
                self.skip_line();
                continue;
            }
            if !self.strict
                && self.at_loose_ex()
                && let Some(ItemKind::Func(FuncDef {
                    body: Some(body), ..
                })) = m.items.last_mut().map(|it| &mut it.kind)
            {
                let sp = self.span();
                match self.contract() {
                    Ok(c) => {
                        self.note(
                            "N32",
                            sp,
                            "top-level `ex` moved into the body of the function above",
                        );
                        body.contracts.push(c);
                    }
                    Err(e) => self.recover(e),
                }
                continue;
            }
            match self.item() {
                Ok(it) => m.items.push(it),
                Err(e) => self.recover(e),
            }
            m.items.append(&mut self.lifted);
        }
        m
    }

    /// An `ex` line at the left margin, where an item should start (N32).
    fn at_loose_ex(&self) -> bool {
        self.toks[self.pos].line_start
            && self.toks[self.pos].span.col == 1
            && self.soft("ex")
            && matches!(
                self.tok_at(1),
                Tok::Name(_)
                    | Tok::Int(_)
                    | Tok::Float(_)
                    | Tok::Str { .. }
                    | Tok::Op("-" | "[" | "(")
            )
    }

    fn recover(&mut self, e: PErr) {
        self.error(e.code, e.span, e.message);
        self.bump();
        while !matches!(self.tok(), Tok::Eof) {
            let t = &self.toks[self.pos];
            if t.line_start
                && t.span.col == 1
                && !matches!(t.tok, Tok::Dedent | Tok::Indent | Tok::Newline)
            {
                break;
            }
            self.bump();
        }
    }

    fn skip_line(&mut self) {
        while !matches!(self.tok(), Tok::Newline | Tok::Eof) {
            self.bump();
        }
        self.bump();
    }

    /// Skip one statement, including the block a compound header opens.
    fn skip_stmt(&mut self) {
        self.skip_line();
        if matches!(self.tok(), Tok::Indent) {
            let mut depth = 0;
            loop {
                match self.bump() {
                    Tok::Indent => depth += 1,
                    Tok::Dedent => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    Tok::Eof => break,
                    _ => {}
                }
            }
        }
    }

    fn decorator_rest(&mut self) -> P<Decorator> {
        let path = self.path()?;
        let args = if self.is_op("(") {
            Some(self.args()?)
        } else {
            None
        };
        self.expect_newline()?;
        Ok(Decorator { path, args })
    }

    fn item(&mut self) -> P<Item> {
        self.node(Kind::Item, Self::item_)
    }

    fn item_(&mut self) -> P<Item> {
        let mut decorators = Vec::new();
        while self.is_op("@") {
            let at = self.span();
            self.bump();
            let d = self.decorator_rest()?;
            if !self.strict
                && self.is_kw("class")
                && matches!(d.path.last(), Some(n) if n == "dataclass")
            {
                self.note(
                    "N08",
                    at,
                    "`@dataclass` removed; a Yan Agent class is a record",
                );
                continue;
            }
            decorators.push(d);
        }
        let span = self.span();
        let is_pub = self.eat_kw("pub");
        let name_at = |p: &Self| p.toks[(p.pos + 1).min(p.toks.len() - 1)].span;
        let (kind, name_span) = if self.is_kw("def") {
            self.bump();
            let f = self.funcdef_rest(false)?;
            let ns = f.name_span;
            (ItemKind::Func(f), ns)
        } else if self.is_kw("class") {
            let ns = name_at(self);
            self.bump();
            (ItemKind::Record(self.class_rest(is_pub)?), ns)
        } else if self.soft("type") && matches!(self.tok_at(1), Tok::Name(n) if is_type_name(n)) {
            let ns = name_at(self);
            self.bump();
            (ItemKind::Type(self.typedef_rest()?), ns)
        } else if self.soft("trait") && matches!(self.tok_at(1), Tok::Name(_)) {
            let ns = name_at(self);
            self.bump();
            (ItemKind::Trait(self.trait_rest()?), ns)
        } else if self.soft("impl") && matches!(self.tok_at(1), Tok::Name(_)) {
            self.bump();
            (ItemKind::Impl(self.impl_rest()?), span)
        } else if self.soft("lemma") && matches!(self.tok_at(1), Tok::Name(_)) {
            self.bump();
            let f = self.funcdef_rest(true)?;
            let ns = f.name_span;
            (ItemKind::Lemma(f), ns)
        } else if !self.strict
            && matches!(self.tok(), Tok::Name(n) if is_type_name(n) && n.contains(|c: char| c.is_ascii_lowercase()))
            && self.is_op_at(1, "=")
            && matches!(self.tok_at(2), Tok::Name(_))
        {
            let ns = self.span();
            self.note("N20", ns, "type alias normalized to `type X = ...`");
            (ItemKind::Type(self.typedef_rest()?), ns)
        } else if matches!(self.tok(), Tok::Name(_)) && self.is_op_at(1, ":") {
            let ns = self.span();
            let name = self.name()?;
            self.expect_op(":")?;
            let ty = self.ty()?;
            self.expect_op("=")?;
            let value = self.expr()?;
            self.expect_newline()?;
            (ItemKind::Const { name, ty, value }, ns)
        } else {
            return self.unexpected("an item");
        };
        Ok(Item {
            decorators,
            is_pub,
            kind,
            span,
            name_span,
        })
    }

    /// After `def` or `lemma`.
    fn funcdef_rest(&mut self, lemma: bool) -> P<FuncDef> {
        let name_span = self.span();
        let name = self.name()?;
        let tparams = self.tparams()?;
        let params = self.params()?;
        let ret = if !lemma && self.eat_op("->") {
            Some(self.rettype()?)
        } else {
            None
        };
        let uses = if !lemma && self.soft("uses") {
            Some(self.uses()?)
        } else {
            None
        };
        self.expect_op(":")?;
        let body = if self.is_op("...") {
            self.bump();
            self.expect_newline()?;
            None
        } else {
            let fails = ret.as_ref().is_some_and(|r| !r.fail.is_empty());
            let mut b = self.fbody()?;
            if fails && !self.strict {
                self.normalize_returns(&mut b.stmts);
            }
            Some(b)
        };
        Ok(FuncDef {
            name,
            name_span,
            tparams,
            params,
            ret,
            uses,
            body,
        })
    }

    /// N26: `return Ok(v)` and `return Err(e)` in a `T fail E` function.
    fn normalize_returns(&mut self, stmts: &mut [Stmt]) {
        for s in stmts {
            match &mut s.kind {
                StmtKind::Return(Some(e)) => {
                    if let ExprKind::Call(f, args) = &e.kind
                        && let ExprKind::Name(n) = &f.kind
                        && (n == "Ok" || n == "Err")
                        && args.len() == 1
                        && args[0].name.is_none()
                        && let ArgKind::Expr(v) = &args[0].kind
                    {
                        let ok = n == "Ok";
                        let v = v.clone();
                        self.note(
                            "N26",
                            s.span,
                            format!("`return {}(...)` normalized", if ok { "Ok" } else { "Err" }),
                        );
                        s.kind = if ok {
                            StmtKind::Return(Some(v))
                        } else {
                            StmtKind::Fail(v)
                        };
                    }
                }
                StmtKind::If { arms, els } => {
                    for (_, b) in arms.iter_mut() {
                        self.normalize_returns(&mut b.stmts);
                    }
                    if let Some(b) = els {
                        self.normalize_returns(&mut b.stmts);
                    }
                }
                StmtKind::While { body, .. } | StmtKind::For { body, .. } => {
                    self.normalize_returns(body)
                }
                StmtKind::Match { cases, .. } => {
                    for c in cases {
                        self.normalize_returns(&mut c.body.stmts);
                    }
                }
                StmtKind::With { body, .. } => self.normalize_returns(&mut body.stmts),
                _ => {}
            }
        }
    }

    fn rettype(&mut self) -> P<RetType> {
        let mut r = RetType {
            ty: None,
            fail: Vec::new(),
            from: Vec::new(),
        };
        if self.eat_kw("fail") {
            r.fail = self.errtype()?;
        } else {
            let at = self.span();
            let ty = self.ty()?;
            match ty {
                Type::Named { path, args }
                    if path == ["Result"] && args.len() == 2 && !self.strict =>
                {
                    self.note("N23", at, "`-> Result[T, E]` normalized to `-> T fail E`");
                    r.ty = match &args[0] {
                        TArg::Type(Type::None) => None,
                        TArg::Type(t) => Some(t.clone()),
                        _ => None,
                    };
                    r.fail = match &args[1] {
                        TArg::Type(Type::Named { path, .. }) => vec![path.clone()],
                        TArg::Text(t) => t
                            .split(" | ")
                            .map(|p| {
                                let mut p = p.split('.').map(str::to_owned).collect();
                                self.builtin(&mut p, 0, at);
                                p
                            })
                            .collect(),
                        _ => Vec::new(),
                    };
                }
                Type::None => {}
                ty => r.ty = Some(ty),
            }
            if self.eat_kw("fail") {
                r.fail = self.errtype()?;
            }
        }
        if self.eat_kw("from") {
            r.from.push(self.name()?);
            while self.eat_op(",") {
                r.from.push(self.name()?);
            }
        }
        Ok(r)
    }

    fn errtype(&mut self) -> P<Vec<Path>> {
        let mut out = Vec::new();
        loop {
            let at = self.span();
            let mut p = self.path()?;
            self.builtin(&mut p, 0, at);
            out.push(p);
            if !self.eat_op("|") {
                return Ok(out);
            }
        }
    }

    fn uses(&mut self) -> P<Uses> {
        self.expect_kw("uses")?;
        if self.eat_kw("none") {
            return Ok(Uses::None);
        }
        Ok(Uses::Caps(self.caprefs()?))
    }

    fn caprefs(&mut self) -> P<Vec<CapRef>> {
        let mut out = Vec::new();
        loop {
            let path = self.path()?;
            let alias = if self.eat_kw("as") {
                Some(self.name()?)
            } else {
                None
            };
            out.push(CapRef { path, alias });
            if !self.eat_op(",") {
                return Ok(out);
            }
        }
    }

    fn tparams(&mut self) -> P<Vec<TParam>> {
        let mut out = Vec::new();
        if !self.eat_op("[") {
            return Ok(out);
        }
        loop {
            let name = self.name()?;
            if is_type_name(&name) {
                let mut bounds = Vec::new();
                if self.eat_op(":") {
                    loop {
                        bounds.push(if self.eat_kw("Nat") {
                            Bound::Nat
                        } else {
                            let path = self.path()?;
                            let args = if self.eat_op("[") {
                                self.types_until("]")?
                            } else {
                                Vec::new()
                            };
                            Bound::Path { path, args }
                        });
                        if !self.eat_op("+") {
                            break;
                        }
                    }
                }
                out.push(TParam::Type { name, bounds });
            } else {
                self.expect_op(":")?;
                out.push(TParam::Value {
                    name,
                    ty: self.ty()?,
                });
            }
            if !self.eat_op(",") {
                break;
            }
        }
        self.expect_op("]")?;
        Ok(out)
    }

    fn types_until(&mut self, close: &str) -> P<Vec<Type>> {
        let mut out = Vec::new();
        while !self.is_op(close) {
            out.push(self.ty()?);
            if !self.eat_op(",") {
                break;
            }
        }
        self.expect_op(close)?;
        Ok(out)
    }

    fn params(&mut self) -> P<Vec<Param>> {
        self.node(Kind::Params, Self::params_)
    }

    fn params_(&mut self) -> P<Vec<Param>> {
        self.expect_op("(")?;
        let mut out = Vec::new();
        while !self.is_op(")") {
            if self.eat_op("/") {
                out.push(Param::Slash);
            } else if self.eat_op("**") {
                let name = self.name()?;
                self.expect_op(":")?;
                out.push(Param::Pack {
                    name,
                    ty: self.ty()?,
                });
            } else {
                let conv = if matches!(self.tok_at(1), Tok::Name(_)) {
                    match self.tok() {
                        Tok::Name(n) if n == "var" => Some(Conv::Var),
                        Tok::Name(n) if n == "take" => Some(Conv::Take),
                        Tok::Name(n) if n == "ghost" => Some(Conv::Ghost),
                        _ => None,
                    }
                } else {
                    None
                };
                if conv.is_some() {
                    self.bump();
                }
                let span = self.span();
                let name = if self.eat_kw("_") {
                    "_".to_owned()
                } else {
                    self.name()?
                };
                if out.is_empty()
                    && name == "self"
                    && !self.is_op(":")
                    && let Some(ty) = self.self_ty.clone()
                {
                    out.push(Param::Value {
                        conv,
                        name,
                        span,
                        ty,
                        default: None,
                    });
                    if !self.eat_op(",") {
                        break;
                    }
                    continue;
                }
                if !self.is_op(":") {
                    return Err(PErr {
                        code: "E240",
                        span,
                        message: format!("parameter `{name}` needs a type annotation"),
                    });
                }
                self.bump();
                let ty = self.ty()?;
                let default = if self.eat_op("=") {
                    Some(self.expr()?)
                } else {
                    None
                };
                out.push(Param::Value {
                    conv,
                    name,
                    span,
                    ty,
                    default,
                });
            }
            if !self.eat_op(",") {
                break;
            }
        }
        self.expect_op(")")?;
        Ok(out)
    }

    fn fbody(&mut self) -> P<Body> {
        self.node(Kind::Body, Self::fbody_)
    }

    fn fbody_(&mut self) -> P<Body> {
        self.expect_newline_then_indent()?;
        let doc = self.docstring();
        let mut contracts = Vec::new();
        while CONTRACT_WORDS
            .iter()
            .any(|w| self.soft(w) && (*w != "cost" || self.cost_bound_at(1)))
        {
            contracts.push(self.contract()?);
        }
        let stmts = self.stmts_until_dedent()?;
        Ok(Body {
            doc,
            contracts,
            stmts,
        })
    }

    fn docstring(&mut self) -> Option<String> {
        if let Tok::Str { value, prefix, .. } = self.tok()
            && prefix.is_empty()
            && matches!(self.tok_at(1), Tok::Newline)
        {
            let v = value.clone();
            self.bump();
            self.bump();
            return Some(v);
        }
        None
    }

    fn contract(&mut self) -> P<Contract> {
        let Tok::Name(w) = self.bump() else {
            unreachable!("caller checked a contract word")
        };
        if w == "cost" {
            return self.cost();
        }
        let e = self.expr()?;
        let c = match w.as_str() {
            "pre" => Contract::Pre(e),
            "post" => Contract::Post(e),
            "decreases" => Contract::Decreases(e),
            _ => {
                let mut with = Vec::new();
                if self.eat_kw("with") {
                    loop {
                        let n = self.name()?;
                        self.expect_op("=")?;
                        with.push((n, self.expr()?));
                        if !self.eat_op(",") {
                            break;
                        }
                    }
                }
                Contract::Ex { expr: e, with }
            }
        };
        self.expect_newline()?;
        Ok(c)
    }

    fn cost_bound_at(&self, n: usize) -> bool {
        matches!(self.tok_at(n), Tok::Name(w) if w == "steps" || w == "alloc")
    }

    /// `cost steps <= A, alloc <= B`, after the `cost` word.
    fn cost(&mut self) -> P<Contract> {
        let (mut steps, mut alloc) = (None, None);
        loop {
            if !self.cost_bound_at(0) {
                return self.unexpected("`steps` or `alloc`");
            }
            let w = self.name()?;
            self.expect_op("<=")?;
            let e = self.expr()?;
            let slot = if w == "steps" { &mut steps } else { &mut alloc };
            if slot.replace(e).is_some() {
                return self.err("E106", format!("`cost` bounds `{w}` twice"));
            }
            if !self.eat_op(",") {
                break;
            }
        }
        self.expect_newline()?;
        Ok(Contract::Cost { steps, alloc })
    }

    fn expect_newline_then_indent(&mut self) -> P<()> {
        if !matches!(self.tok(), Tok::Newline) {
            return self.unexpected("end of line");
        }
        self.bump();
        if !matches!(self.tok(), Tok::Indent) {
            return self.unexpected("an indented block");
        }
        self.bump();
        Ok(())
    }

    fn stmts_until_dedent(&mut self) -> P<Vec<Stmt>> {
        let mut out = Vec::new();
        while !matches!(self.tok(), Tok::Dedent | Tok::Eof) {
            if let Some(s) = self.stmt()? {
                out.append(&mut self.hoisted);
                out.push(s);
            }
        }
        if matches!(self.tok(), Tok::Dedent) {
            self.bump();
        }
        Ok(out)
    }

    fn class_rest(&mut self, is_pub: bool) -> P<Record> {
        let name = self.name()?;
        let tparams = self.tparams()?;
        if !self.strict && self.is_op("(") && self.is_kw_at(1, "Exception") && self.is_op_at(2, ")")
        {
            let at = self.toks[self.pos + 1].span;
            self.note("N20", at, "base class `Exception` dropped");
            self.bump();
            self.bump();
            self.bump();
        }
        self.expect_op(":")?;
        self.expect_newline_then_indent()?;
        let doc = self.docstring();
        let mut fields = Vec::new();
        if self.eat_kw("pass") {
            self.expect_newline()?;
        } else {
            while !matches!(self.tok(), Tok::Dedent | Tok::Eof) {
                if !self.strict && self.is_kw("def") {
                    self.method(&name, &tparams, is_pub)?;
                    continue;
                }
                fields.push(self.field()?);
                self.expect_newline()?;
            }
        }
        if matches!(self.tok(), Tok::Dedent) {
            self.bump();
        }
        Ok(Record {
            name,
            tparams,
            doc,
            fields,
        })
    }

    /// N09: a method of class `class` becomes a top-level function taking `self: C`.
    fn method(&mut self, class: &str, tparams: &[TParam], is_pub: bool) -> P<()> {
        let span = self.span();
        self.bump();
        let args = tparams
            .iter()
            .map(|t| match t {
                TParam::Type { name, .. } | TParam::Value { name, .. } => TArg::Type(Type::Named {
                    path: vec![name.clone()],
                    args: Vec::new(),
                }),
            })
            .collect();
        self.self_ty = Some(Type::Named {
            path: vec![class.to_owned()],
            args,
        });
        let f = self.funcdef_rest(false);
        self.self_ty = None;
        let mut f = f?;
        self.note(
            "N09",
            span,
            format!(
                "method `{}` lifted to a top-level function taking `self: {class}`",
                f.name
            ),
        );
        f.tparams.splice(0..0, tparams.iter().cloned());
        self.lifted.push(Item {
            decorators: Vec::new(),
            is_pub,
            span,
            name_span: f.name_span,
            kind: ItemKind::Func(f),
        });
        Ok(())
    }

    fn field(&mut self) -> P<Field> {
        let name = self.name()?;
        self.expect_op(":")?;
        let start = self.span();
        let ty = self.ty()?;
        let ty_span = Span {
            end_line: self.toks[self.pos - 1].span.end_line,
            end_col: self.toks[self.pos - 1].span.end_col,
            ..start
        };
        let default = if self.eat_op("=") {
            Some(self.expr()?)
        } else {
            None
        };
        Ok(Field {
            decorators: Vec::new(),
            name,
            ty,
            ty_span,
            default,
        })
    }

    fn typedef_rest(&mut self) -> P<TypeDef> {
        let name = self.name()?;
        let tparams = self.tparams()?;
        self.expect_op("=")?;
        let rhs = if self.soft("uses") {
            self.bump();
            TypeRhs::Caps(self.caprefs()?)
        } else if self.is_kw("newtype") && self.is_op_at(1, "[") {
            self.bump();
            self.bump();
            let t = self.ty()?;
            self.expect_op("]")?;
            TypeRhs::Newtype(t)
        } else {
            let mut variants = Vec::new();
            let mut others = Vec::new();
            let mut first_other = None;
            loop {
                if matches!(self.tok(), Tok::Name(n) if is_type_name(n)) && self.is_op_at(1, "(") {
                    let vname = self.name()?;
                    self.bump();
                    let mut fields = Vec::new();
                    while !self.is_op(")") {
                        fields.push(self.field()?);
                        if !self.eat_op(",") {
                            break;
                        }
                    }
                    self.expect_op(")")?;
                    variants.push(Variant {
                        decorators: Vec::new(),
                        name: vname,
                        fields,
                    });
                } else {
                    first_other.get_or_insert(self.span());
                    others.push(self.base_ty()?);
                }
                if !self.eat_op("|") {
                    break;
                }
            }
            match (variants.is_empty(), others.is_empty()) {
                (false, true) => TypeRhs::Sum(variants),
                (true, _) => TypeRhs::Alias(others),
                (false, false) => {
                    let at = first_other.expect("a non-variant alternative was seen");
                    self.error(
                        "E232",
                        at,
                        "a sum mixes inline variants with other alternatives",
                    );
                    TypeRhs::Sum(variants)
                }
            }
        };
        let where_ = if self.eat_kw("where") {
            Some(self.expr()?)
        } else {
            None
        };
        self.expect_newline()?;
        Ok(TypeDef {
            name,
            tparams,
            rhs,
            where_,
        })
    }

    fn trait_rest(&mut self) -> P<Trait> {
        let name = self.name()?;
        let tparams = self.tparams()?;
        self.expect_op(":")?;
        self.expect_newline_then_indent()?;
        let doc = self.docstring();
        let mut assoc = Vec::new();
        while self.is_kw("type")
            && matches!(self.tok_at(1), Tok::Name(_))
            && matches!(self.tok_at(2), Tok::Newline)
        {
            self.bump();
            assoc.push(self.name()?);
            self.expect_newline()?;
        }
        let mut sigs = Vec::new();
        while !matches!(self.tok(), Tok::Dedent | Tok::Eof) {
            let mut decs = Vec::new();
            while self.eat_op("@") {
                decs.push(self.decorator_rest()?);
            }
            self.expect_kw("def")?;
            sigs.push((decs, self.funcdef_rest(false)?));
        }
        if matches!(self.tok(), Tok::Dedent) {
            self.bump();
        }
        Ok(Trait {
            name,
            tparams,
            doc,
            assoc,
            sigs,
        })
    }

    fn impl_rest(&mut self) -> P<Impl> {
        let trait_ = self.path()?;
        let args = if self.eat_op("[") {
            self.types_until("]")?
        } else {
            Vec::new()
        };
        self.expect_kw("for")?;
        let for_ty = self.ty()?;
        self.expect_op(":")?;
        self.expect_newline_then_indent()?;
        let mut assoc = Vec::new();
        while self.is_kw("type") && matches!(self.tok_at(1), Tok::Name(_)) && self.is_op_at(2, "=")
        {
            self.bump();
            let n = self.name()?;
            self.bump();
            assoc.push((n, self.ty()?));
            self.expect_newline()?;
        }
        let mut fns = Vec::new();
        while !matches!(self.tok(), Tok::Dedent | Tok::Eof) {
            self.expect_kw("def")?;
            fns.push(self.funcdef_rest(false)?);
        }
        if matches!(self.tok(), Tok::Dedent) {
            self.bump();
        }
        Ok(Impl {
            trait_,
            args,
            for_ty,
            assoc,
            fns,
        })
    }

    // ---- types ----

    fn ty(&mut self) -> P<Type> {
        self.node(Kind::Type, Self::ty_)
    }

    fn ty_(&mut self) -> P<Type> {
        let at = self.span();
        let base = self.base_ty()?;
        if self.is_op("|") && self.is_kw_at(1, "None") {
            self.bump();
            self.bump();
            if self.is_op("|") && self.is_kw_at(1, "None") {
                self.bump();
                let end = self.span();
                self.bump();
                let span = Span {
                    end_line: end.end_line,
                    end_col: end.end_col,
                    ..at
                };
                self.error(
                    "E276",
                    span,
                    format!(
                        "the union lists `None` twice; name the inner optional with `type A = {} | None` and write `A | None`",
                        crate::printer::ty(&base)
                    ),
                );
                return Ok(option(option(base)));
            }
            return Ok(option(base));
        }
        Ok(base)
    }

    fn base_ty(&mut self) -> P<Type> {
        let at = self.span();
        if self.eat_kw("None") {
            return Ok(Type::None);
        }
        if !self.strict
            && let Tok::Str { prefix, value, .. } = self.tok()
            && prefix.is_empty()
            && !value.is_empty()
            && value.split('.').all(|n| {
                n.starts_with(|c: char| c.is_alphabetic() || c == '_')
                    && n.chars().all(|c| c.is_alphanumeric() || c == '_')
            })
        {
            let path = value.split('.').map(str::to_owned).collect();
            self.note("N19", at, "quoted forward reference unquoted");
            self.bump();
            return Ok(Type::Named {
                path,
                args: Vec::new(),
            });
        }
        let par = self.is_kw("par") && self.is_kw_at(1, "Callable");
        if par {
            self.bump();
        }
        if self.is_kw("Callable") && self.is_op_at(1, "[") {
            self.bump();
            self.bump();
            self.expect_op("[")?;
            let params = self.types_until("]")?;
            self.expect_op(",")?;
            let ret = Box::new(self.ty()?);
            let (mut fail, mut uses) = (Vec::new(), Vec::new());
            while self.eat_op(",") {
                if self.eat_kw("fail") {
                    self.expect_op("(")?;
                    fail = self.errtype()?;
                    self.expect_op(")")?;
                } else {
                    self.expect_kw("uses")?;
                    self.expect_op("(")?;
                    uses.push(self.name()?);
                    while self.eat_op(",") {
                        uses.push(self.name()?);
                    }
                    self.expect_op(")")?;
                }
            }
            self.expect_op("]")?;
            return Ok(Type::Callable {
                par,
                params,
                ret,
                fail,
                uses,
            });
        }
        let mut path = self.path()?;
        if self.is_op("(") {
            let args = self.args()?;
            return Ok(Type::Call { path, args });
        }
        let mut args = Vec::new();
        if self.eat_op("[") {
            while !self.is_op("]") {
                args.push(self.targ()?);
                if !self.eat_op(",") {
                    break;
                }
            }
            self.expect_op("]")?;
        }
        if !self.strict {
            let rename = match path.as_slice() {
                [n] if n == "int64" => Some(("N27", "int")),
                [n] if n == "float64" => Some(("N27", "float")),
                [n] if n == "List" => Some(("N04", "list")),
                [n] if n == "Dict" => Some(("N04", "dict")),
                [n] if n == "Set" => Some(("N04", "set")),
                [n] if n == "Tuple" => Some(("N04", "tuple")),
                [n] if n == "Optional" && args.len() == 1 => Some(("N04", "")),
                _ => None,
            };
            if let Some((code, to)) = rename {
                self.note(code, at, format!("`{}` normalized", path.join(".")));
                if to.is_empty() {
                    let TArg::Type(t) = args.remove(0) else {
                        return Ok(Type::Named { path, args });
                    };
                    return Ok(option(t));
                }
                path = vec![to.to_owned()];
            }
        }
        self.builtin(&mut path, args.len(), at);
        Ok(Type::Named { path, args })
    }

    /// Respells a built-in type name as HIR does, in Yan (16.3).
    fn builtin(&mut self, path: &mut Path, arity: usize, at: Span) {
        let [n] = path.as_slice() else {
            return;
        };
        let yan = match n.as_str() {
            "int64" => Some("i64"),
            "float64" => Some("f64"),
            n => type_row(n, Surface::Yag)
                .filter(|r| r.arity == arity)
                .map(|r| r.yir),
        };
        if let Some(y) = yan {
            *path = vec![y.to_owned()];
        } else if n == "Option" || TYPES.iter().any(|r| r.yir == n && r.yag != n) {
            // Yan Core's own spelling would otherwise pass for Yan Agent's.
            self.error("E200", at, format!("cannot find type `{n}`"));
        }
    }

    fn targ(&mut self) -> P<TArg> {
        if let Tok::Int(s) = self.tok() {
            let s = s.clone();
            self.bump();
            return Ok(TArg::Int(s));
        }
        let t = self.ty()?;
        if self.is_op("|") || self.is_op("*") || self.is_op("/") || self.is_op("**") {
            let mut text = crate::printer::ty(&t);
            while let Tok::Op(o @ ("|" | "*" | "/" | "**")) = self.tok() {
                let o = *o;
                self.bump();
                let rhs = match self.bump() {
                    Tok::Name(n) => n,
                    Tok::Int(n) => n,
                    _ => return self.unexpected("a name"),
                };
                if o == "**" {
                    text.push_str(&format!("**{rhs}"));
                } else {
                    text.push_str(&format!(" {o} {rhs}"));
                }
                while self.is_op(".") {
                    self.bump();
                    text.push('.');
                    text.push_str(&self.name()?);
                }
            }
            return Ok(TArg::Text(text));
        }
        Ok(TArg::Type(t))
    }

    // ---- statements ----

    fn block(&mut self) -> P<Block> {
        self.node(Kind::Body, Self::block_)
    }

    fn block_(&mut self) -> P<Block> {
        if matches!(self.tok(), Tok::Newline) {
            self.expect_newline_then_indent()?;
            return Ok(Block {
                inline: false,
                stmts: self.stmts_until_dedent()?,
            });
        }
        let s = self.simple_stmt()?;
        self.expect_newline()?;
        let mut stmts = std::mem::take(&mut self.hoisted);
        stmts.push(s);
        Ok(Block {
            inline: true,
            stmts,
        })
    }

    fn lbody(&mut self) -> P<(Vec<LoopContract>, Vec<Stmt>)> {
        self.expect_newline_then_indent()?;
        let mut cs = Vec::new();
        loop {
            if self.soft("invariant") {
                self.bump();
                cs.push(LoopContract::Invariant(self.expr()?));
            } else if self.soft("decreases") {
                self.bump();
                cs.push(LoopContract::Decreases(self.expr()?));
            } else {
                break;
            }
            self.expect_newline()?;
        }
        Ok((cs, self.stmts_until_dedent()?))
    }

    fn target(&mut self) -> P<Vec<Name>> {
        let mut out = Vec::new();
        loop {
            out.push(if self.eat_kw("_") {
                "_".to_owned()
            } else {
                self.name()?
            });
            if !self.eat_op(",") {
                return Ok(out);
            }
        }
    }

    /// One statement, or `None` for a rejected one that was reported and skipped.
    fn stmt(&mut self) -> P<Option<Stmt>> {
        self.node(Kind::Stmt, Self::stmt_)
    }

    fn stmt_(&mut self) -> P<Option<Stmt>> {
        let span = self.span();
        let word = match self.tok() {
            Tok::Name(n) => n.clone(),
            _ => String::new(),
        };
        if REJECTED_E120.contains(&word.as_str()) {
            self.error(
                "E120",
                span,
                format!("`{word}` is not Yan Agent; use `fail` and `Result`"),
            );
            self.skip_stmt();
            while ["except", "finally", "else"].iter().any(|w| self.is_kw(w)) {
                self.skip_stmt();
            }
            return Ok(None);
        }
        if REJECTED_E121.contains(&word.as_str()) {
            self.error("E121", span, format!("`{word}` is not part of Yan Agent"));
            self.skip_stmt();
            return Ok(None);
        }
        if CONTRACT_WORDS.contains(&word.as_str())
            && matches!(
                self.tok_at(1),
                Tok::Name(_) | Tok::Int(_) | Tok::Float(_) | Tok::Str { .. } | Tok::Op("-" | "[")
            )
        {
            self.error(
                "E106",
                span,
                format!("`{word}` after the first statement; contracts open the body"),
            );
            self.skip_stmt();
            return Ok(None);
        }
        let kind = match word.as_str() {
            "if" => {
                self.bump();
                let mut arms = vec![(self.expr()?, self.colon_block()?)];
                let mut els = None;
                loop {
                    if self.eat_kw("elif") {
                        arms.push((self.expr()?, self.colon_block()?));
                    } else if self.eat_kw("else") {
                        els = Some(self.colon_block()?);
                        break;
                    } else {
                        break;
                    }
                }
                StmtKind::If { arms, els }
            }
            "while" => {
                self.bump();
                let cond = self.expr()?;
                self.expect_op(":")?;
                let (contracts, body) = self.lbody()?;
                StmtKind::While {
                    label: None,
                    cond,
                    contracts,
                    body,
                }
            }
            "for" => {
                self.bump();
                let target = self.target()?;
                self.expect_kw("in")?;
                let iter = self.expr()?;
                self.expect_op(":")?;
                let (contracts, body) = self.lbody()?;
                StmtKind::For {
                    label: None,
                    target,
                    iter,
                    contracts,
                    body,
                }
            }
            "match" if self.soft("match") && !self.is_op_at(1, "(") && !self.is_op_at(1, ".") => {
                self.bump();
                let scrutinee = self.expr()?;
                self.expect_op(":")?;
                self.expect_newline_then_indent()?;
                let mut cases = Vec::new();
                while self.eat_kw("case") {
                    let span = self.span();
                    let pat = self.pattern()?;
                    let guard = if self.eat_kw("if") {
                        Some(self.expr()?)
                    } else {
                        None
                    };
                    cases.push(Case {
                        pat,
                        span,
                        guard,
                        body: self.colon_block()?,
                    });
                }
                if !matches!(self.tok(), Tok::Dedent) {
                    return self.unexpected("`case`");
                }
                self.bump();
                StmtKind::Match { scrutinee, cases }
            }
            "with" => {
                self.bump();
                let kind =
                    if (self.is_kw("deadline") || self.is_kw("timeout")) && self.is_op_at(1, "(") {
                        let deadline = self.is_kw("deadline");
                        self.bump();
                        self.bump();
                        let e = self.expr()?;
                        self.expect_op(")")?;
                        if deadline {
                            WithKind::Deadline(e)
                        } else {
                            WithKind::Timeout(e)
                        }
                    } else if self.is_kw("supervisor") && self.is_op_at(1, "(") {
                        self.bump();
                        self.bump();
                        let e = self.expr()?;
                        self.expect_op(")")?;
                        WithKind::Supervisor(e)
                    } else {
                        let n = self.name()?;
                        self.expect_op("=")?;
                        WithKind::Bind(n, self.expr()?)
                    };
                StmtKind::With {
                    kind,
                    body: self.colon_block()?,
                }
            }
            "par" if !self.par_expr_follows() => {
                self.bump();
                let concurrent = self.eat_kw("concurrent");
                let limit = if self.eat_op("(") {
                    self.expect_kw("limit")?;
                    self.expect_op("=")?;
                    let e = self.expr()?;
                    self.expect_op(")")?;
                    Some(e)
                } else {
                    None
                };
                let body = if self.eat_kw("for") {
                    let target = self.target()?;
                    self.expect_kw("in")?;
                    let iter = self.expr()?;
                    self.expect_op(":")?;
                    let (contracts, body) = self.lbody()?;
                    ParBody::For {
                        target,
                        iter,
                        contracts,
                        body,
                    }
                } else {
                    self.expect_op(":")?;
                    self.expect_newline_then_indent()?;
                    ParBody::Block(self.stmts_until_dedent()?)
                };
                StmtKind::Par {
                    concurrent,
                    limit,
                    body,
                }
            }
            _ => {
                let s = self.simple_stmt()?;
                self.expect_newline()?;
                return Ok(Some(s));
            }
        };
        Ok(Some(Stmt { kind, span }))
    }

    fn colon_block(&mut self) -> P<Block> {
        self.expect_op(":")?;
        self.block()
    }

    fn par_expr_follows(&self) -> bool {
        let n = if self.is_kw_at(1, "concurrent") { 2 } else { 1 };
        self.is_op_at(n, "[") || self.is_op_at(n, "{")
    }

    fn simple_stmt(&mut self) -> P<Stmt> {
        let outer = std::mem::replace(&mut self.in_simple, true);
        let s = self.simple();
        self.in_simple = outer;
        s
    }

    fn simple(&mut self) -> P<Stmt> {
        let span = self.span();
        let word = match self.tok() {
            Tok::Name(n) => n.clone(),
            _ => String::new(),
        };
        let kind = match word.as_str() {
            "return" => {
                self.bump();
                let ends = matches!(self.tok(), Tok::Newline | Tok::Dedent | Tok::Eof);
                StmtKind::Return(if ends { None } else { Some(self.exprs()?) })
            }
            "become" => {
                self.bump();
                StmtKind::Become(self.postfix()?)
            }
            "fail" => {
                self.bump();
                StmtKind::Fail(self.expr()?)
            }
            "break" => {
                self.bump();
                StmtKind::Break
            }
            "continue" => {
                self.bump();
                StmtKind::Continue
            }
            "pass" => {
                self.bump();
                StmtKind::Pass
            }
            "assert" => {
                self.bump();
                StmtKind::Assert(self.expr()?)
            }
            "unreachable" => {
                self.bump();
                StmtKind::Unreachable
            }
            "reveal" if self.soft("reveal") && matches!(self.tok_at(1), Tok::Name(_)) => {
                self.bump();
                StmtKind::Reveal(self.path()?)
            }
            "ghost" if self.soft("ghost") && matches!(self.tok_at(1), Tok::Name(_)) => {
                self.bump();
                let name = if self.eat_kw("_") {
                    "_".to_owned()
                } else {
                    self.name()?
                };
                let ty = if self.eat_op(":") {
                    Some(self.ty()?)
                } else {
                    None
                };
                self.expect_op("=")?;
                StmtKind::Ghost {
                    name,
                    ty,
                    value: self.expr()?,
                }
            }
            "var" | "let"
                if matches!(self.tok_at(1), Tok::Name(_))
                    && (self.is_op_at(2, "=") || self.is_op_at(2, ":")) =>
            {
                self.note(
                    "N14",
                    span,
                    format!("`{word}` on a local removed; mutability is inferred"),
                );
                self.bump();
                return self.simple();
            }
            _ => {
                let target = self.exprs()?;
                if self.eat_op(":") {
                    let ty = self.ty()?;
                    self.expect_op("=")?;
                    StmtKind::Assign {
                        target,
                        ty: Some(ty),
                        value: self.exprs()?,
                    }
                } else if self.eat_op("=") {
                    StmtKind::Assign {
                        target,
                        ty: None,
                        value: self.exprs()?,
                    }
                } else if let Some(op) = AUGOPS.iter().find(|a| self.is_op(a)) {
                    self.bump();
                    StmtKind::AugAssign {
                        target,
                        op,
                        value: self.expr()?,
                    }
                } else {
                    StmtKind::Expr(target)
                }
            }
        };
        Ok(Stmt { kind, span })
    }

    // ---- patterns ----

    fn pattern(&mut self) -> P<Pat> {
        self.node(Kind::Pattern, Self::pattern_)
    }

    fn pattern_(&mut self) -> P<Pat> {
        let mut p = self.pattern_one()?;
        while self.eat_kw("as") {
            p = Pat::As(Box::new(p), self.name()?);
        }
        Ok(p)
    }

    fn pattern_one(&mut self) -> P<Pat> {
        if self.eat_kw("_") {
            return Ok(Pat::Wild);
        }
        if self.is_op("(") {
            self.bump();
            let mut ps = vec![self.pattern()?];
            while self.eat_op(",") {
                if self.is_op(")") {
                    break;
                }
                ps.push(self.pattern()?);
            }
            self.expect_op(")")?;
            return Ok(Pat::Tuple(ps));
        }
        if matches!(
            self.tok(),
            Tok::Int(_) | Tok::Float(_) | Tok::Str { .. } | Tok::Char(_) | Tok::Op("-")
        ) || self.is_kw("True")
            || self.is_kw("False")
            || self.is_kw("None")
        {
            return Ok(Pat::Lit(self.unary()?));
        }
        let path = self.path()?;
        let args = if self.eat_op("(") {
            let mut args = Vec::new();
            while !self.is_op(")") {
                let name = if matches!(self.tok(), Tok::Name(_)) && self.is_op_at(1, "=") {
                    let n = self.name()?;
                    self.bump();
                    Some(n)
                } else {
                    None
                };
                args.push((name, self.pattern()?));
                if !self.eat_op(",") {
                    break;
                }
            }
            self.expect_op(")")?;
            Some(args)
        } else {
            None
        };
        Ok(Pat::Path { path, args })
    }

    // ---- expressions ----

    fn exprs(&mut self) -> P<Expr> {
        let first = self.expr()?;
        if !self.is_op(",") {
            return Ok(first);
        }
        let span = first.span;
        let mut xs = vec![first];
        while self.eat_op(",") {
            if matches!(self.tok(), Tok::Newline | Tok::Op("=" | ":")) {
                break;
            }
            xs.push(self.expr()?);
        }
        Ok(Expr {
            kind: ExprKind::Tuple(xs),
            span,
            paren: false,
        })
    }

    fn expr(&mut self) -> P<Expr> {
        self.node(Kind::Expr, Self::expr_)
    }

    fn expr_(&mut self) -> P<Expr> {
        if self.is_kw("lambda") {
            let span = self.span();
            self.bump();
            let mut ps = Vec::new();
            while !self.is_op(":") {
                ps.push(self.name()?);
                if !self.eat_op(",") {
                    break;
                }
            }
            self.expect_op(":")?;
            let body = self.expr()?;
            return Ok(Expr {
                kind: ExprKind::Lambda(ps, Box::new(body)),
                span,
                paren: false,
            });
        }
        self.fallback()
    }

    fn fallback(&mut self) -> P<Expr> {
        let lhs = self.cond()?;
        if !self.is_kw("else") {
            return Ok(lhs);
        }
        let span = lhs.span;
        self.bump();
        if matches!(lhs.kind, ExprKind::Cond { .. }) && !lhs.paren {
            self.mix(span, "a conditional expression beside a fallback `else`");
        }
        let fb = if matches!(self.tok(), Tok::Name(_)) && self.is_op_at(1, "->") {
            let n = self.name()?;
            self.bump();
            Fallback::Bind(n, self.cond()?)
        } else if self.eat_kw("return") {
            let ends = matches!(
                self.tok(),
                Tok::Newline | Tok::Dedent | Tok::Eof | Tok::Op(")" | "]" | "}" | ",")
            );
            Fallback::Return(if ends { None } else { Some(self.cond()?) })
        } else if self.eat_kw("fail") {
            Fallback::Fail(self.cond()?)
        } else if self.eat_kw("break") {
            Fallback::Break
        } else if self.eat_kw("continue") {
            Fallback::Continue
        } else {
            let v = self.cond()?;
            let arith = op_of(&v).is_some_and(|o| ARITH.contains(&o) || BITWISE.contains(&o));
            if arith || matches!(v.kind, ExprKind::Cond { .. }) && !v.paren {
                self.mix(
                    span,
                    "an operator expression as the right side of a fallback `else`",
                );
            }
            Fallback::Value(v)
        };
        Ok(Expr {
            kind: ExprKind::Fallback(Box::new(lhs), Box::new(fb)),
            span,
            paren: false,
        })
    }

    fn cond(&mut self) -> P<Expr> {
        let then = self.disj()?;
        if !self.is_kw("if") {
            return Ok(then);
        }
        let span = then.span;
        self.bump();
        let cond = self.disj()?;
        self.expect_kw("else")?;
        let els = self.cond()?;
        Ok(Expr {
            kind: ExprKind::Cond {
                then: Box::new(then),
                cond: Box::new(cond),
                els: Box::new(els),
            },
            span,
            paren: false,
        })
    }

    fn disj(&mut self) -> P<Expr> {
        let mut l = self.conj()?;
        while self.eat_kw("or") {
            let r = self.conj()?;
            if op_of(&l) == Some("and") || op_of(&r) == Some("and") {
                self.mix(l.span, "`and` and `or` in one expression");
            }
            l = bin("or", l, r);
        }
        Ok(l)
    }

    fn conj(&mut self) -> P<Expr> {
        let mut l = self.neg()?;
        while self.eat_kw("and") {
            let r = self.neg()?;
            l = bin("and", l, r);
        }
        Ok(l)
    }

    fn neg(&mut self) -> P<Expr> {
        if !self.is_kw("not") {
            return self.comp();
        }
        let span = self.span();
        self.bump();
        let e = self.neg()?;
        if !self.strict
            && !e.paren
            && let ExprKind::Compare(op @ ("in" | "is"), l, r) = &e.kind
        {
            let to = if *op == "in" { "not in" } else { "is not" };
            self.note(
                "N28",
                span,
                format!("`not ... {op} ...` normalized to `{to}`"),
            );
            return Ok(Expr {
                kind: ExprKind::Compare(to, l.clone(), r.clone()),
                span,
                paren: false,
            });
        }
        Ok(Expr {
            kind: ExprKind::Not(Box::new(e)),
            span,
            paren: false,
        })
    }

    fn compop(&mut self) -> Option<&'static str> {
        let op = match self.tok() {
            Tok::Op(o @ ("==" | "!=" | "<" | "<=" | ">" | ">=")) => *o,
            Tok::Name(n) if n == "in" => "in",
            Tok::Name(n) if n == "not" && self.is_kw_at(1, "in") => {
                self.bump();
                "not in"
            }
            Tok::Name(n) if n == "is" => {
                if self.is_kw_at(1, "not") {
                    self.bump();
                    "is not"
                } else {
                    "is"
                }
            }
            _ => return None,
        };
        self.bump();
        Some(op)
    }

    fn comp(&mut self) -> P<Expr> {
        let l = self.bitor()?;
        let Some(mut op) = self.compop() else {
            return Ok(l);
        };
        let span = l.span;
        let r = self.bitor()?;
        for side in [&l, &r] {
            if op_of(side).is_some_and(|o| BITWISE.contains(&o)) {
                self.mix(span, "a bitwise operator beside a comparison");
                break;
            }
        }
        let mut r = r;
        if matches!(r.kind, ExprKind::Lit(Lit::None)) && !self.strict && (op == "==" || op == "!=")
        {
            let to = if op == "==" { "is" } else { "is not" };
            self.note(
                "N10",
                span,
                format!("`{op} None` normalized to `{to} None`"),
            );
            op = to;
        }
        let mut e = Expr {
            kind: ExprKind::Compare(op, Box::new(l), Box::new(r.clone())),
            span,
            paren: false,
        };
        if let Some(op2) = self.compop() {
            let rr = self.bitor()?;
            let simple_mid = matches!(r.kind, ExprKind::Name(_) | ExprKind::Lit(_));
            if simple_mid && !self.strict {
                self.note("N22", span, "chained comparison rewritten with `and`");
            } else {
                self.error("E104", span, "chained comparison; write `a < b and b < c`");
            }
            r.paren = false;
            let second = Expr {
                kind: ExprKind::Compare(op2, Box::new(r), Box::new(rr)),
                span,
                paren: false,
            };
            e = bin("and", e, second);
            while self.compop().is_some() {
                self.bitor()?;
            }
        }
        Ok(e)
    }

    fn bitwise_level(&mut self, ops: &[&'static str], next: fn(&mut Self) -> P<Expr>) -> P<Expr> {
        let mut l = next(self)?;
        while let Tok::Op(o) = self.tok() {
            let Some(op) = ops.iter().find(|x| **x == *o).copied() else {
                break;
            };
            self.bump();
            let r = next(self)?;
            for side in [&l, &r] {
                if let Some(o2) = op_of(side)
                    && o2 != op
                    && (BITWISE.contains(&o2) || ARITH.contains(&o2))
                {
                    self.mix(l.span, "a bitwise operator mixed with another operator");
                    break;
                }
            }
            l = bin(op, l, r);
        }
        Ok(l)
    }

    fn bitor(&mut self) -> P<Expr> {
        self.bitwise_level(&["|"], Self::bitxor)
    }

    fn bitxor(&mut self) -> P<Expr> {
        self.bitwise_level(&["^"], Self::bitand)
    }

    fn bitand(&mut self) -> P<Expr> {
        self.bitwise_level(&["&"], Self::shift)
    }

    fn shift(&mut self) -> P<Expr> {
        self.bitwise_level(&["<<", ">>"], Self::arith)
    }

    fn arith(&mut self) -> P<Expr> {
        let mut l = self.term()?;
        while let Tok::Op(o @ ("+" | "-")) = self.tok() {
            let op = *o;
            self.bump();
            let r = self.term()?;
            l = bin(op, l, r);
        }
        Ok(l)
    }

    fn term(&mut self) -> P<Expr> {
        let mut l = self.unary()?;
        while let Tok::Op(o @ ("*" | "/" | "//" | "%" | "@")) = self.tok() {
            let op = *o;
            self.bump();
            let r = self.unary()?;
            if let Some(lo) = op_of(&l)
                && ["*", "/", "//", "%"].contains(&lo)
                && lo != op
                && (lo == "%" || op == "%")
            {
                self.mix(l.span, "`%` combined with `*`, `/` or `//`");
            }
            l = bin(op, l, r);
        }
        Ok(l)
    }

    fn unary(&mut self) -> P<Expr> {
        if let Tok::Op(o @ ("-" | "+" | "~")) = self.tok() {
            let op = *o;
            let span = self.span();
            self.bump();
            let e = self.unary()?;
            if op == "-" && op_of(&e) == Some("**") {
                self.mix(span, "unary minus on a `**` operand");
            }
            if op == "-"
                && let ExprKind::Lit(Lit::Int(s) | Lit::Float(s)) = &e.kind
                && !e.paren
            {
                let neg = format!("-{s}");
                let lit = if matches!(e.kind, ExprKind::Lit(Lit::Int(_))) {
                    Lit::Int(neg)
                } else {
                    Lit::Float(neg)
                };
                return Ok(Expr {
                    kind: ExprKind::Lit(lit),
                    span,
                    paren: false,
                });
            }
            return Ok(Expr {
                kind: ExprKind::Unary(op, Box::new(e)),
                span,
                paren: false,
            });
        }
        self.power()
    }

    fn power(&mut self) -> P<Expr> {
        let l = self.postfix()?;
        if self.eat_op("**") {
            let r = self.unary()?;
            return Ok(bin("**", l, r));
        }
        Ok(l)
    }

    fn postfix(&mut self) -> P<Expr> {
        let mut e = self.atom()?;
        loop {
            let span = e.span;
            if self.is_op(".") && matches!(self.tok_at(1), Tok::Name(_)) {
                self.bump();
                let at = self.span();
                let n = self.name()?;
                e = Expr {
                    kind: ExprKind::Attr(Box::new(e), n, at),
                    span,
                    paren: false,
                };
            } else if self.is_op("(") {
                let args = self.args()?;
                e = self.lenient_call(e, args, span);
            } else if self.is_op("[") {
                self.bump();
                e = self.subscript(e)?;
                self.expect_op("]")?;
            } else if self.eat_op("?") {
                e = Expr {
                    kind: ExprKind::Try(Box::new(e)),
                    span,
                    paren: false,
                };
            } else {
                return Ok(e);
            }
        }
    }

    /// N17: `d.get(k, v)` becomes `d.get(k) else v`.
    fn lenient_call(&mut self, f: Expr, mut args: Vec<Arg>, span: Span) -> Expr {
        let get = matches!(&f.kind, ExprKind::Attr(_, n, _) if n == "get");
        if get
            && !self.strict
            && args.len() == 2
            && args
                .iter()
                .all(|a| a.name.is_none() && matches!(a.kind, ArgKind::Expr(_)))
        {
            self.note("N17", span, "`d.get(k, v)` normalized to `d.get(k) else v`");
            let ArgKind::Expr(dflt) = args.pop().expect("two arguments").kind else {
                unreachable!("checked above")
            };
            let call = Expr {
                kind: ExprKind::Call(Box::new(f), args),
                span,
                paren: false,
            };
            return Expr {
                kind: ExprKind::Fallback(Box::new(call), Box::new(Fallback::Value(dflt))),
                span,
                paren: true,
            };
        }
        Expr {
            kind: ExprKind::Call(Box::new(f), args),
            span,
            paren: false,
        }
    }

    fn subscript(&mut self, base: Expr) -> P<Expr> {
        let span = base.span;
        let opt = |p: &mut Self| -> P<Option<Box<Expr>>> {
            if p.is_op(":") || p.is_op("]") {
                Ok(None)
            } else {
                Ok(Some(Box::new(p.expr()?)))
            }
        };
        let first = opt(self)?;
        if !self.is_op(":") {
            let Some(idx) = first else {
                return self.unexpected("an index");
            };
            let idx = *idx;
            if !self.strict
                && let ExprKind::Lit(Lit::Int(s)) = &idx.kind
                && s.starts_with('-')
            {
                self.note(
                    "N16",
                    span,
                    "negative literal index rewritten as `xs[len(xs) - k]`",
                );
            }
            return Ok(Expr {
                kind: ExprKind::Index(Box::new(base), Box::new(idx)),
                span,
                paren: false,
            });
        }
        self.bump();
        let hi = opt(self)?;
        let (step, has_step) = if self.eat_op(":") {
            (opt(self)?, true)
        } else {
            (None, false)
        };
        Ok(Expr {
            kind: ExprKind::Slice(Box::new(base), [first, hi, step], has_step),
            span,
            paren: false,
        })
    }

    fn args(&mut self) -> P<Vec<Arg>> {
        self.expect_op("(")?;
        let mut out = Vec::new();
        while !self.is_op(")") {
            let span = self.span();
            let name = if matches!(self.tok(), Tok::Name(_)) && self.is_op_at(1, "=") {
                let n = self.name()?;
                self.bump();
                Some(n)
            } else {
                None
            };
            let kind = if self.eat_op("&") {
                ArgKind::Ref(self.postfix()?)
            } else if self.eat_kw("become") {
                ArgKind::Become(self.postfix()?)
            } else {
                let e = self.expr()?;
                if self.is_kw("for") && out.is_empty() {
                    let c = self.comp_rest(e, None, CompKind::List, None, span)?;
                    if !self.is_op(")") {
                        return self.unexpected("`)`");
                    }
                    ArgKind::Expr(c)
                } else {
                    ArgKind::Expr(e)
                }
            };
            out.push(Arg { name, kind, span });
            if !self.eat_op(",") {
                break;
            }
        }
        self.expect_op(")")?;
        Ok(out)
    }

    /// After the element of a comprehension, at `for`.
    fn comp_rest(
        &mut self,
        elem: Expr,
        value: Option<Expr>,
        kind: CompKind,
        par: Option<bool>,
        span: Span,
    ) -> P<Expr> {
        self.expect_kw("for")?;
        let target = self.target()?;
        self.expect_kw("in")?;
        let iter = self.disj()?;
        let cond = if self.eat_kw("if") {
            Some(Box::new(self.disj()?))
        } else {
            None
        };
        if self.is_kw("for")
            && !self.strict
            && self.in_simple
            && par.is_none()
            && matches!(kind, CompKind::List)
        {
            return self.multi_for(elem, (target, iter, cond.map(|c| *c)), span);
        }
        if self.is_kw("for") {
            self.error("E107", span, "a comprehension has exactly one `for` clause");
            while !matches!(
                self.tok(),
                Tok::Op("]" | "}" | ")") | Tok::Newline | Tok::Eof
            ) {
                self.bump();
            }
        }
        Ok(Expr {
            kind: ExprKind::Comp {
                kind,
                par,
                elem: Box::new(elem),
                value: value.map(Box::new),
                target,
                iter: Box::new(iter),
                cond,
            },
            span,
            paren: false,
        })
    }

    /// N45: the list comprehension `[e for a in A for b in B ...]` as nested `for` statements that
    /// fill a fresh name, hoisted before the statement; the comprehension reads as that name.
    fn multi_for(
        &mut self,
        elem: Expr,
        first: (Vec<Name>, Expr, Option<Expr>),
        span: Span,
    ) -> P<Expr> {
        let mut clauses = vec![first];
        while self.eat_kw("for") {
            let target = self.target()?;
            self.expect_kw("in")?;
            let iter = self.disj()?;
            let cond = if self.eat_kw("if") {
                Some(self.disj()?)
            } else {
                None
            };
            clauses.push((target, iter, cond));
        }
        self.comps += 1;
        let name = format!("_comp{}", self.comps);
        self.note("N45", span, "nested comprehension rewritten as loops");
        let mk = |kind| Expr {
            kind,
            span,
            paren: false,
        };
        let var = || mk(ExprKind::Name(name.clone()));
        let (init, fill) = (ExprKind::List(Vec::new()), method(var(), "append", elem));
        let mut body = Stmt { kind: fill, span };
        for (target, iter, cond) in clauses.into_iter().rev() {
            let mut inner = vec![body];
            if let Some(c) = cond {
                inner = vec![Stmt {
                    kind: StmtKind::If {
                        arms: vec![(
                            c,
                            Block {
                                inline: false,
                                stmts: inner,
                            },
                        )],
                        els: None,
                    },
                    span,
                }];
            }
            body = Stmt {
                kind: StmtKind::For {
                    label: None,
                    target,
                    iter,
                    contracts: Vec::new(),
                    body: inner,
                },
                span,
            };
        }
        self.hoisted.push(Stmt {
            kind: StmtKind::Assign {
                target: var(),
                ty: None,
                value: mk(init),
            },
            span,
        });
        self.hoisted.push(body);
        Ok(var())
    }

    fn atom(&mut self) -> P<Expr> {
        let span = self.span();
        let mk = |kind| Expr {
            kind,
            span,
            paren: false,
        };
        if self.is_kw("fail") {
            // `fail e` as an operand is the failed outcome, `Err(e)` (R424): `ex f(x) == fail E`.
            self.bump();
            let e = self.unary()?;
            let arg = Arg {
                name: None,
                span: e.span,
                kind: ArgKind::Expr(e),
            };
            return Ok(mk(ExprKind::Call(
                Box::new(mk(ExprKind::Name("Err".into()))),
                vec![arg],
            )));
        }
        let kind = match self.tok().clone() {
            Tok::Int(s) => ExprKind::Lit(Lit::Int(s)),
            Tok::Float(s) => ExprKind::Lit(Lit::Float(s)),
            Tok::Str { prefix, value, .. } => ExprKind::Lit(Lit::Str { prefix, value }),
            Tok::Char(c) => ExprKind::Lit(Lit::Char(c)),
            Tok::FStr { template, parts } => {
                self.bump();
                let mut pieces = Vec::new();
                for p in parts {
                    pieces.push(match p {
                        FPart::Lit(s) => FPiece::Lit(s),
                        FPart::Expr {
                            src,
                            line,
                            col,
                            fmt,
                        } => FPiece::Expr(self.sub_expr(&src, line, col)?, fmt),
                    });
                }
                return Ok(mk(ExprKind::FStr { template, pieces }));
            }
            Tok::Op("...") => ExprKind::Hole,
            Tok::Op("(") => {
                self.bump();
                if self.eat_op(")") {
                    return Ok(Expr {
                        kind: ExprKind::Tuple(Vec::new()),
                        span,
                        paren: true,
                    });
                }
                let first = self.expr()?;
                if self.eat_op(")") {
                    return Ok(Expr {
                        paren: true,
                        ..first
                    });
                }
                let mut xs = vec![first];
                while self.eat_op(",") {
                    if self.is_op(")") {
                        break;
                    }
                    xs.push(self.expr()?);
                }
                self.expect_op(")")?;
                return Ok(Expr {
                    kind: ExprKind::Tuple(xs),
                    span,
                    paren: true,
                });
            }
            Tok::Op("[") => return self.list_like(None, span),
            Tok::Op("{") => return self.brace_like(None, span),
            Tok::Name(n) => match n.as_str() {
                "True" => ExprKind::Lit(Lit::Bool(true)),
                "False" => ExprKind::Lit(Lit::Bool(false)),
                "None" => ExprKind::Lit(Lit::None),
                "_" => ExprKind::Wild,
                "par" => {
                    self.bump();
                    let concurrent = self.eat_kw("concurrent");
                    return if self.is_op("[") {
                        self.list_like(Some(concurrent), span)
                    } else if self.is_op("{") {
                        self.brace_like(Some(concurrent), span)
                    } else {
                        self.unexpected("a comprehension")
                    };
                }
                _ => {
                    let n = self.name()?;
                    return Ok(mk(ExprKind::Name(n)));
                }
            },
            _ => return self.unexpected("an expression"),
        };
        self.bump();
        Ok(mk(kind))
    }

    fn list_like(&mut self, par: Option<bool>, span: Span) -> P<Expr> {
        self.expect_op("[")?;
        if self.eat_op("]") {
            return Ok(Expr {
                kind: ExprKind::List(Vec::new()),
                span,
                paren: false,
            });
        }
        let first = self.expr()?;
        if self.is_kw("for") {
            let c = self.comp_rest(first, None, CompKind::List, par, span)?;
            self.expect_op("]")?;
            return Ok(c);
        }
        let mut xs = vec![first];
        while self.eat_op(",") {
            if self.is_op("]") {
                break;
            }
            xs.push(self.expr()?);
        }
        self.expect_op("]")?;
        Ok(Expr {
            kind: ExprKind::List(xs),
            span,
            paren: false,
        })
    }

    fn brace_like(&mut self, par: Option<bool>, span: Span) -> P<Expr> {
        self.expect_op("{")?;
        if self.eat_op("}") {
            return Ok(Expr {
                kind: ExprKind::Dict(Vec::new()),
                span,
                paren: false,
            });
        }
        let first = self.expr()?;
        if self.eat_op(":") {
            let v = self.expr()?;
            if self.is_kw("for") {
                let c = self.comp_rest(first, Some(v), CompKind::Dict, par, span)?;
                self.expect_op("}")?;
                return Ok(c);
            }
            let mut kvs = vec![(first, v)];
            while self.eat_op(",") {
                if self.is_op("}") {
                    break;
                }
                let k = self.expr()?;
                self.expect_op(":")?;
                kvs.push((k, self.expr()?));
            }
            self.expect_op("}")?;
            return Ok(Expr {
                kind: ExprKind::Dict(kvs),
                span,
                paren: false,
            });
        }
        if self.is_kw("for") {
            let c = self.comp_rest(first, None, CompKind::Set, par, span)?;
            self.expect_op("}")?;
            return Ok(c);
        }
        let mut xs = vec![first];
        while self.eat_op(",") {
            if self.is_op("}") {
                break;
            }
            xs.push(self.expr()?);
        }
        self.expect_op("}")?;
        Ok(Expr {
            kind: ExprKind::Set(xs),
            span,
            paren: false,
        })
    }

    /// Parse an f-string interpolation, with spans placed where its text sits in the file.
    fn sub_expr(&mut self, src: &str, line: u32, col: u32) -> P<Expr> {
        let lexed = lex(src);
        let shift = |mut s: Span| {
            if s.line == 1 {
                s.col += col - 1;
            }
            if s.end_line == 1 {
                s.end_col += col - 1;
            }
            s.line += line - 1;
            s.end_line += line - 1;
            s
        };
        let toks = lexed
            .tokens
            .into_iter()
            .filter(|t| !matches!(t.tok, Tok::Indent | Tok::Dedent))
            .map(|t| Token {
                span: shift(t.span),
                ..t
            })
            .collect();
        let mut sub = Parser {
            toks,
            pos: 0,
            strict: self.strict,
            issues: Vec::new(),
            lifted: Vec::new(),
            self_ty: None,
            nodes: Vec::new(),
            hoisted: Vec::new(),
            in_simple: false,
            comps: 0,
        };
        let r = sub.expr();
        self.issues.append(&mut sub.issues);
        let e = r?;
        if !matches!(sub.tok(), Tok::Newline | Tok::Eof) {
            return Err(PErr {
                code: "E100",
                span: sub.span(),
                message: "unexpected text in an f-string interpolation".into(),
            });
        }
        Ok(e)
    }
}

fn option(t: Type) -> Type {
    Type::Named {
        path: vec!["Option".into()],
        args: vec![TArg::Type(t)],
    }
}

/// `receiver.name(arg)` as a statement.
fn method(receiver: Expr, name: &str, arg: Expr) -> StmtKind {
    let span = arg.span;
    let callee = Expr {
        kind: ExprKind::Attr(Box::new(receiver), name.to_owned(), span),
        span,
        paren: false,
    };
    let arg = Arg {
        name: None,
        span,
        kind: ArgKind::Expr(arg),
    };
    StmtKind::Expr(Expr {
        kind: ExprKind::Call(Box::new(callee), vec![arg]),
        span,
        paren: false,
    })
}
