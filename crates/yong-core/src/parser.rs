//! Recursive-descent parser for `.yong` text (spec 3.5), recovering at item boundaries (22.3).

use yong_diag::{Diagnostic, Span};

use crate::ast::*;
use crate::lexer::{Tok, Token, lex};

const ITEM_STARTS: [&str; 12] = [
    "fn", "gen", "record", "sum", "newtype", "trait", "impl", "const", "test", "unsafe", "pub", "@",
];

struct Error {
    code: &'static str,
    at: usize,
    message: String,
}

type PResult<T> = Result<T, Error>;

struct Parser<'a> {
    src: &'a str,
    toks: Vec<Token>,
    pos: usize,
}

/// Parse one `.yong` file. Returns the module when the file parsed without errors.
pub fn parse(file: &str, src: &str) -> (Option<Module>, Vec<Diagnostic>) {
    let lexed = match lex(src) {
        Ok(l) => l,
        Err(e) => {
            let quote = &src[e.start..e.end.min(src.len())];
            return (
                None,
                vec![Diagnostic::error(e.code, file, e.span, quote, e.message)],
            );
        }
    };
    let mut p = Parser {
        src,
        toks: lexed.tokens,
        pos: 0,
    };
    let mut diags = Vec::new();
    let path = match p.module_header() {
        Ok(path) => path,
        Err(e) => return (None, vec![p.diagnostic(file, "", e)]),
    };
    let module_name = path.join(".");
    let mut items = Vec::new();
    while !p.at_eof() {
        let start = p.pos;
        match p.item() {
            Ok(item) => items.push(item),
            Err(e) => {
                let item = p.item_name_from(start, &module_name);
                diags.push(p.diagnostic(file, &item, e));
                p.recover();
            }
        }
    }
    let module = diags.is_empty().then_some(Module {
        header: lexed.header,
        path,
        items,
    });
    (module, diags)
}

fn describe(t: &Tok) -> String {
    match t {
        Tok::Name(n) => format!("`{n}`"),
        Tok::Int(s) | Tok::Float(s) => format!("number `{s}`"),
        Tok::Str(_) => "string".into(),
        Tok::Char(_) => "character".into(),
        Tok::Label(l) => format!("label `'{l}`"),
        Tok::Punct(p) => format!("`{p}`"),
        Tok::Eof => "end of file".into(),
    }
}

fn join_end(a: Span, b: Span) -> Span {
    Span {
        line: a.line,
        col: a.col,
        end_line: b.end_line,
        end_col: b.end_col,
    }
}

impl Parser<'_> {
    fn diagnostic(&self, file: &str, item: &str, e: Error) -> Diagnostic {
        let t = &self.toks[e.at];
        Diagnostic::error(e.code, file, t.span, &self.src[t.start..t.end], e.message)
            .with_item(item)
    }

    fn item_name_from(&self, start: usize, module: &str) -> String {
        let name = self.toks[start..self.pos.max(start + 1)]
            .windows(2)
            .find_map(|w| match (&w[0].tok, &w[1].tok) {
                (Tok::Name(k), Tok::Name(n)) if ITEM_STARTS.contains(&k.as_str()) => {
                    Some(n.clone())
                }
                _ => None,
            });
        match name {
            Some(n) => format!("{module}.{n}"),
            None => module.to_owned(),
        }
    }

    /// Skip to the next token that starts an item in column 1.
    fn recover(&mut self) {
        self.bump();
        while !self.at_eof() {
            let t = &self.toks[self.pos];
            let starts = match &t.tok {
                Tok::Name(n) => ITEM_STARTS.contains(&n.as_str()),
                Tok::Punct(p) => *p == "@",
                _ => false,
            };
            if starts && t.span.col == 1 {
                return;
            }
            self.bump();
        }
    }

    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn peek_at(&self, n: usize) -> &Tok {
        &self.toks[(self.pos + n).min(self.toks.len() - 1)].tok
    }

    fn at_eof(&self) -> bool {
        *self.peek() == Tok::Eof
    }

    fn bump(&mut self) -> Tok {
        let t = self.peek().clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn span(&self) -> Span {
        self.toks[self.pos].span
    }

    fn prev_span(&self) -> Span {
        self.toks[self.pos.saturating_sub(1)].span
    }

    fn same_line(&self) -> bool {
        self.pos > 0 && self.span().line == self.prev_span().end_line
    }

    fn unexpected<T>(&self, expected: &str) -> PResult<T> {
        Err(Error {
            code: "E100",
            at: self.pos,
            message: format!("unexpected {}; expected {expected}", describe(self.peek())),
        })
    }

    fn is_punct(&self, p: &str) -> bool {
        matches!(self.peek(), Tok::Punct(q) if *q == p)
    }

    fn is_kw(&self, k: &str) -> bool {
        matches!(self.peek(), Tok::Name(n) if n == k)
    }

    fn eat_punct(&mut self, p: &str) -> bool {
        let hit = self.is_punct(p);
        if hit {
            self.bump();
        }
        hit
    }

    fn eat_kw(&mut self, k: &str) -> bool {
        let hit = self.is_kw(k);
        if hit {
            self.bump();
        }
        hit
    }

    fn expect_punct(&mut self, p: &str) -> PResult<()> {
        if self.eat_punct(p) {
            Ok(())
        } else {
            self.unexpected(&format!("`{p}`"))
        }
    }

    fn expect_kw(&mut self, k: &str) -> PResult<()> {
        if self.eat_kw(k) {
            Ok(())
        } else {
            self.unexpected(&format!("`{k}`"))
        }
    }

    /// Expect the closer of the bracket opened at token `open`; at end of file that is E102.
    fn close(&mut self, p: &str, open: usize) -> PResult<()> {
        if self.at_eof() {
            let opener = describe(&self.toks[open].tok);
            return Err(Error {
                code: "E102",
                at: open,
                message: format!("unclosed {opener}"),
            });
        }
        self.expect_punct(p)
    }

    fn name(&mut self) -> PResult<Name> {
        match self.peek() {
            Tok::Name(n) => {
                let n = n.clone();
                self.bump();
                Ok(n)
            }
            _ => self.unexpected("a name"),
        }
    }

    fn path(&mut self) -> PResult<Path> {
        let mut path = vec![self.name()?];
        while self.is_punct(".") && matches!(self.peek_at(1), Tok::Name(_)) {
            self.bump();
            path.push(self.name()?);
        }
        Ok(path)
    }

    fn string(&mut self) -> PResult<String> {
        match self.peek() {
            Tok::Str(s) => {
                let s = s.clone();
                self.bump();
                Ok(s)
            }
            _ => self.unexpected("a string"),
        }
    }

    fn label(&mut self) -> PResult<Name> {
        match self.peek() {
            Tok::Label(l) => {
                let l = l.clone();
                self.bump();
                Ok(l)
            }
            _ => self.unexpected("a label"),
        }
    }

    /// `open item { sep item } close`, allowing an empty list.
    fn list<T>(
        &mut self,
        open: &str,
        close: &str,
        mut item: impl FnMut(&mut Self) -> PResult<T>,
    ) -> PResult<Vec<T>> {
        let at = self.pos;
        self.expect_punct(open)?;
        let mut out = Vec::new();
        if self.eat_punct(close) {
            return Ok(out);
        }
        loop {
            out.push(item(self)?);
            if !self.eat_punct(",") {
                break;
            }
        }
        self.close(close, at)?;
        Ok(out)
    }

    fn module_header(&mut self) -> PResult<Path> {
        self.expect_kw("module")?;
        self.path()
    }

    // ---- items ----

    fn item(&mut self) -> PResult<Item> {
        let start = self.span();
        let mut attrs = Vec::new();
        while self.eat_punct("@") {
            let name = self.name()?;
            let args = if self.is_punct("(") {
                Some(self.list("(", ")", Self::attr_arg)?)
            } else {
                None
            };
            attrs.push(Attr { name, args });
        }
        let is_pub = self.eat_kw("pub");
        let is_unsafe = self.eat_kw("unsafe");
        let kind = match self.peek() {
            Tok::Name(k) => k.clone(),
            _ => return self.unexpected("an item"),
        };
        let kind = match (kind.as_str(), is_pub, is_unsafe) {
            ("fn", _, _) => {
                self.bump();
                ItemKind::Fn(self.fn_rest(is_pub, is_unsafe, true, false)?)
            }
            ("gen", _, false) => {
                self.bump();
                ItemKind::Gen(self.fn_rest(is_pub, false, true, true)?)
            }
            ("record", _, false) => {
                self.bump();
                let path = self.path()?;
                let tparams = self.tparams()?;
                let fields = self.list("{", "}", Self::field)?;
                ItemKind::Record(Record {
                    is_pub,
                    path,
                    tparams,
                    fields,
                })
            }
            ("sum", _, false) => {
                self.bump();
                let path = self.path()?;
                let tparams = self.tparams()?;
                let variants = self.list("{", "}", |p| {
                    let name = p.name()?;
                    let fields = if p.is_punct("{") {
                        Some(p.list("{", "}", Self::field)?)
                    } else {
                        None
                    };
                    Ok(Variant { name, fields })
                })?;
                if variants.is_empty() {
                    return self.unexpected("a variant");
                }
                ItemKind::Sum(Sum {
                    is_pub,
                    path,
                    tparams,
                    variants,
                })
            }
            ("newtype", _, false) => {
                self.bump();
                let path = self.path()?;
                let tparams = self.tparams()?;
                self.expect_punct("=")?;
                let ty = self.ty()?;
                let pred = if self.eat_kw("where") {
                    Some(self.expr()?)
                } else {
                    None
                };
                ItemKind::Newtype(Newtype {
                    is_pub,
                    path,
                    tparams,
                    ty,
                    pred,
                })
            }
            ("trait", _, false) => {
                self.bump();
                ItemKind::Trait(self.trait_rest(is_pub)?)
            }
            ("impl", false, _) => {
                self.bump();
                ItemKind::Impl(self.impl_rest(is_unsafe)?)
            }
            ("const", _, false) => {
                self.bump();
                let path = self.path()?;
                self.expect_punct(":")?;
                let ty = self.ty()?;
                self.expect_punct("=")?;
                let value = self.const_val()?;
                ItemKind::Const(Const {
                    is_pub,
                    path,
                    ty,
                    value,
                })
            }
            ("test", false, false) => {
                self.bump();
                let path = self.path()?;
                let body = self.block()?;
                ItemKind::Test(Test { path, body })
            }
            ("extern", false, true) => {
                self.bump();
                ItemKind::Extern(self.extern_rest()?)
            }
            _ => return self.unexpected("an item"),
        };
        Ok(Item {
            attrs,
            kind,
            span: join_end(start, self.prev_span()),
        })
    }

    fn attr_arg(&mut self) -> PResult<AttrArg> {
        let name =
            if matches!(self.peek(), Tok::Name(_)) && matches!(self.peek_at(1), Tok::Punct("=")) {
                let n = self.name()?;
                self.bump();
                Some(n)
            } else {
                None
            };
        Ok(AttrArg {
            name,
            value: self.const_val()?,
        })
    }

    /// After `fn` or `gen`. `item` false parses a trait signature: one-name path, optional body.
    fn fn_rest(
        &mut self,
        is_pub: bool,
        is_unsafe: bool,
        item: bool,
        is_gen: bool,
    ) -> PResult<FnDef> {
        let name_span = self.span();
        let path = if item {
            self.path()?
        } else {
            vec![self.name()?]
        };
        let tparams = self.tparams()?;
        let params = self.list("(", ")", Self::param)?;
        self.expect_punct("->")?;
        let ret = self.ty()?;
        let from = self.from()?;
        let fail = if !is_gen && self.eat_kw("fail") {
            Some(self.fail_ty()?)
        } else {
            None
        };
        let mut contracts = Vec::new();
        loop {
            contracts.push(if self.eat_kw("requires") {
                Contract::Requires(self.expr()?)
            } else if self.eat_kw("ensures") {
                Contract::Ensures(self.expr()?)
            } else if self.eat_kw("decreases") {
                Contract::Decreases(self.exprs()?)
            } else if self.eat_kw("example") {
                Contract::Example(self.expr()?)
            } else {
                break;
            });
        }
        let body = if item || self.is_punct("{") {
            Some(self.block()?)
        } else {
            None
        };
        Ok(FnDef {
            is_pub,
            is_unsafe,
            path,
            name_span,
            tparams,
            params,
            ret,
            from,
            fail,
            contracts,
            body,
        })
    }

    fn from(&mut self) -> PResult<Vec<Name>> {
        let mut out = Vec::new();
        if self.eat_kw("from") {
            out.push(self.name()?);
            while self.eat_punct(",") {
                out.push(self.name()?);
            }
        }
        Ok(out)
    }

    fn conv(&mut self) -> PResult<Conv> {
        let c = match self.peek() {
            Tok::Name(n) if n == "let" => Conv::Let,
            Tok::Name(n) if n == "inout" => Conv::Inout,
            Tok::Name(n) if n == "sink" => Conv::Sink,
            _ => return self.unexpected("`let`, `inout` or `sink`"),
        };
        self.bump();
        Ok(c)
    }

    fn param(&mut self) -> PResult<Param> {
        if self.eat_kw("cap") {
            let name = self.name()?;
            self.expect_punct(":")?;
            return Ok(Param::Cap {
                name,
                ty: self.named()?,
            });
        }
        let comptime = self.eat_kw("comptime");
        let conv = self.conv()?;
        let name = self.name()?;
        self.expect_punct(":")?;
        Ok(Param::Value {
            comptime,
            conv,
            name,
            ty: self.ty()?,
        })
    }

    fn field(&mut self) -> PResult<Field> {
        let is_pub = self.eat_kw("pub");
        let name = self.name()?;
        self.expect_punct(":")?;
        let ty = self.ty()?;
        let default = if self.eat_punct("=") {
            Some(self.const_val()?)
        } else {
            None
        };
        Ok(Field {
            is_pub,
            name,
            ty,
            default,
        })
    }

    fn tparams(&mut self) -> PResult<Vec<TParam>> {
        if !self.is_punct("[") {
            return Ok(Vec::new());
        }
        let ps = self.list("[", "]", |p| {
            let comptime = p.eat_kw("comptime");
            let name = p.name()?;
            p.expect_punct(":")?;
            let kind = match p.name()?.as_str() {
                "Type" => Kind::Type,
                "Nat" => Kind::Nat,
                "Dim" => Kind::Dim,
                _ => {
                    p.pos -= 1;
                    return p.unexpected("`Type`, `Nat` or `Dim`");
                }
            };
            let mut bounds = Vec::new();
            if p.eat_kw("is") {
                bounds.push(p.path()?);
                while p.eat_punct("+") {
                    bounds.push(p.path()?);
                }
            }
            Ok(TParam {
                comptime,
                name,
                kind,
                bounds,
            })
        })?;
        if ps.is_empty() {
            return self.unexpected("a type parameter");
        }
        Ok(ps)
    }

    fn trait_rest(&mut self, is_pub: bool) -> PResult<Trait> {
        let path = self.path()?;
        let tparams = self.tparams()?;
        let open = self.pos;
        self.expect_punct("{")?;
        let mut assoc = Vec::new();
        while self.eat_kw("type") {
            assoc.push(self.name()?);
        }
        let mut sigs = Vec::new();
        while self.eat_kw("fn") {
            sigs.push(self.fn_rest(false, false, false, false)?);
        }
        let derived = if self.eat_kw("derived") {
            Some(self.block()?)
        } else {
            None
        };
        self.close("}", open)?;
        Ok(Trait {
            is_pub,
            path,
            tparams,
            assoc,
            sigs,
            derived,
        })
    }

    fn impl_rest(&mut self, is_unsafe: bool) -> PResult<Impl> {
        let trait_ = self.named()?;
        self.expect_kw("for")?;
        let for_ty = self.ty()?;
        let open = self.pos;
        self.expect_punct("{")?;
        let mut assoc = Vec::new();
        while self.eat_kw("type") {
            let n = self.name()?;
            self.expect_punct("=")?;
            assoc.push((n, self.ty()?));
        }
        let mut fns = Vec::new();
        loop {
            let is_pub = self.eat_kw("pub");
            let is_unsafe = self.eat_kw("unsafe");
            if !self.eat_kw("fn") {
                if is_pub || is_unsafe {
                    return self.unexpected("`fn`");
                }
                break;
            }
            fns.push(self.fn_rest(is_pub, is_unsafe, true, false)?);
        }
        self.close("}", open)?;
        Ok(Impl {
            is_unsafe,
            trait_,
            for_ty,
            assoc,
            fns,
        })
    }

    fn extern_rest(&mut self) -> PResult<Extern> {
        let abi = self.string()?;
        self.expect_kw("lib")?;
        let lib = self.name()?;
        let link = if self.eat_kw("link") {
            Some(self.string()?)
        } else {
            None
        };
        let open = self.pos;
        self.expect_punct("{")?;
        let mut items = Vec::new();
        while !self.is_punct("}") && !self.at_eof() {
            items.push(self.xitem()?);
        }
        self.close("}", open)?;
        Ok(Extern {
            abi,
            lib,
            link,
            items,
        })
    }

    fn xitem(&mut self) -> PResult<XItem> {
        if self.eat_kw("type") {
            let name = self.name()?;
            self.expect_kw("drop")?;
            return Ok(XItem::Type {
                name,
                drop: self.path()?,
            });
        }
        let safe = if self.eat_kw("safe") {
            true
        } else if self.eat_kw("unsafe") {
            false
        } else {
            return self.unexpected("`safe`, `unsafe` or `type`");
        };
        if self.eat_kw("static") {
            let name = self.name()?;
            self.expect_punct(":")?;
            return Ok(XItem::Static {
                safe,
                name,
                ty: self.ty()?,
            });
        }
        self.expect_kw("fn")?;
        let name = self.name()?;
        let open = self.pos;
        self.expect_punct("(")?;
        let mut params = Vec::new();
        let mut variadic = false;
        if !self.is_punct(")") {
            loop {
                if self.eat_punct("...") {
                    variadic = true;
                    break;
                }
                let name = self.name()?;
                self.expect_punct(":")?;
                params.push(if self.is_kw("scoped") || self.is_kw("escaping") {
                    let escaping = self.is_kw("escaping");
                    self.bump();
                    self.expect_kw("Fn")?;
                    XParam::Callback {
                        name,
                        escaping,
                        ty: self.fn_type()?,
                    }
                } else {
                    XParam::Value {
                        name,
                        ty: self.ty()?,
                    }
                });
                if !self.eat_punct(",") {
                    break;
                }
            }
        }
        self.close(")", open)?;
        self.expect_punct("->")?;
        let ret = self.ty()?;
        let from = self.from()?;
        let uses_none =
            self.is_kw("uses") && matches!(self.peek_at(1), Tok::Name(n) if n == "none");
        if uses_none {
            self.bump();
            self.bump();
        }
        let nonblocking = self.eat_kw("nonblocking");
        Ok(XItem::Fn {
            safe,
            name,
            params,
            variadic,
            ret,
            from,
            uses_none,
            nonblocking,
        })
    }

    // ---- types ----

    fn named(&mut self) -> PResult<Named> {
        let path = self.path()?;
        let args = if self.is_punct("[") {
            self.targs()?
        } else {
            Vec::new()
        };
        Ok(Named { path, args })
    }

    fn targs(&mut self) -> PResult<Vec<TArg>> {
        let args = self.list("[", "]", |p| match p.peek() {
            Tok::Int(n) => {
                let n = n.clone();
                p.bump();
                Ok(TArg::Nat(n))
            }
            _ => Ok(TArg::Type(p.ty()?)),
        })?;
        if args.is_empty() {
            return self.unexpected("a type argument");
        }
        Ok(args)
    }

    fn ty(&mut self) -> PResult<Type> {
        let next_is_paren = matches!(self.peek_at(1), Tok::Punct("("));
        if self.is_kw("Tuple") && next_is_paren {
            self.bump();
            return Ok(Type::Tuple(self.list("(", ")", Self::ty)?));
        }
        if self.is_kw("Fn") && next_is_paren {
            self.bump();
            return Ok(Type::Fn(self.fn_type()?));
        }
        if self.eat_kw("dyn") {
            return Ok(Type::Dyn(self.named()?));
        }
        if self.is_punct("{") {
            let open = self.pos;
            self.bump();
            let name = self.name()?;
            self.expect_punct(":")?;
            let ty = Box::new(self.ty()?);
            self.expect_punct("|")?;
            let pred = self.expr()?;
            self.close("}", open)?;
            return Ok(Type::Refined { name, ty, pred });
        }
        if !matches!(self.peek(), Tok::Name(_)) {
            return self.unexpected("a type");
        }
        Ok(Type::Named(self.named()?))
    }

    fn fail_ty(&mut self) -> PResult<Type> {
        let first = self.ty()?;
        if !self.is_punct("|") {
            return Ok(first);
        }
        let mut alts = vec![first];
        while self.eat_punct("|") {
            alts.push(self.ty()?);
        }
        Ok(Type::Union(alts))
    }

    /// After `Fn`.
    fn fn_type(&mut self) -> PResult<FnType> {
        let params = self.list("(", ")", |p| Ok((p.conv()?, p.ty()?)))?;
        self.expect_punct("->")?;
        let ret = Box::new(self.ty()?);
        let uses = self.uses()?;
        let fail = if self.eat_kw("fail") {
            Some(Box::new(self.fail_ty()?))
        } else {
            None
        };
        Ok(FnType {
            params,
            ret,
            uses,
            fail,
        })
    }

    // SPEC-Q: `uses a, b` inside a parameter or type-argument list is ambiguous with the list's
    // own commas. Assumed: a comma continues the set unless the next name is followed by `:`
    // or is not a plain name.
    fn uses(&mut self) -> PResult<Option<CapSet>> {
        if !self.eat_kw("uses") {
            return Ok(None);
        }
        if self.eat_kw("none") {
            return Ok(Some(CapSet::None));
        }
        let mut names = vec![self.name()?];
        while self.is_punct(",")
            && matches!(self.peek_at(1), Tok::Name(_))
            && !matches!(self.peek_at(2), Tok::Punct(":" | "[" | "."))
        {
            self.bump();
            names.push(self.name()?);
        }
        Ok(Some(CapSet::Names(names)))
    }

    // ---- statements ----

    fn block(&mut self) -> PResult<Block> {
        let open = self.pos;
        self.expect_punct("{")?;
        let mut stmts = Vec::new();
        while !self.is_punct("}") && !self.at_eof() {
            stmts.push(self.stmt()?);
        }
        self.close("}", open)?;
        Ok(Block { stmts })
    }

    fn stmt(&mut self) -> PResult<Stmt> {
        let start = self.span();
        let kind = self.stmt_kind()?;
        Ok(Stmt {
            kind,
            span: join_end(start, self.prev_span()),
        })
    }

    fn stmt_kind(&mut self) -> PResult<StmtKind> {
        let kw = match self.peek() {
            Tok::Name(n) => n.clone(),
            _ => String::new(),
        };
        let next = self.peek_at(1).clone();
        let next_is = |p: &str| matches!(&next, Tok::Punct(q) if *q == p);
        Ok(match kw.as_str() {
            "let" | "var" => {
                self.bump();
                let pat_span = self.span();
                let pat = self.pat()?;
                self.expect_punct(":")?;
                let ty = self.ty()?;
                self.expect_punct("=")?;
                let rhs_span = self.span();
                StmtKind::Let {
                    mutable: kw == "var",
                    pat,
                    pat_span,
                    ty,
                    rhs: self.rhs()?,
                    rhs_span,
                }
            }
            "if" => {
                self.bump();
                let cond = self.atom()?;
                let then = self.block()?;
                let els = if self.eat_kw("else") {
                    Some(self.block()?)
                } else {
                    None
                };
                StmtKind::If { cond, then, els }
            }
            "match" => {
                self.bump();
                let scrutinee = self.atom()?;
                let open = self.pos;
                self.expect_punct("{")?;
                let mut arms = Vec::new();
                while !self.is_punct("}") && !self.at_eof() {
                    let pat = self.pat()?;
                    self.expect_punct("=>")?;
                    arms.push((pat, self.block()?));
                }
                self.close("}", open)?;
                StmtKind::Match { scrutinee, arms }
            }
            "loop" => {
                self.bump();
                let label = self.label()?;
                let mut invariants = Vec::new();
                while self.eat_kw("invariant") {
                    invariants.push(self.expr()?);
                }
                let decreases = if self.eat_kw("decreases") {
                    self.exprs()?
                } else {
                    Vec::new()
                };
                StmtKind::Loop {
                    label,
                    invariants,
                    decreases,
                    body: self.block()?,
                }
            }
            "break" => {
                self.bump();
                let label = self.label()?;
                let has_value = self.same_line()
                    && matches!(
                        self.peek(),
                        Tok::Name(_)
                            | Tok::Int(_)
                            | Tok::Float(_)
                            | Tok::Str(_)
                            | Tok::Char(_)
                            | Tok::Punct("(")
                    );
                let value = if has_value { Some(self.atom()?) } else { None };
                StmtKind::Break { label, value }
            }
            "continue" => {
                self.bump();
                StmtKind::Continue {
                    label: self.label()?,
                }
            }
            "jp" => {
                self.bump();
                let name = self.name()?;
                let params = self.list("(", ")", Self::param)?;
                StmtKind::Jp {
                    name,
                    params,
                    body: self.block()?,
                }
            }
            "jump" => {
                self.bump();
                let name = self.name()?;
                StmtKind::Jump {
                    name,
                    args: self.list("(", ")", Self::atom)?,
                }
            }
            "ret" => {
                self.bump();
                StmtKind::Ret(self.atom()?)
            }
            "fail" => {
                self.bump();
                StmtKind::Fail(self.atom()?)
            }
            "yield" => {
                self.bump();
                StmtKind::Yield(self.atom()?)
            }
            "become" => {
                self.bump();
                StmtKind::Become(self.call()?)
            }
            "with" => {
                self.bump();
                let name = self.name()?;
                self.expect_punct("=")?;
                let dyn_ = self.eat_kw("dyn");
                let handler = self.atom()?;
                StmtKind::With {
                    name,
                    dyn_,
                    handler,
                    body: self.block()?,
                }
            }
            "par" if next_is("{") || matches!(&next, Tok::Name(n) if n == "concurrent") => {
                self.bump();
                let concurrent = self.eat_kw("concurrent");
                let open = self.pos;
                self.expect_punct("{")?;
                let mut blocks = Vec::new();
                while self.is_punct("{") {
                    blocks.push(self.block()?);
                }
                self.close("}", open)?;
                StmtKind::Par { concurrent, blocks }
            }
            "comptime" => {
                self.bump();
                if self.eat_kw("for") {
                    let name = self.name()?;
                    self.expect_punct(":")?;
                    let ty = self.ty()?;
                    self.expect_kw("in")?;
                    let iter = self.atom()?;
                    StmtKind::ComptimeFor {
                        name,
                        ty,
                        iter,
                        body: self.block()?,
                    }
                } else if self.eat_kw("if") {
                    let cond = self.atom()?;
                    let then = self.block()?;
                    let els = if self.eat_kw("else") {
                        Some(self.block()?)
                    } else {
                        None
                    };
                    StmtKind::ComptimeIf { cond, then, els }
                } else {
                    return self.unexpected("`for` or `if`");
                }
            }
            "assert" => {
                self.bump();
                StmtKind::Assert(self.expr()?)
            }
            "unreachable" => {
                self.bump();
                StmtKind::Unreachable
            }
            "unsafe" if next_is("{") => {
                self.bump();
                StmtKind::Unsafe(self.block()?)
            }
            "asm" if next_is("(") => {
                self.bump();
                let open = self.pos;
                self.bump();
                let template = self.string()?;
                let mut operands = Vec::new();
                while self.eat_punct(",") {
                    operands.push(self.asm_operand()?);
                }
                self.close(")", open)?;
                StmtKind::Asm { template, operands }
            }
            "drop" if matches!(next, Tok::Name(_)) => {
                self.bump();
                StmtKind::Drop(self.name()?)
            }
            "reuse" if matches!(next, Tok::Name(_)) => {
                self.bump();
                let name = self.name()?;
                self.expect_kw("as")?;
                StmtKind::Reuse {
                    name,
                    ctor: self.ctor()?,
                }
            }
            _ => {
                let rhs = self.rhs()?;
                let place = match &rhs {
                    Rhs::Proj(p) => Some(p.clone()),
                    Rhs::Atom(Atom::Name(n, sp)) => Some(Place {
                        root: n.clone(),
                        root_span: *sp,
                        projs: Vec::new(),
                    }),
                    _ => None,
                };
                match place {
                    Some(place) if self.eat_punct("=") => {
                        let rhs_span = self.span();
                        StmtKind::Assign {
                            place,
                            rhs: self.rhs()?,
                            rhs_span,
                        }
                    }
                    _ => StmtKind::Rhs(rhs),
                }
            }
        })
    }

    fn asm_operand(&mut self) -> PResult<AsmOperand> {
        let dir = match self.peek() {
            Tok::Name(n) if n == "clobber_abi" => {
                self.bump();
                let open = self.pos;
                self.expect_punct("(")?;
                let s = self.string()?;
                self.close(")", open)?;
                return Ok(AsmOperand::ClobberAbi(s));
            }
            Tok::Name(n) if n == "options" => {
                self.bump();
                return Ok(AsmOperand::Options(self.list("(", ")", Self::name)?));
            }
            Tok::Name(n) if n == "in" => AsmDir::In,
            Tok::Name(n) if n == "out" => AsmDir::Out,
            Tok::Name(n) if n == "inout" => AsmDir::Inout,
            Tok::Name(n) if n == "lateout" => AsmDir::Lateout,
            _ => return self.unexpected("an asm operand"),
        };
        self.bump();
        let open = self.pos;
        self.expect_punct("(")?;
        let reg = match self.peek() {
            Tok::Str(_) => AsmReg::Explicit(self.string()?),
            _ => AsmReg::Class(self.name()?),
        };
        self.close(")", open)?;
        Ok(AsmOperand::Reg {
            dir,
            reg,
            value: self.atom()?,
        })
    }

    fn rhs(&mut self) -> PResult<Rhs> {
        if self.eat_kw("try") {
            return Ok(Rhs::Try(self.call()?));
        }
        if self.eat_kw("copy") {
            return Ok(Rhs::Copy(self.place()?));
        }
        if self.eat_kw("move") {
            return Ok(Rhs::Move(self.place()?));
        }
        if self.is_kw("fn") && matches!(self.peek_at(1), Tok::Punct("[")) {
            self.bump();
            return Ok(Rhs::Closure(Box::new(self.closure_rest()?)));
        }
        if self.is_punct("(") {
            let at = self.span();
            return Ok(match self.ctor_or_unit()? {
                None => Rhs::Atom(Atom::Lit(Literal::Unit, at)),
                Some(c) => Rhs::Ctor(c),
            });
        }
        if !matches!(self.peek(), Tok::Name(_)) || self.is_kw("true") || self.is_kw("false") {
            return Ok(Rhs::Atom(self.atom()?));
        }
        let start = self.pos;
        let span = self.span();
        let path = self.path()?;
        let targs = self.try_targs();
        if self.is_punct("(") {
            let args = self.list("(", ")", Self::arg)?;
            let call = Call {
                path,
                span,
                targs,
                args,
            };
            if self.eat_punct("?") {
                let via = if self.eat_kw("via") {
                    Some(self.path()?)
                } else {
                    None
                };
                return Ok(Rhs::Propagate { call, via });
            }
            return Ok(Rhs::Call(call));
        }
        if self.is_punct("{") {
            let fields = self.list("{", "}", |p| {
                let n = p.name()?;
                p.expect_punct(":")?;
                Ok((n, p.atom()?))
            })?;
            return Ok(Rhs::Ctor(Ctor::Named {
                path,
                targs,
                fields,
            }));
        }
        self.pos = start;
        let place = self.place()?;
        Ok(if place.projs.is_empty() {
            Rhs::Atom(Atom::Name(place.root, span))
        } else {
            Rhs::Proj(place)
        })
    }

    /// `[TArg, ...]` when it is followed by `(` or `{`; otherwise nothing is consumed.
    fn try_targs(&mut self) -> Vec<TArg> {
        if !self.is_punct("[") {
            return Vec::new();
        }
        let start = self.pos;
        match self.targs() {
            Ok(args) if self.is_punct("(") || self.is_punct("{") => args,
            _ => {
                self.pos = start;
                Vec::new()
            }
        }
    }

    fn call(&mut self) -> PResult<Call> {
        let span = self.span();
        let path = self.path()?;
        let targs = if self.is_punct("[") {
            self.targs()?
        } else {
            Vec::new()
        };
        Ok(Call {
            path,
            span,
            targs,
            args: self.list("(", ")", Self::arg)?,
        })
    }

    fn arg(&mut self) -> PResult<Arg> {
        let span = self.span();
        if self.eat_kw("cap") {
            return Ok(Arg::Cap(self.name()?, span));
        }
        Ok(Arg::Value(self.conv()?, self.atom()?, span))
    }

    fn ctor(&mut self) -> PResult<Ctor> {
        if self.is_punct("(") {
            return match self.ctor_or_unit()? {
                Some(c) => Ok(c),
                None => {
                    self.pos -= 1;
                    self.unexpected("an atom")
                }
            };
        }
        let path = self.path()?;
        let targs = if self.is_punct("[") {
            self.targs()?
        } else {
            Vec::new()
        };
        let fields = self.list("{", "}", |p| {
            let n = p.name()?;
            p.expect_punct(":")?;
            Ok((n, p.atom()?))
        })?;
        Ok(Ctor::Named {
            path,
            targs,
            fields,
        })
    }

    /// `()` is the unit literal; `(a, ...)` a tuple constructor.
    fn ctor_or_unit(&mut self) -> PResult<Option<Ctor>> {
        let atoms = self.list("(", ")", Self::atom)?;
        Ok((!atoms.is_empty()).then_some(Ctor::Tuple(atoms)))
    }

    fn place(&mut self) -> PResult<Place> {
        let root_span = self.span();
        let root = self.name()?;
        let mut projs = Vec::new();
        loop {
            if self.is_punct(".") && matches!(self.peek_at(1), Tok::Name(_)) {
                self.bump();
                let span = self.span();
                projs.push(Proj::Field(self.name()?, span));
            } else if self.is_punct("[") && self.same_line() {
                let open = self.pos;
                self.bump();
                projs.push(Proj::Index(self.atom()?));
                self.close("]", open)?;
            } else {
                return Ok(Place {
                    root,
                    root_span,
                    projs,
                });
            }
        }
    }

    /// After `fn`.
    fn closure_rest(&mut self) -> PResult<Closure> {
        let open = self.pos;
        self.expect_punct("[")?;
        let mut captures = Vec::new();
        while !self.is_punct("]") && !self.at_eof() {
            let span = self.span();
            captures.push((self.conv()?, self.name()?, span));
        }
        self.close("]", open)?;
        let params = self.list("(", ")", Self::param)?;
        self.expect_punct("->")?;
        let ret = self.ty()?;
        let uses = self.uses()?;
        let fail = if self.eat_kw("fail") {
            Some(self.fail_ty()?)
        } else {
            None
        };
        Ok(Closure {
            captures,
            params,
            ret,
            uses,
            fail,
            body: self.block()?,
        })
    }

    // ---- atoms, expressions, patterns, constants ----

    fn literal(&mut self) -> Option<Literal> {
        let lit = match self.peek() {
            Tok::Int(s) => Literal::Int(s.clone()),
            Tok::Float(s) => Literal::Float(s.clone()),
            Tok::Str(s) => Literal::Str(s.clone()),
            Tok::Char(c) => Literal::Char(*c),
            Tok::Name(n) if n == "true" => Literal::Bool(true),
            Tok::Name(n) if n == "false" => Literal::Bool(false),
            Tok::Punct("(") if matches!(self.peek_at(1), Tok::Punct(")")) => {
                self.bump();
                Literal::Unit
            }
            _ => return None,
        };
        self.bump();
        Some(lit)
    }

    fn atom(&mut self) -> PResult<Atom> {
        let span = self.span();
        if let Some(l) = self.literal() {
            return Ok(Atom::Lit(l, span));
        }
        match self.peek() {
            Tok::Name(_) => Ok(Atom::Name(self.name()?, span)),
            _ => self.unexpected("a name or a literal"),
        }
    }

    fn expr(&mut self) -> PResult<Expr> {
        let is_call = matches!(self.peek(), Tok::Name(_)) && {
            let start = self.pos;
            let ok = self.path().is_ok() && {
                self.try_targs();
                self.is_punct("(")
            };
            self.pos = start;
            ok
        };
        if is_call {
            Ok(Expr::Call(self.call()?))
        } else {
            Ok(Expr::Atom(self.atom()?))
        }
    }

    fn exprs(&mut self) -> PResult<Vec<Expr>> {
        let mut out = vec![self.expr()?];
        while self.eat_punct(",") {
            out.push(self.expr()?);
        }
        Ok(out)
    }

    fn pat(&mut self) -> PResult<Pat> {
        let first = self.pat_one()?;
        if !self.is_punct("|") {
            return Ok(first);
        }
        let mut alts = vec![first];
        while self.eat_punct("|") {
            alts.push(self.pat_one()?);
        }
        Ok(Pat::Or(alts))
    }

    fn pat_one(&mut self) -> PResult<Pat> {
        if self.eat_kw("_") {
            return Ok(Pat::Wild);
        }
        if let Some(lo) = self.literal() {
            if self.eat_punct("..=") {
                return match self.literal() {
                    Some(hi) => Ok(Pat::Range(lo, hi)),
                    None => self.unexpected("a literal"),
                };
            }
            return Ok(Pat::Lit(lo));
        }
        if self.is_punct("(") {
            return Ok(Pat::Tuple(self.list("(", ")", Self::pat)?));
        }
        if self.is_punct("[") {
            let open = self.pos;
            self.bump();
            let mut elems = Vec::new();
            let mut rest = None;
            while !self.is_punct("]") && !self.at_eof() {
                if self.eat_punct("..") {
                    rest = Some(if matches!(self.peek(), Tok::Name(_)) {
                        Some(self.name()?)
                    } else {
                        None
                    });
                    break;
                }
                elems.push(self.pat()?);
                if !self.eat_punct(",") {
                    break;
                }
            }
            self.close("]", open)?;
            return Ok(Pat::Slice { elems, rest });
        }
        let path = self.path()?;
        if !self.is_punct("{") {
            return Ok(if path.len() == 1 {
                Pat::Bind(path.into_iter().next().expect("one name"))
            } else {
                Pat::Ctor { path, fields: None }
            });
        }
        let open = self.pos;
        self.bump();
        let mut fields = Vec::new();
        let mut rest = false;
        loop {
            if self.eat_punct("..") {
                rest = true;
                break;
            }
            let n = self.name()?;
            self.expect_punct(":")?;
            fields.push((n, self.pat()?));
            if !self.eat_punct(",") {
                break;
            }
        }
        self.close("}", open)?;
        Ok(Pat::Ctor {
            path,
            fields: Some((fields, rest)),
        })
    }

    fn const_val(&mut self) -> PResult<ConstVal> {
        if let Some(l) = self.literal() {
            return Ok(ConstVal::Lit(l));
        }
        if self.is_punct("[") {
            return Ok(ConstVal::List(self.list("[", "]", Self::const_val)?));
        }
        if self.is_punct("(") {
            return Ok(ConstVal::Tuple(self.list("(", ")", Self::const_val)?));
        }
        if !matches!(self.peek(), Tok::Name(_)) {
            return self.unexpected("a constant");
        }
        let path = self.path()?;
        let fields = if self.is_punct("{") {
            Some(self.list("{", "}", |p| {
                let n = p.name()?;
                p.expect_punct(":")?;
                Ok((n, p.const_val()?))
            })?)
        } else {
            None
        };
        Ok(ConstVal::Path { path, fields })
    }
}
