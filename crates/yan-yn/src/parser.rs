//! Recursive-descent parser for 15.2, with the three two-token lookahead points of the
//! Factorings paragraph: clause-form arms, lambda parameters and function types.

use std::collections::HashSet;

use yan_diag::{Diagnostic, Span};

use crate::ast::*;
use crate::lexer::{LexPart, LexStr, Tok, Token};

type P<T> = Result<T, ()>;

const ASSIGN_OPS: [&str; 13] = [
    "=", "+=", "-=", "*=", "/=", "//=", "%=", "**=", "&=", "|=", "^=", "<<=", ">>=",
];

pub struct Parser<'a> {
    toks: &'a [Token],
    pos: usize,
    file: &'a str,
    src: &'a str,
    pub diags: Vec<Diagnostic>,
    /// Layout depth before each token, for recovery.
    depth: Vec<u32>,
    subject_depth: u32,
    /// In an index: SIGN is a binary operator and SPLPAREN a call.
    sign_binary: bool,
    fn_kind: Option<FnKind>,
    reported: HashSet<(u32, u32, &'static str)>,
}

fn join(a: Span, b: Span) -> Span {
    Span {
        line: a.line,
        col: a.col,
        end_line: b.end_line,
        end_col: b.end_col,
    }
}

fn is_assign_op(t: &Tok) -> bool {
    matches!(t, Tok::Punct(p) if ASSIGN_OPS.contains(p))
}

fn is_paren(e: &Expr) -> bool {
    matches!(e.kind, ExprKind::Paren(_))
}

fn binop(e: &Expr) -> Option<BinOp> {
    match e.kind {
        ExprKind::Binary(op, ..) => Some(op),
        _ => None,
    }
}

impl<'a> Parser<'a> {
    pub fn new(toks: &'a [Token], file: &'a str, src: &'a str) -> Self {
        let mut depth = Vec::with_capacity(toks.len());
        let mut d = 0u32;
        for t in toks {
            depth.push(d);
            match t.tok {
                Tok::Indent => d += 1,
                Tok::Dedent => d = d.saturating_sub(1),
                _ => {}
            }
        }
        Self {
            toks,
            pos: 0,
            file,
            src,
            diags: Vec::new(),
            depth,
            subject_depth: 0,
            sign_binary: false,
            fn_kind: None,
            reported: HashSet::new(),
        }
    }

    // ---- token access

    fn peek(&self) -> &'a Tok {
        &self.toks[self.pos].tok
    }

    fn peek_n(&self, n: usize) -> &'a Tok {
        &self.toks[(self.pos + n).min(self.toks.len() - 1)].tok
    }

    fn span(&self) -> Span {
        self.toks[self.pos].span
    }

    fn prev_span(&self) -> Span {
        self.toks[self.pos.saturating_sub(1)].span
    }

    fn since(&self, start: Span) -> Span {
        join(start, self.prev_span())
    }

    fn bump(&mut self) -> &'a Token {
        let t = &self.toks[self.pos];
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    fn is_p(&self, p: &str) -> bool {
        matches!(self.peek(), Tok::Punct(x) if *x == p)
    }

    fn is_kw(&self, k: &str) -> bool {
        matches!(self.peek(), Tok::Kw(x) if *x == k)
    }

    fn is_word(&self, w: &str) -> bool {
        matches!(self.peek(), Tok::Name(x) if x == w)
    }

    fn eat_p(&mut self, p: &str) -> bool {
        let hit = self.is_p(p);
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

    fn eat_word(&mut self, w: &str) -> bool {
        let hit = self.is_word(w);
        if hit {
            self.bump();
        }
        hit
    }

    fn at_line_end(&self) -> bool {
        matches!(self.peek(), Tok::Newline | Tok::Dedent | Tok::Eof)
    }

    fn at_block(&self) -> bool {
        *self.peek() == Tok::Newline && *self.peek_n(1) == Tok::Indent
    }

    fn quote(&self) -> String {
        let t = &self.toks[self.pos];
        match &t.tok {
            Tok::Newline => "end of line".into(),
            Tok::Indent => "indent".into(),
            Tok::Dedent => "dedent".into(),
            Tok::Eof => "end of file".into(),
            _ => self.src.get(t.start..t.end).unwrap_or("").to_owned(),
        }
    }

    fn fail<T>(&mut self, expected: &str) -> P<T> {
        let quote = self.quote();
        let span = self.span();
        self.diags.push(Diagnostic::error(
            "E100",
            self.file,
            span,
            &quote,
            format!("unexpected token; expected {expected}"),
        ));
        Err(())
    }

    fn report(&mut self, code: &'static str, span: Span, msg: &str) {
        if self.reported.insert((span.line, span.col, code)) {
            let quote = self.slice(span);
            self.diags
                .push(Diagnostic::error(code, self.file, span, &quote, msg));
        }
    }

    fn slice(&self, span: Span) -> String {
        let line = self.src.lines().nth(span.line as usize - 1).unwrap_or("");
        let lo = (span.col as usize - 1).min(line.len());
        let hi = if span.end_line == span.line {
            (span.end_col as usize - 1).clamp(lo, line.len())
        } else {
            line.len()
        };
        line.get(lo..hi).unwrap_or("").to_owned()
    }

    fn expect_p(&mut self, p: &str) -> P<Span> {
        if self.is_p(p) {
            Ok(self.bump().span)
        } else {
            self.fail(&format!("`{p}`"))
        }
    }

    fn expect_kw(&mut self, k: &str) -> P<Span> {
        if self.is_kw(k) {
            Ok(self.bump().span)
        } else {
            self.fail(&format!("`{k}`"))
        }
    }

    fn expect_word(&mut self, w: &str) -> P<Span> {
        if self.is_word(w) {
            Ok(self.bump().span)
        } else {
            self.fail(&format!("`{w}`"))
        }
    }

    fn name(&mut self) -> P<(Name, Span)> {
        match self.peek() {
            Tok::Name(n) => {
                let s = self.bump().span;
                Ok((n.clone(), s))
            }
            _ => self.fail("a name"),
        }
    }

    fn type_name(&mut self) -> P<(Name, Span)> {
        match self.peek() {
            Tok::TypeName(n) => {
                let s = self.bump().span;
                Ok((n.clone(), s))
            }
            _ => self.fail("a type name"),
        }
    }

    fn string(&mut self) -> P<String> {
        match self.peek() {
            Tok::Str(s) if s.parts.len() == 1 => {
                self.bump();
                match &s.parts[0] {
                    LexPart::Text(t) => Ok(t.clone()),
                    LexPart::Interp { .. } => self.fail("a plain string"),
                }
            }
            _ => self.fail("a string"),
        }
    }

    fn newline(&mut self) -> P<()> {
        match self.peek() {
            Tok::Newline => {
                self.bump();
                Ok(())
            }
            Tok::Eof | Tok::Dedent => Ok(()),
            _ => self.fail("end of line"),
        }
    }

    /// The end of a simple statement: a newline, unless the statement ended in a block.
    fn end_stmt(&mut self) -> P<()> {
        if self.pos > 0 && self.toks[self.pos - 1].tok == Tok::Dedent {
            return Ok(());
        }
        self.newline()
    }

    /// Whether the bracket opening at `self.pos + at` is closed by `) ->`.
    fn closes_with_arrow(&self, at: usize) -> bool {
        let mut depth = 0i32;
        let mut i = self.pos + at;
        while let Some(t) = self.toks.get(i) {
            match t.tok {
                Tok::Punct("(" | "[" | "{") | Tok::SplParen => depth += 1,
                Tok::Punct(")" | "]" | "}") => {
                    depth -= 1;
                    if depth == 0 {
                        return matches!(
                            self.toks.get(i + 1).map(|t| &t.tok),
                            Some(Tok::Punct("->"))
                        );
                    }
                }
                Tok::Newline | Tok::Eof => return false,
                _ => {}
            }
            i += 1;
        }
        false
    }

    /// Whether the line from here has `->` outside brackets.
    fn line_has_arrow(&self) -> bool {
        let mut depth = 0i32;
        for t in &self.toks[self.pos..] {
            match t.tok {
                Tok::Punct("(" | "[" | "{") | Tok::SplParen => depth += 1,
                Tok::Punct(")" | "]" | "}") => depth -= 1,
                Tok::Punct("->") if depth == 0 => return true,
                Tok::Newline | Tok::Indent | Tok::Dedent | Tok::Eof => return false,
                _ => {}
            }
        }
        false
    }

    fn recover(&mut self) {
        let start = self.pos;
        loop {
            let t = &self.toks[self.pos].tok;
            if *t == Tok::Eof {
                return;
            }
            let at_item = self.pos > start
                && self.depth[self.pos] == 0
                && !matches!(t, Tok::Newline | Tok::Indent | Tok::Dedent)
                && matches!(self.toks[self.pos - 1].tok, Tok::Newline | Tok::Dedent);
            if at_item {
                return;
            }
            self.bump();
        }
    }

    // ---- module and items

    pub fn module(&mut self) -> Module {
        let mut attrs = Vec::new();
        while self.is_p("@@") {
            match self.attr() {
                Ok(a) => attrs.push(a),
                Err(()) => self.recover(),
            }
        }
        let mut items = Vec::new();
        while *self.peek() != Tok::Eof {
            match self.item() {
                Ok(i) => items.push(i),
                Err(()) => self.recover(),
            }
        }
        Module {
            attrs,
            items,
            docs: Vec::new(),
            trivia: Default::default(),
        }
    }

    fn attr(&mut self) -> P<Attr> {
        let start = self.span();
        self.bump();
        let attr = self.attr_body(start)?;
        self.newline()?;
        Ok(attr)
    }

    fn attr_body(&mut self, start: Span) -> P<Attr> {
        let (name, _) = self.name()?;
        let args = if self.is_p("(") {
            self.bump();
            self.args()?
        } else {
            Vec::new()
        };
        Ok(Attr {
            name,
            args,
            span: self.since(start),
        })
    }

    fn attrs(&mut self) -> P<Vec<Attr>> {
        let mut out = Vec::new();
        while self.is_p("@") {
            out.push(self.attr()?);
        }
        Ok(out)
    }

    fn item(&mut self) -> P<Item> {
        let start = self.span();
        let attrs = self.attrs()?;
        if self.is_kw("use") {
            let kind = self.use_decl()?;
            return Ok(Item {
                attrs,
                public: false,
                kind,
                span: self.since(start),
            });
        }
        let public = self.eat_kw("pub");
        let kind = match self.peek() {
            Tok::Kw("type") => ItemKind::Type(self.type_def()?),
            Tok::Kw("trait") => ItemKind::Trait(self.trait_def()?),
            Tok::Kw("impl") => ItemKind::Impl(self.impl_def(false)?),
            Tok::Kw("unsafe") if *self.peek_n(1) == Tok::Kw("impl") => {
                self.bump();
                ItemKind::Impl(self.impl_def(true)?)
            }
            Tok::Kw("unsafe") if *self.peek_n(1) == Tok::Kw("extern") => {
                ItemKind::Extern(self.extern_def()?)
            }
            Tok::Kw("let") => ItemKind::Const(self.const_def()?),
            Tok::Name(_) => ItemKind::Fn(self.fn_def()?),
            _ => return self.fail("an item"),
        };
        Ok(Item {
            attrs,
            public,
            kind,
            span: self.since(start),
        })
    }

    fn fn_kind_word(&self) -> FnKind {
        let kind = match self.peek() {
            Tok::Name(w) if w == "gen" => FnKind::Gen,
            Tok::Name(w) if w == "sub" => FnKind::Sub,
            Tok::Name(w) if w == "lemma" => FnKind::Lemma,
            _ => return FnKind::Plain,
        };
        if matches!(self.peek_n(1), Tok::Name(_)) {
            kind
        } else {
            FnKind::Plain
        }
    }

    fn fn_def(&mut self) -> P<FnDef> {
        let kind = self.fn_kind_word();
        if kind != FnKind::Plain {
            self.bump();
        }
        let sig = self.sig()?;
        self.expect_p("=")?;
        let outer = self.fn_kind.replace(kind);
        let body = self.body(true);
        self.fn_kind = outer;
        Ok(FnDef {
            kind,
            sig,
            body: body?,
        })
    }

    fn use_decl(&mut self) -> P<ItemKind> {
        self.bump();
        let relative = self.eat_p(".");
        let mut path = vec![self.name()?.0];
        while self.eat_p(".") {
            path.push(self.name()?.0);
        }
        let alias = if self.eat_kw("as") {
            Some(self.name()?.0)
        } else {
            None
        };
        self.newline()?;
        Ok(ItemKind::Use(UseDecl {
            relative,
            path,
            alias,
        }))
    }

    fn sig(&mut self) -> P<Sig> {
        let start = self.span();
        let (name, name_span) = self.name()?;
        let tparams = if self.is_p("[") {
            self.tparams()?
        } else {
            Vec::new()
        };
        self.expect_p("(")?;
        let params = self.params()?;
        let mut sig = Sig {
            name,
            name_span,
            tparams,
            params,
            ret: None,
            from: Vec::new(),
            fail: None,
            uses: None,
            span: start,
        };
        if self.eat_p("->") {
            if self.at_fn_type() {
                sig.ret = Some(self.fn_type()?);
                sig.span = self.since(start);
                return Ok(sig);
            }
            sig.ret = Some(self.vtype()?);
            if self.eat_word("from") {
                sig.from.push(self.name()?.0);
                while self.eat_p(",") {
                    sig.from.push(self.name()?.0);
                }
            }
        }
        if self.eat_kw("fail") {
            sig.fail = Some(self.vtype()?);
        }
        if self.eat_word("uses") {
            sig.uses = Some(self.cap_items()?);
        }
        sig.span = self.since(start);
        Ok(sig)
    }

    fn cap_item(&mut self) -> P<CapItem> {
        match self.peek() {
            Tok::Name(n) if n == "none" => {
                self.bump();
                Ok(CapItem::None)
            }
            Tok::Name(n) => {
                let name = n.clone();
                self.bump();
                let ty = if self.eat_p(":") {
                    Some(self.type_name()?.0)
                } else {
                    None
                };
                Ok(CapItem::Named { name, ty })
            }
            Tok::TypeName(n) => {
                self.bump();
                Ok(CapItem::Set(n.clone()))
            }
            _ => self.fail("a capability"),
        }
    }

    fn cap_items(&mut self) -> P<Vec<CapItem>> {
        let mut out = vec![self.cap_item()?];
        while self.eat_p(",") {
            out.push(self.cap_item()?);
        }
        Ok(out)
    }

    fn tparams(&mut self) -> P<Vec<TParam>> {
        self.expect_p("[")?;
        let mut out = Vec::new();
        loop {
            match self.peek() {
                Tok::TypeName(_) => {
                    let (name, _) = self.type_name()?;
                    let mut bounds = Vec::new();
                    if self.eat_p(":") {
                        bounds.push(self.bound()?);
                        while self.eat_p("+") {
                            bounds.push(self.bound()?);
                        }
                    }
                    out.push(TParam::Type { name, bounds });
                }
                Tok::Name(_) => {
                    let (name, _) = self.name()?;
                    self.expect_p(":")?;
                    let ty = self.ty()?;
                    out.push(TParam::Value { name, ty });
                }
                _ => return self.fail("a type parameter"),
            }
            if !self.eat_p(",") {
                break;
            }
        }
        self.expect_p("]")?;
        Ok(out)
    }

    fn bound(&mut self) -> P<Bound> {
        if matches!(self.peek(), Tok::TypeName(n) if n == "Nat") {
            self.bump();
            return Ok(Bound::Nat);
        }
        let path = self.tpath()?;
        let args = if self.eat_p("[") {
            self.types_until("]")?
        } else {
            Vec::new()
        };
        Ok(Bound::Trait { path, args })
    }

    fn params(&mut self) -> P<Vec<Param>> {
        let mut out = Vec::new();
        while !self.is_p(")") {
            let start = self.span();
            if self.eat_p("/") {
                out.push(Param::PosOnly(start));
            } else if self.eat_p("**") {
                let (name, _) = self.name()?;
                self.expect_p(":")?;
                let ty = self.ty()?;
                out.push(Param::Rest {
                    name,
                    ty,
                    span: self.since(start),
                });
            } else {
                let mode = if self.eat_kw("var") {
                    ParamMode::Var
                } else if self.eat_kw("take") {
                    ParamMode::Take
                } else if self.is_word("ghost")
                    && matches!(self.peek_n(1), Tok::Name(_) | Tok::Kw("self"))
                {
                    self.bump();
                    ParamMode::Ghost
                } else {
                    ParamMode::Let
                };
                let name = if self.eat_kw("self") {
                    "self".to_owned()
                } else {
                    self.name()?.0
                };
                let ty = if self.eat_p(":") {
                    Some(self.ty()?)
                } else {
                    None
                };
                let default = if self.eat_p("=") {
                    Some(self.expr()?)
                } else {
                    None
                };
                out.push(Param::Value {
                    mode,
                    name,
                    ty,
                    default,
                    span: self.since(start),
                });
            }
            if !self.eat_p(",") {
                break;
            }
        }
        self.expect_p(")")?;
        Ok(out)
    }

    fn type_def(&mut self) -> P<TypeDef> {
        self.bump();
        let (name, name_span) = self.type_name()?;
        let tparams = if self.is_p("[") {
            self.tparams()?
        } else {
            Vec::new()
        };
        self.expect_p("=")?;
        let rhs = if self.is_p("{") {
            self.record_fields()?
        } else if self.eat_word("uses") {
            TypeRhs::CapSet(self.cap_items()?)
        } else if self.is_word("newtype") && *self.peek_n(1) == Tok::Punct("[") {
            self.bump();
            self.bump();
            let t = self.ty()?;
            self.expect_p("]")?;
            TypeRhs::Newtype(t)
        } else if self.at_variant() {
            let mut variants = vec![self.variant()?];
            while self.eat_p("|") {
                if !self.at_variant() {
                    return self.fail("a variant, as every alternative of a sum is one");
                }
                variants.push(self.variant()?);
            }
            TypeRhs::Sum(variants)
        } else {
            TypeRhs::Alias(self.ty()?)
        };
        let refine = if self.is_word("where") {
            self.bump();
            Some(self.expr()?)
        } else {
            None
        };
        self.newline()?;
        Ok(TypeDef {
            name,
            name_span,
            tparams,
            rhs,
            refine,
        })
    }

    fn at_variant(&self) -> bool {
        matches!(self.peek(), Tok::TypeName(_)) && *self.peek_n(1) == Tok::Punct("(")
            || *self.peek() == Tok::Punct("@")
    }

    fn inline_attrs(&mut self) -> P<Vec<Attr>> {
        let mut out = Vec::new();
        while self.is_p("@") {
            let start = self.bump().span;
            out.push(self.attr_body(start)?);
        }
        Ok(out)
    }

    fn variant(&mut self) -> P<Variant> {
        let start = self.span();
        let attrs = self.inline_attrs()?;
        let (name, _) = self.type_name()?;
        self.expect_p("(")?;
        let mut fields = Vec::new();
        while !self.is_p(")") {
            fields.push(self.field()?);
            if !self.eat_p(",") {
                break;
            }
        }
        self.expect_p(")")?;
        Ok(Variant {
            attrs,
            name,
            fields,
            span: self.since(start),
        })
    }

    fn field(&mut self) -> P<Field> {
        let start = self.span();
        let attrs = self.inline_attrs()?;
        let (name, _) = self.name()?;
        self.expect_p(":")?;
        let ty = self.ty()?;
        let default = if self.eat_p("=") {
            Some(self.expr()?)
        } else {
            None
        };
        Ok(Field {
            attrs,
            name,
            ty,
            default,
            span: self.since(start),
        })
    }

    fn record_fields(&mut self) -> P<TypeRhs> {
        self.expect_p("{")?;
        let mut fields = Vec::new();
        let mut positional = None;
        while !self.is_p("}") {
            if self.is_p("/") && positional.is_none() {
                self.bump();
                positional = Some(fields.len());
            } else {
                fields.push(self.field()?);
            }
            if !self.eat_p(",") {
                break;
            }
        }
        self.expect_p("}")?;
        Ok(TypeRhs::Record { fields, positional })
    }

    fn open_block(&mut self) -> P<()> {
        if !self.at_block() {
            return self.fail("an indented block");
        }
        self.bump();
        self.bump();
        Ok(())
    }

    fn close_block(&mut self) -> P<()> {
        match self.peek() {
            Tok::Dedent => {
                self.bump();
                Ok(())
            }
            Tok::Eof => Ok(()),
            _ => self.fail("end of block"),
        }
    }

    fn trait_def(&mut self) -> P<TraitDef> {
        self.bump();
        let (name, _) = self.type_name()?;
        let tparams = if self.is_p("[") {
            self.tparams()?
        } else {
            Vec::new()
        };
        self.open_block()?;
        let mut def = TraitDef {
            name,
            tparams,
            assoc: Vec::new(),
            methods: Vec::new(),
            derived: None,
        };
        while !matches!(self.peek(), Tok::Dedent | Tok::Eof) {
            if self.eat_kw("type") {
                def.assoc.push(self.type_name()?.0);
                self.newline()?;
            } else if self.is_word("derived") && *self.peek_n(1) == Tok::Punct("=") {
                self.bump();
                self.bump();
                def.derived = Some(self.body(false)?);
            } else {
                let attrs = self.attrs()?;
                let sig = self.sig()?;
                let default = if self.eat_p("=") {
                    Some(self.body(false)?)
                } else {
                    self.newline()?;
                    None
                };
                def.methods.push(TraitMethod {
                    attrs,
                    sig,
                    default,
                });
            }
        }
        self.close_block()?;
        Ok(def)
    }

    fn impl_def(&mut self, unsafe_: bool) -> P<ImplDef> {
        self.expect_kw("impl")?;
        let trait_path = self.tpath()?;
        let trait_args = if self.eat_p("[") {
            self.types_until("]")?
        } else {
            Vec::new()
        };
        self.expect_kw("for")?;
        let for_type = self.ty()?;
        self.open_block()?;
        let mut def = ImplDef {
            unsafe_,
            trait_path,
            trait_args,
            for_type,
            assoc: Vec::new(),
            items: Vec::new(),
        };
        while !matches!(self.peek(), Tok::Dedent | Tok::Eof) {
            if self.eat_kw("type") {
                let (n, _) = self.type_name()?;
                self.expect_p("=")?;
                let t = self.ty()?;
                self.newline()?;
                def.assoc.push((n, t));
            } else {
                let start = self.span();
                let attrs = self.attrs()?;
                let f = self.fn_def()?;
                def.items.push(Item {
                    attrs,
                    public: false,
                    kind: ItemKind::Fn(f),
                    span: self.since(start),
                });
            }
        }
        self.close_block()?;
        Ok(def)
    }

    fn const_def(&mut self) -> P<ConstDef> {
        self.bump();
        let (name, _) = self.name()?;
        self.expect_p(":")?;
        let ty = self.ty()?;
        self.expect_p("=")?;
        let value = self.expr()?;
        self.newline()?;
        Ok(ConstDef { name, ty, value })
    }

    fn extern_def(&mut self) -> P<ExternDef> {
        self.expect_kw("unsafe")?;
        self.expect_kw("extern")?;
        let abi = self.string()?;
        self.expect_word("lib")?;
        let (lib, _) = self.name()?;
        let link = if self.eat_word("link") {
            Some(self.string()?)
        } else {
            None
        };
        self.open_block()?;
        let mut items = Vec::new();
        while !matches!(self.peek(), Tok::Dedent | Tok::Eof) {
            items.push(self.xitem()?);
        }
        self.close_block()?;
        Ok(ExternDef {
            abi,
            lib,
            link,
            items,
        })
    }

    fn xitem(&mut self) -> P<XItem> {
        let start = self.span();
        if self.eat_kw("type") {
            let (name, _) = self.type_name()?;
            self.expect_word("drop")?;
            let (drop, _) = self.name()?;
            self.newline()?;
            return Ok(XItem::Type {
                name,
                drop,
                span: self.since(start),
            });
        }
        let attrs = self.attrs()?;
        let safe = if self.eat_word("safe") {
            true
        } else {
            self.expect_kw("unsafe")?;
            false
        };
        if self.eat_word("static") {
            let (name, _) = self.name()?;
            self.expect_p(":")?;
            let ty = self.ty()?;
            self.newline()?;
            return Ok(XItem::Static {
                attrs,
                safe,
                name,
                ty,
                span: self.since(start),
            });
        }
        let (name, _) = self.name()?;
        self.expect_p("(")?;
        let mut params = Vec::new();
        while !self.is_p(")") {
            if self.is_p("..") && *self.peek_n(1) == Tok::Punct(".") {
                self.bump();
                self.bump();
                params.push(XParam::Variadic);
            } else {
                let (pname, _) = self.name()?;
                self.expect_p(":")?;
                let callback = if self.eat_word("scoped") {
                    Some(Callback::Scoped)
                } else if self.eat_word("escaping") {
                    Some(Callback::Escaping)
                } else {
                    None
                };
                let ty = if callback.is_some() {
                    self.fn_type()?
                } else {
                    self.ty()?
                };
                params.push(XParam::Typed {
                    name: pname,
                    callback,
                    ty,
                });
            }
            if !self.eat_p(",") {
                break;
            }
        }
        self.expect_p(")")?;
        let mut ret = None;
        let mut from = Vec::new();
        if self.eat_p("->") {
            ret = Some(self.vtype()?);
            if self.eat_word("from") {
                from.push(self.name()?.0);
                while self.eat_p(",") {
                    from.push(self.name()?.0);
                }
            }
        }
        let uses_none = if self.eat_word("uses") {
            self.expect_word("none")?;
            true
        } else {
            false
        };
        let nonblocking = self.eat_word("nonblocking");
        self.newline()?;
        Ok(XItem::Fn {
            attrs,
            safe,
            name,
            params,
            ret,
            from,
            uses_none,
            nonblocking,
            span: self.since(start),
        })
    }

    // ---- bodies and statements

    fn body(&mut self, allow_clauses: bool) -> P<Body> {
        if !self.at_block() {
            let e = self.expr()?;
            self.end_stmt()?;
            return Ok(Body::Expr(e));
        }
        self.open_block()?;
        let contracts = self.contracts(&["pre", "post", "ex", "decreases", "cost"])?;
        let clause = allow_clauses
            && self.line_has_arrow()
            && matches!(
                self.peek(),
                Tok::Name(_)
                    | Tok::TypeName(_)
                    | Tok::Kw("_" | "True" | "False" | "None")
                    | Tok::Int(_)
                    | Tok::Float(_)
                    | Tok::Str(_)
                    | Tok::Char(_)
                    | Tok::Punct("(" | "[")
            );
        if clause {
            let mut arms = Vec::new();
            while !matches!(self.peek(), Tok::Dedent | Tok::Eof) {
                arms.push(self.arm()?);
            }
            self.close_block()?;
            return Ok(Body::Clauses { contracts, arms });
        }
        let stmts = self.stmts()?;
        Ok(Body::Block { contracts, stmts })
    }

    fn contracts(&mut self, words: &[&str]) -> P<Vec<Contract>> {
        let mut out = Vec::new();
        loop {
            if matches!(self.peek(), Tok::Name(w) if w == "cost" && words.contains(&"cost"))
                && matches!(self.peek_n(1), Tok::Name(b) if b == "steps" || b == "alloc")
            {
                self.cost(&mut out)?;
                continue;
            }
            let kind = match self.peek() {
                Tok::Name(w)
                    if words.contains(&w.as_str())
                        && w != "cost"
                        && !is_assign_op(self.peek_n(1)) =>
                {
                    match w.as_str() {
                        "pre" => ContractKind::Pre,
                        "post" => ContractKind::Post,
                        "ex" => ContractKind::Ex,
                        "decreases" => ContractKind::Decreases,
                        _ => ContractKind::Invariant,
                    }
                }
                _ => return Ok(out),
            };
            let start = self.bump().span;
            let expr = self.expr()?;
            self.newline()?;
            out.push(Contract {
                kind,
                expr,
                span: self.since(start),
            });
        }
    }

    /// `cost steps <= A, alloc <= B`: one contract per bound.
    fn cost(&mut self, out: &mut Vec<Contract>) -> P<()> {
        self.bump();
        loop {
            let start = self.span();
            let kind = match self.peek() {
                Tok::Name(w) if w == "steps" => ContractKind::CostSteps,
                Tok::Name(w) if w == "alloc" => ContractKind::CostAlloc,
                _ => return self.expect_word("steps").map(|_| ()),
            };
            self.bump();
            self.expect_p("<=")?;
            let expr = self.expr()?;
            out.push(Contract {
                kind,
                expr,
                span: self.since(start),
            });
            if !self.eat_p(",") {
                break;
            }
        }
        self.newline()
    }

    /// The statements of an opened block, through its DEDENT.
    fn stmts(&mut self) -> P<Vec<Stmt>> {
        let mut out = Vec::new();
        while !matches!(self.peek(), Tok::Dedent | Tok::Eof) {
            out.push(self.stmt()?);
        }
        self.close_block()?;
        Ok(out)
    }

    fn block(&mut self) -> P<Vec<Stmt>> {
        self.open_block()?;
        self.stmts()
    }

    fn arm(&mut self) -> P<Arm> {
        let start = self.span();
        let pattern = self.pattern()?;
        let guard = if self.eat_kw("if") {
            Some(self.disj()?)
        } else {
            None
        };
        self.expect_p("->")?;
        let body = self.arm_body()?;
        Ok(Arm {
            pattern,
            guard,
            body,
            span: self.since(start),
        })
    }

    fn arm_body(&mut self) -> P<ArmBody> {
        if self.at_block() {
            return Ok(ArmBody::Block(self.block()?));
        }
        let e = self.expr()?;
        self.end_stmt()?;
        Ok(ArmBody::Expr(e))
    }

    fn stmt(&mut self) -> P<Stmt> {
        let start = self.span();
        let kind = self.stmt_kind(start)?;
        Ok(Stmt {
            kind,
            span: self.since(start),
        })
    }

    fn label(&mut self) -> Option<Name> {
        match self.peek() {
            Tok::Name(n) => {
                let n = n.clone();
                self.bump();
                Some(n)
            }
            _ => None,
        }
    }

    fn stmt_kind(&mut self, start: Span) -> P<StmtKind> {
        let simple = |p: &mut Self, k: StmtKind| -> P<StmtKind> {
            p.end_stmt()?;
            Ok(k)
        };
        match self.peek() {
            Tok::Kw("let" | "var") => self.bind(false),
            Tok::Name(w) if w == "ghost" && matches!(self.peek_n(1), Tok::Kw("let" | "var")) => {
                self.bump();
                self.bind(true)
            }
            Tok::Kw("return") => {
                self.bump();
                let e = if self.at_line_end() {
                    None
                } else {
                    Some(self.expr()?)
                };
                simple(self, StmtKind::Return(e))
            }
            Tok::Kw("fail") => {
                self.bump();
                let e = self.expr()?;
                simple(self, StmtKind::Fail(e))
            }
            Tok::Name(w) if w == "yield" && !is_assign_op(self.peek_n(1)) => {
                self.bump();
                if !matches!(self.fn_kind, Some(FnKind::Gen | FnKind::Sub)) {
                    self.report(
                        "E141",
                        start,
                        "`yield` is legal only in a generator or subscript body",
                    );
                }
                let e = self.expr()?;
                simple(self, StmtKind::Yield(e))
            }
            Tok::Name(w) if w == "reveal" && matches!(self.peek_n(1), Tok::Name(_)) => {
                self.bump();
                let mut path = vec![self.name()?.0];
                while self.eat_p(".") {
                    path.push(self.name()?.0);
                }
                simple(self, StmtKind::Reveal(path))
            }
            Tok::Kw("break") => {
                self.bump();
                let l = self.label();
                simple(self, StmtKind::Break(l))
            }
            Tok::Kw("continue") => {
                self.bump();
                let l = self.label();
                simple(self, StmtKind::Continue(l))
            }
            Tok::Kw("assert") => {
                self.bump();
                let e = self.expr()?;
                simple(self, StmtKind::Assert(e))
            }
            Tok::Kw("unreachable") => {
                self.bump();
                simple(self, StmtKind::Unreachable)
            }
            Tok::Kw("become") => {
                self.bump();
                let e = self.postfix()?;
                simple(self, StmtKind::Become(e))
            }
            Tok::Kw("if") => self.if_stmt(start),
            Tok::Kw("match") => {
                self.bump();
                let scrutinee = self.expr()?;
                self.open_block()?;
                let mut arms = Vec::new();
                while !matches!(self.peek(), Tok::Dedent | Tok::Eof) {
                    arms.push(self.arm()?);
                }
                self.close_block()?;
                Ok(StmtKind::Match { scrutinee, arms })
            }
            Tok::Kw("for" | "while" | "loop") => self.loop_stmt(None),
            Tok::Name(n)
                if *self.peek_n(1) == Tok::Punct(":")
                    && matches!(self.peek_n(2), Tok::Kw("for" | "while" | "loop")) =>
            {
                let label = Some(n.clone());
                self.bump();
                self.bump();
                self.loop_stmt(label)
            }
            Tok::Kw("par") => {
                let concurrent = self.peek_n(1) == &Tok::Name("concurrent".into());
                let next = self.peek_n(if concurrent { 2 } else { 1 });
                if *next == Tok::Newline {
                    self.bump();
                    if concurrent {
                        self.bump();
                    }
                    let body = self.block()?;
                    Ok(StmtKind::Par { concurrent, body })
                } else {
                    let e = self.par_for()?;
                    Ok(StmtKind::Expr(e))
                }
            }
            Tok::Name(w) if w == "select" && *self.peek_n(1) == Tok::Newline => {
                self.bump();
                self.open_block()?;
                let mut arms = Vec::new();
                while !matches!(self.peek(), Tok::Dedent | Tok::Eof) {
                    let s = self.span();
                    let op = self.postfix()?;
                    let pattern = if self.eat_kw("is") {
                        Some(self.pattern()?)
                    } else {
                        None
                    };
                    self.expect_p("->")?;
                    let body = self.arm_body()?;
                    arms.push(SelArm {
                        op,
                        pattern,
                        body,
                        span: self.since(s),
                    });
                }
                self.close_block()?;
                Ok(StmtKind::Select(arms))
            }
            Tok::Kw("with") => {
                self.bump();
                let kind = match self.peek() {
                    Tok::Name(w)
                        if (w == "deadline" || w == "timeout")
                            && *self.peek_n(1) == Tok::Punct("(") =>
                    {
                        let deadline = w == "deadline";
                        self.bump();
                        self.bump();
                        let e = self.expr()?;
                        self.expect_p(")")?;
                        if deadline {
                            WithKind::Deadline(e)
                        } else {
                            WithKind::Timeout(e)
                        }
                    }
                    Tok::Name(w) if w == "supervisor" && *self.peek_n(1) == Tok::Punct("(") => {
                        self.bump();
                        self.bump();
                        let e = self.expr()?;
                        self.expect_p(")")?;
                        WithKind::Supervisor(e)
                    }
                    _ => {
                        let (name, _) = self.name()?;
                        self.expect_p("=")?;
                        let dynamic = self.eat_kw("dyn");
                        let value = self.expr()?;
                        WithKind::Bind {
                            name,
                            dynamic,
                            value,
                        }
                    }
                };
                let body = self.block()?;
                Ok(StmtKind::With { kind, body })
            }
            Tok::Kw("unsafe") => {
                self.bump();
                Ok(StmtKind::Unsafe(self.block()?))
            }
            Tok::Kw("#") if matches!(self.peek_n(1), Tok::Kw("for")) => {
                self.bump();
                self.bump();
                let pattern = self.pattern()?;
                self.expect_kw("in")?;
                let iter = self.expr()?;
                let body = self.block()?;
                Ok(StmtKind::StageFor {
                    pattern,
                    iter,
                    body,
                })
            }
            Tok::Kw("#") if matches!(self.peek_n(1), Tok::Kw("if")) => {
                self.bump();
                self.bump();
                let cond = self.expr()?;
                let then = self.block()?;
                let otherwise = if self.eat_kw("else") {
                    Some(self.block()?)
                } else {
                    None
                };
                Ok(StmtKind::StageIf {
                    cond,
                    then,
                    otherwise,
                })
            }
            _ => {
                let e = self.expr()?;
                if let Tok::Punct(op) = self.peek()
                    && ASSIGN_OPS.contains(op)
                {
                    self.bump();
                    let value = self.expr()?;
                    self.end_stmt()?;
                    return Ok(StmtKind::Assign {
                        place: e,
                        op,
                        value,
                    });
                }
                simple(self, StmtKind::Expr(e))
            }
        }
    }

    fn bind(&mut self, ghost: bool) -> P<StmtKind> {
        let mutable = matches!(self.bump().tok, Tok::Kw("var"));
        let pattern = self.pattern()?;
        let ty = if self.eat_p(":") {
            Some(self.ty()?)
        } else {
            None
        };
        self.expect_p("=")?;
        let value = if self.at_block() {
            let start = self.span();
            let stmts = self.block()?;
            Expr {
                kind: ExprKind::Block(stmts),
                span: self.since(start),
            }
        } else if self.is_kw("par") {
            self.par_for()?
        } else {
            self.expr()?
        };
        self.end_stmt()?;
        Ok(StmtKind::Let {
            mutable,
            ghost,
            pattern,
            ty,
            value,
        })
    }

    fn if_stmt(&mut self, start: Span) -> P<StmtKind> {
        self.bump();
        let cond = self.expr()?;
        if self.is_kw("then") {
            let e = self.cond_rest(start, cond)?;
            let e = self.fallback_rest(e)?;
            self.end_stmt()?;
            return Ok(StmtKind::Expr(e));
        }
        let mut branches = vec![(cond, self.block()?)];
        let mut otherwise = None;
        while self.eat_kw("else") {
            if self.eat_kw("if") {
                let c = self.expr()?;
                branches.push((c, self.block()?));
            } else {
                otherwise = Some(self.block()?);
                break;
            }
        }
        Ok(StmtKind::If {
            branches,
            otherwise,
        })
    }

    fn loop_stmt(&mut self, label: Option<Name>) -> P<StmtKind> {
        let head = self.bump().tok.clone();
        let (header, mutable) = match head {
            Tok::Kw("for") => {
                let mutable = self.eat_kw("var");
                let pattern = self.pattern()?;
                self.expect_kw("in")?;
                let iter = self.expr()?;
                (Some((pattern, iter)), mutable)
            }
            _ => (None, false),
        };
        let cond = if head == Tok::Kw("while") {
            Some(self.expr()?)
        } else {
            None
        };
        self.open_block()?;
        let invariants = self.contracts(&["invariant", "decreases"])?;
        let body = self.stmts()?;
        Ok(match (header, cond) {
            (Some((pattern, iter)), _) => StmtKind::For {
                label,
                mutable,
                pattern,
                iter,
                invariants,
                body,
            },
            (None, Some(cond)) => StmtKind::While {
                label,
                cond,
                invariants,
                body,
            },
            (None, None) => StmtKind::Loop {
                label,
                invariants,
                body,
            },
        })
    }

    fn par_for(&mut self) -> P<Expr> {
        let start = self.expect_kw("par")?;
        let concurrent = self.eat_word("concurrent");
        let limit = if self.eat_p("(") {
            self.expect_word("limit")?;
            self.expect_p(":")?;
            let e = self.expr()?;
            self.expect_p(")")?;
            Some(Box::new(e))
        } else {
            None
        };
        self.expect_kw("for")?;
        let pattern = self.pattern()?;
        self.expect_kw("in")?;
        let iter = Box::new(self.expr()?);
        let body = self.block()?;
        Ok(Expr {
            kind: ExprKind::ParFor {
                concurrent,
                limit,
                pattern,
                iter,
                body,
            },
            span: self.since(start),
        })
    }

    // ---- expressions

    fn mk(&self, start: Span, kind: ExprKind) -> Expr {
        Expr {
            kind,
            span: self.since(start),
        }
    }

    fn mix(&mut self, e: &Expr) {
        let span = Span {
            end_line: e.span.line,
            end_col: e.span.col + 1,
            ..e.span
        };
        self.report("E101", span, "operator mix that requires parentheses");
    }

    pub fn expr(&mut self) -> P<Expr> {
        let start = self.span();
        let lambda_params = match self.peek() {
            Tok::Name(n) if *self.peek_n(1) == Tok::Punct("->") => {
                let n = n.clone();
                self.bump();
                Some(vec![n])
            }
            Tok::Punct("(") | Tok::SplParen if self.closes_with_arrow(0) => {
                self.bump();
                let mut ps = Vec::new();
                while !self.is_p(")") {
                    ps.push(self.name()?.0);
                    if !self.eat_p(",") {
                        break;
                    }
                }
                self.expect_p(")")?;
                Some(ps)
            }
            _ => None,
        };
        if let Some(params) = lambda_params {
            self.expect_p("->")?;
            let body = Box::new(self.expr()?);
            return Ok(self.mk(start, ExprKind::Lambda { params, body }));
        }
        let lhs = self.cond()?;
        self.fallback_rest(lhs)
    }

    fn fallback_rest(&mut self, lhs: Expr) -> P<Expr> {
        if !self.is_kw("else") {
            return Ok(lhs);
        }
        self.bump();
        let start = lhs.span;
        let fb = match self.peek() {
            Tok::Name(n) if *self.peek_n(1) == Tok::Punct("->") => {
                let n = n.clone();
                self.bump();
                self.bump();
                if self.at_block() {
                    Fallback::Block(Some(n), self.block()?)
                } else {
                    Fallback::Bind(n, self.expr()?)
                }
            }
            Tok::Newline if *self.peek_n(1) == Tok::Indent => Fallback::Block(None, self.block()?),
            Tok::Kw("return" | "fail" | "break" | "continue" | "unreachable") => {
                let s = self.span();
                let kind = match self.bump().tok {
                    Tok::Kw("return") => StmtKind::Return(
                        if self.at_line_end() || self.is_p(")") || self.is_p(",") {
                            None
                        } else {
                            Some(self.cond()?)
                        },
                    ),
                    Tok::Kw("fail") => StmtKind::Fail(self.cond()?),
                    Tok::Kw("break") => StmtKind::Break(self.label()),
                    Tok::Kw("continue") => StmtKind::Continue(self.label()),
                    _ => StmtKind::Unreachable,
                };
                Fallback::Diverge(Stmt {
                    kind,
                    span: self.since(s),
                })
            }
            _ => {
                let rhs = self.cond()?;
                if !is_paren(&rhs)
                    && (matches!(rhs.kind, ExprKind::If { .. })
                        || binop(&rhs).is_some_and(|op| op.is_arithmetic() || op.is_bitwise()))
                {
                    self.mix(&lhs);
                }
                Fallback::Value(rhs)
            }
        };
        if matches!(lhs.kind, ExprKind::If { .. }) {
            self.mix(&lhs);
        }
        Ok(self.mk(start, ExprKind::Fallback(Box::new(lhs), Box::new(fb))))
    }

    fn cond(&mut self) -> P<Expr> {
        if self.is_kw("if") {
            let start = self.bump().span;
            let c = self.disj()?;
            return self.cond_rest(start, c);
        }
        self.disj()
    }

    fn cond_rest(&mut self, start: Span, cond: Expr) -> P<Expr> {
        self.expect_kw("then")?;
        let then = self.disj()?;
        self.expect_kw("else")?;
        let otherwise = self.cond()?;
        Ok(self.mk(
            start,
            ExprKind::If {
                cond: Box::new(cond),
                then: Box::new(then),
                otherwise: Box::new(otherwise),
            },
        ))
    }

    fn disj(&mut self) -> P<Expr> {
        let start = self.span();
        let mut e = self.conj()?;
        while self.eat_kw("or") {
            let rhs = self.conj()?;
            let mixed = [&e, &rhs].iter().any(|x| binop(x) == Some(BinOp::And));
            e = self.mk(
                start,
                ExprKind::Binary(BinOp::Or, Box::new(e), Box::new(rhs)),
            );
            if mixed {
                self.mix(&e);
            }
        }
        Ok(e)
    }

    fn conj(&mut self) -> P<Expr> {
        let start = self.span();
        let mut e = self.neg()?;
        while self.eat_kw("and") {
            let rhs = self.neg()?;
            let mixed = [&e, &rhs].iter().any(|x| binop(x) == Some(BinOp::Or));
            e = self.mk(
                start,
                ExprKind::Binary(BinOp::And, Box::new(e), Box::new(rhs)),
            );
            if mixed {
                self.mix(&e);
            }
        }
        Ok(e)
    }

    fn neg(&mut self) -> P<Expr> {
        if self.is_kw("not") {
            let start = self.bump().span;
            let e = self.neg()?;
            return Ok(self.mk(start, ExprKind::Unary(UnOp::Not, Box::new(e))));
        }
        self.comp()
    }

    fn compop(&self) -> Option<(BinOp, usize)> {
        Some(match self.peek() {
            Tok::Punct("==") => (BinOp::Eq, 1),
            Tok::Punct("!=") => (BinOp::Ne, 1),
            Tok::Punct("<") => (BinOp::Lt, 1),
            Tok::Punct("<=") => (BinOp::Le, 1),
            Tok::Punct(">") => (BinOp::Gt, 1),
            Tok::Punct(">=") => (BinOp::Ge, 1),
            Tok::Kw("in") => (BinOp::In, 1),
            Tok::Kw("not") if *self.peek_n(1) == Tok::Kw("in") => (BinOp::NotIn, 2),
            _ => return None,
        })
    }

    fn comp(&mut self) -> P<Expr> {
        let start = self.span();
        let lhs = self.bitor()?;
        let e = self.comp_rest(start, lhs)?;
        if self.compop().is_some() || self.is_kw("is") {
            let span = Span {
                end_line: e.span.line,
                end_col: e.span.col + 1,
                ..e.span
            };
            self.report(
                "E104",
                span,
                "chained comparison; join the comparisons with `and`",
            );
            let _ = self.comp_rest(start, e.clone())?;
        }
        Ok(e)
    }

    fn comp_rest(&mut self, start: Span, lhs: Expr) -> P<Expr> {
        if self.eat_kw("is") {
            let negated = self.eat_kw("not");
            let pattern = self.pattern()?;
            return Ok(self.mk(
                start,
                ExprKind::Is {
                    value: Box::new(lhs),
                    negated,
                    pattern,
                },
            ));
        }
        let Some((op, n)) = self.compop() else {
            return Ok(lhs);
        };
        for _ in 0..n {
            self.bump();
        }
        let rhs = self.bitor()?;
        let bad = [&lhs, &rhs]
            .iter()
            .any(|x| binop(x).is_some_and(BinOp::is_bitwise));
        let e = self.mk(start, ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)));
        if bad {
            self.mix(&e);
        }
        Ok(e)
    }

    /// A left-associative chain over `ops`, with E101 for operands that mix bitwise and
    /// other operators.
    fn chain(&mut self, ops: &[(&'static str, BinOp)], next: fn(&mut Self) -> P<Expr>) -> P<Expr> {
        let start = self.span();
        let mut e = next(self)?;
        loop {
            let op = match self.peek() {
                Tok::Punct(p) | Tok::Sign(p) => ops.iter().find(|(s, _)| s == p).map(|(_, o)| *o),
                _ => None,
            };
            let Some(op) = op else {
                return Ok(e);
            };
            if matches!(self.peek(), Tok::Sign(_)) && !self.sign_binary {
                return Ok(e);
            }
            self.bump();
            let rhs = next(self)?;
            let bad = [&e, &rhs].iter().any(|x| {
                binop(x).is_some_and(|o| {
                    if op.is_bitwise() {
                        o != op && (o.is_bitwise() || o.is_arithmetic() || o.is_comparison())
                    } else {
                        o.is_bitwise()
                            || (op == BinOp::Mod) != (o == BinOp::Mod)
                                && [op, o].iter().all(|k| {
                                    matches!(
                                        k,
                                        BinOp::Mul | BinOp::Div | BinOp::FloorDiv | BinOp::Mod
                                    )
                                })
                    }
                })
            });
            e = self.mk(start, ExprKind::Binary(op, Box::new(e), Box::new(rhs)));
            if bad {
                self.mix(&e);
            }
        }
    }

    fn bitor(&mut self) -> P<Expr> {
        self.chain(&[("|", BinOp::BitOr)], Self::bitxor)
    }

    fn bitxor(&mut self) -> P<Expr> {
        self.chain(&[("^", BinOp::BitXor)], Self::bitand)
    }

    fn bitand(&mut self) -> P<Expr> {
        self.chain(&[("&", BinOp::BitAnd)], Self::shift)
    }

    fn shift(&mut self) -> P<Expr> {
        self.chain(&[("<<", BinOp::Shl), (">>", BinOp::Shr)], Self::arith)
    }

    fn arith(&mut self) -> P<Expr> {
        self.chain(&[("+", BinOp::Add), ("-", BinOp::Sub)], Self::term)
    }

    fn term(&mut self) -> P<Expr> {
        self.chain(
            &[
                ("*", BinOp::Mul),
                ("/", BinOp::Div),
                ("//", BinOp::FloorDiv),
                ("%", BinOp::Mod),
                ("@", BinOp::MatMul),
            ],
            Self::unary,
        )
    }

    fn unary(&mut self) -> P<Expr> {
        let op = match self.peek() {
            Tok::Punct("-") | Tok::Sign("-") => UnOp::Neg,
            Tok::Punct("+") | Tok::Sign("+") => UnOp::Pos,
            Tok::Punct("~") => UnOp::BitNot,
            Tok::Kw("copy") => UnOp::Copy,
            Tok::Kw("share") => UnOp::Share,
            Tok::Kw("try") => UnOp::Try,
            _ => return self.power(),
        };
        let start = self.bump().span;
        let e = self.unary()?;
        let bad = op == UnOp::Neg && binop(&e) == Some(BinOp::Pow);
        let e = self.mk(start, ExprKind::Unary(op, Box::new(e)));
        if bad {
            self.mix(&e);
        }
        Ok(e)
    }

    fn power(&mut self) -> P<Expr> {
        let start = self.span();
        let base = self.cast()?;
        if self.eat_p("**") {
            let exp = self.unary()?;
            return Ok(self.mk(
                start,
                ExprKind::Binary(BinOp::Pow, Box::new(base), Box::new(exp)),
            ));
        }
        Ok(base)
    }

    fn cast(&mut self) -> P<Expr> {
        let start = self.span();
        let mut e = self.postfix()?;
        while self.eat_kw("as") {
            let t = self.base_type()?;
            e = self.mk(start, ExprKind::Cast(Box::new(e), t));
        }
        Ok(e)
    }

    fn postfix(&mut self) -> P<Expr> {
        let start = self.span();
        if self.is_kw("#") {
            self.bump();
            let inner = self.postfix()?;
            return Ok(self.mk(start, ExprKind::Stage(Box::new(inner))));
        }
        let e = self.atom()?;
        let subject = matches!(e.kind, ExprKind::Subject(_));
        if subject {
            self.subject_depth += 1;
        }
        let r = self.postfix_rest(start, e);
        if subject {
            self.subject_depth -= 1;
        }
        r
    }

    fn postfix_rest(&mut self, start: Span, mut e: Expr) -> P<Expr> {
        loop {
            match self.peek() {
                Tok::Punct(".") => {
                    self.bump();
                    e = match self.peek() {
                        Tok::Name(n) => {
                            self.bump();
                            self.mk(start, ExprKind::Field(Box::new(e), n.clone()))
                        }
                        Tok::Kw(k) => {
                            self.bump();
                            self.mk(start, ExprKind::Field(Box::new(e), k.to_string()))
                        }
                        Tok::TypeName(n) => {
                            self.bump();
                            self.mk(start, ExprKind::TypeField(Box::new(e), n.clone()))
                        }
                        Tok::Punct("(") => {
                            self.bump();
                            let (n, _) = self.name()?;
                            self.expect_p(")")?;
                            self.mk(start, ExprKind::Reflect(Box::new(e), n))
                        }
                        _ => return self.fail("a field name"),
                    };
                }
                Tok::Punct("(") => {
                    self.bump();
                    let args = self.args()?;
                    e = self.mk(start, ExprKind::Call(Box::new(e), args));
                }
                Tok::SplParen if self.sign_binary => {
                    self.bump();
                    let args = self.args()?;
                    e = self.mk(start, ExprKind::Call(Box::new(e), args));
                }
                Tok::Punct("[") => {
                    self.bump();
                    let outer = std::mem::replace(&mut self.sign_binary, true);
                    let r = self.ixargs();
                    self.sign_binary = outer;
                    e = self.mk(start, ExprKind::Index(Box::new(e), r?));
                }
                Tok::Punct("?") => {
                    self.bump();
                    e = self.mk(start, ExprKind::Question(Box::new(e)));
                }
                _ => return Ok(e),
            }
        }
    }

    /// Arguments after `(`, through `)`.
    fn args(&mut self) -> P<Vec<Arg>> {
        let outer = std::mem::replace(&mut self.sign_binary, false);
        let r = self.args_inner();
        self.sign_binary = outer;
        let args = r?;
        let last = args.len().saturating_sub(1);
        for (i, a) in args.iter().enumerate() {
            if matches!(a.kind, ArgKind::Become(_)) && i != last {
                let span = a.span;
                self.report("E143", span, "`become` must be the last-evaluated argument");
            }
        }
        Ok(args)
    }

    fn args_inner(&mut self) -> P<Vec<Arg>> {
        let mut out = Vec::new();
        while !self.is_p(")") {
            let start = self.span();
            let label = match self.peek() {
                Tok::Name(n) if *self.peek_n(1) == Tok::Punct(":") => {
                    let n = n.clone();
                    self.bump();
                    self.bump();
                    Some(n)
                }
                _ => None,
            };
            let kind = if label.is_some() && (self.is_p(",") || self.is_p(")")) {
                ArgKind::Pun
            } else if self.is_p("&") {
                let s = self.bump().span;
                let place = self.postfix()?;
                let _ = s;
                ArgKind::Ref(place)
            } else if self.is_kw("become") {
                self.bump();
                ArgKind::Become(self.postfix()?)
            } else {
                ArgKind::Value(self.expr()?)
            };
            let span = match &kind {
                ArgKind::Become(_) if label.is_none() => Span {
                    end_line: start.line,
                    end_col: start.col + 6,
                    ..start
                },
                _ => self.since(start),
            };
            out.push(Arg { label, kind, span });
            if !self.eat_p(",") {
                break;
            }
        }
        self.expect_p(")")?;
        Ok(out)
    }

    fn ixargs(&mut self) -> P<Vec<IxArg>> {
        let mut out = Vec::new();
        while !self.is_p("]") {
            let labeled = match self.peek() {
                Tok::TypeName(n) | Tok::Name(n) if *self.peek_n(1) == Tok::Punct(":") => {
                    Some(n.clone())
                }
                _ => None,
            };
            if let Some(n) = labeled {
                self.bump();
                self.bump();
                let inner = self.ixarg()?;
                out.push(IxArg::Labeled(n, Box::new(inner)));
            } else {
                out.push(self.ixarg()?);
            }
            if !self.eat_p(",") {
                break;
            }
        }
        self.expect_p("]")?;
        Ok(out)
    }

    fn ixarg(&mut self) -> P<IxArg> {
        let lo = if self.is_p("..") || self.is_p("..=") {
            None
        } else {
            let e = self.expr()?;
            if !(self.is_p("..") || self.is_p("..=")) {
                return Ok(IxArg::Expr(e));
            }
            Some(e)
        };
        let inclusive = self.is_p("..=");
        self.bump();
        let hi = if self.is_p(",") || self.is_p("]") {
            None
        } else {
            Some(self.bitor()?)
        };
        Ok(IxArg::Slice { lo, hi, inclusive })
    }

    fn literal(&mut self) -> P<Option<ExprKind>> {
        let kind = match self.peek() {
            Tok::Int(s) => ExprKind::Int(s.clone()),
            Tok::Float(s) => ExprKind::Float(s.clone()),
            Tok::UnitNum { num, unit } => ExprKind::UnitNum {
                num: num.clone(),
                unit: unit.clone(),
            },
            Tok::Temporal { kind, text } => ExprKind::Temporal {
                kind: *kind,
                text: text.clone(),
            },
            Tok::Char(c) => ExprKind::Char(c.clone()),
            Tok::Kw("True") => ExprKind::Bool(true),
            Tok::Kw("False") => ExprKind::Bool(false),
            Tok::Kw("None") => ExprKind::NoneLit,
            Tok::Str(s) => {
                self.bump();
                return Ok(Some(ExprKind::Str(self.str_lit(s)?)));
            }
            _ => return Ok(None),
        };
        self.bump();
        Ok(Some(kind))
    }

    fn str_lit(&mut self, s: &LexStr) -> P<StrLit> {
        let mut parts = Vec::new();
        for part in &s.parts {
            parts.push(match part {
                LexPart::Text(t) => StrPart::Text(t.clone()),
                LexPart::Interp { toks, spec } => {
                    let mut sub = Parser::new(toks, self.file, self.src);
                    sub.reported = std::mem::take(&mut self.reported);
                    let r = sub.expr().and_then(|e| match sub.peek() {
                        Tok::Eof => Ok(e),
                        _ => sub.fail("the end of the interpolation"),
                    });
                    self.reported = sub.reported;
                    self.diags.append(&mut sub.diags);
                    StrPart::Interp {
                        expr: r?,
                        spec: spec.clone(),
                    }
                }
            });
        }
        Ok(StrLit {
            kind: s.kind,
            parts,
        })
    }

    fn atom(&mut self) -> P<Expr> {
        let start = self.span();
        if let Some(kind) = self.literal()? {
            return Ok(self.mk(start, kind));
        }
        let kind = match self.peek() {
            Tok::Name(n) if n == "asm" && *self.peek_n(1) == Tok::Punct("(") => {
                return self.asm(start);
            }
            Tok::Name(n) => {
                self.bump();
                ExprKind::Name(n.clone())
            }
            Tok::TypeName(n) => {
                self.bump();
                ExprKind::TypeName(n.clone())
            }
            Tok::Kw("_") => {
                self.bump();
                ExprKind::Hole
            }
            Tok::Kw("self") => {
                self.bump();
                ExprKind::SelfValue
            }
            Tok::Punct(".") => {
                self.bump();
                if self.subject_depth > 0 {
                    self.report(
                        "E135",
                        start,
                        "implicit-subject closures may not nest; name the inner parameter",
                    );
                }
                let (n, _) = self.name()?;
                ExprKind::Subject(n)
            }
            Tok::Punct("(") | Tok::SplParen => {
                self.bump();
                let outer = std::mem::replace(&mut self.sign_binary, false);
                let r = self.paren_rest();
                self.sign_binary = outer;
                r?
            }
            Tok::Punct("[") => {
                self.bump();
                let outer = std::mem::replace(&mut self.sign_binary, false);
                let r = self.list_rest();
                self.sign_binary = outer;
                r?
            }
            Tok::Punct("{") => {
                self.bump();
                let outer = std::mem::replace(&mut self.sign_binary, false);
                let r = self.brace_rest();
                self.sign_binary = outer;
                r?
            }
            _ => return self.fail("an expression"),
        };
        Ok(self.mk(start, kind))
    }

    fn paren_rest(&mut self) -> P<ExprKind> {
        if self.eat_p(")") {
            return Ok(ExprKind::Unit);
        }
        let first = self.expr()?;
        if !self.eat_p(",") {
            self.expect_p(")")?;
            return Ok(ExprKind::Paren(Box::new(first)));
        }
        let mut items = vec![first];
        while !self.is_p(")") {
            items.push(self.expr()?);
            if !self.eat_p(",") {
                break;
            }
        }
        self.expect_p(")")?;
        Ok(ExprKind::Tuple(items))
    }

    fn at_gelem(&self) -> bool {
        matches!(
            self.peek(),
            Tok::Sign(_)
                | Tok::SplParen
                | Tok::Name(_)
                | Tok::TypeName(_)
                | Tok::Int(_)
                | Tok::Float(_)
                | Tok::UnitNum { .. }
                | Tok::Temporal { .. }
                | Tok::Str(_)
                | Tok::Char(_)
                | Tok::Kw("True" | "False" | "None")
        )
    }

    fn list_rest(&mut self) -> P<ExprKind> {
        if self.eat_p("]") {
            return Ok(ExprKind::List(Vec::new()));
        }
        let first = self.expr()?;
        if self.is_p(";") || self.at_gelem() {
            return self.grid_rest(first);
        }
        let mut items = vec![first];
        while self.eat_p(",") {
            if self.is_p("]") {
                break;
            }
            items.push(self.expr()?);
        }
        self.expect_p("]")?;
        Ok(ExprKind::List(items))
    }

    fn is_gelem(e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Unary(UnOp::Neg | UnOp::Pos, inner) => Self::is_gelem(inner),
            k => matches!(
                k,
                ExprKind::Int(_)
                    | ExprKind::Float(_)
                    | ExprKind::UnitNum { .. }
                    | ExprKind::Temporal { .. }
                    | ExprKind::Str(_)
                    | ExprKind::Char(_)
                    | ExprKind::Bool(_)
                    | ExprKind::NoneLit
                    | ExprKind::Name(_)
                    | ExprKind::TypeName(_)
                    | ExprKind::Paren(_)
            ),
        }
    }

    fn is_literal(e: &Expr) -> bool {
        matches!(
            e.kind,
            ExprKind::Int(_)
                | ExprKind::Float(_)
                | ExprKind::UnitNum { .. }
                | ExprKind::Temporal { .. }
                | ExprKind::Str(_)
                | ExprKind::Char(_)
                | ExprKind::Bool(_)
                | ExprKind::NoneLit
        )
    }

    fn grid_rest(&mut self, first: Expr) -> P<ExprKind> {
        if !Self::is_gelem(&first) {
            let span = first.span;
            self.report(
                "E100",
                span,
                "a grid element is a literal, a name or a parenthesized expression",
            );
            return Err(());
        }
        let mut rows = vec![vec![first]];
        while !self.is_p("]") {
            if self.eat_p(";") {
                rows.push(Vec::new());
                continue;
            }
            if !self.at_gelem() && !self.is_p("(") {
                return self.fail("a grid element, `;` or `]`");
            }
            let start = self.span();
            let sign = match self.peek() {
                Tok::Sign(s) => {
                    self.bump();
                    Some(*s)
                }
                _ => None,
            };
            let operand_start = self.span();
            let operand = if let Some(kind) = self.literal()? {
                self.mk(operand_start, kind)
            } else {
                match self.peek() {
                    Tok::Name(n) => {
                        let k = ExprKind::Name(n.clone());
                        self.bump();
                        self.mk(operand_start, k)
                    }
                    Tok::TypeName(n) => {
                        let k = ExprKind::TypeName(n.clone());
                        self.bump();
                        self.mk(operand_start, k)
                    }
                    Tok::SplParen | Tok::Punct("(") => {
                        self.bump();
                        let e = self.expr()?;
                        self.expect_p(")")?;
                        self.mk(operand_start, ExprKind::Paren(Box::new(e)))
                    }
                    _ => return self.fail("a grid element"),
                }
            };
            let row = rows.last_mut().expect("rows start non-empty");
            let elem = match sign {
                Some(s) => {
                    if let Some(prev) = row.last()
                        && !(Self::is_literal(prev) && Self::is_literal(&operand))
                    {
                        let span = join(prev.span, operand.span);
                        self.report(
                            "E134",
                            span,
                            "`a -b` in a grid reads as two elements; write `a - b` or `(a) (-b)`",
                        );
                    }
                    let op = if s == "-" { UnOp::Neg } else { UnOp::Pos };
                    let span = self.since(start);
                    Expr {
                        kind: ExprKind::Unary(op, Box::new(operand)),
                        span,
                    }
                }
                None => operand,
            };
            rows.last_mut().expect("non-empty").push(elem);
        }
        self.expect_p("]")?;
        Ok(ExprKind::Grid(rows))
    }

    fn brace_rest(&mut self) -> P<ExprKind> {
        if self.eat_p("}") {
            return Ok(ExprKind::Map(Vec::new()));
        }
        let first = self.expr()?;
        if self.eat_p(":") {
            let v = self.expr()?;
            let mut pairs = vec![(first, v)];
            while self.eat_p(",") {
                if self.is_p("}") {
                    break;
                }
                let k = self.expr()?;
                self.expect_p(":")?;
                pairs.push((k, self.expr()?));
            }
            self.expect_p("}")?;
            return Ok(ExprKind::Map(pairs));
        }
        let mut items = vec![first];
        while self.eat_p(",") {
            if self.is_p("}") {
                break;
            }
            items.push(self.expr()?);
        }
        self.expect_p("}")?;
        Ok(ExprKind::Set(items))
    }

    fn asm(&mut self, start: Span) -> P<Expr> {
        self.bump();
        self.expect_p("(")?;
        let template = self.string()?;
        let mut args = Vec::new();
        while self.eat_p(",") {
            let dir = match self.peek() {
                Tok::Kw("in") => Some("in".to_owned()),
                Tok::Name(n) if matches!(n.as_str(), "out" | "inout" | "lateout") => {
                    Some(n.clone())
                }
                _ => None,
            };
            if let Some(dir) = dir {
                self.bump();
                self.expect_p("(")?;
                let reg = match self.peek() {
                    Tok::Name(n) => {
                        let n = n.clone();
                        self.bump();
                        AsmReg::Class(n)
                    }
                    _ => AsmReg::Explicit(self.string()?),
                };
                self.expect_p(")")?;
                if matches!(self.peek(), Tok::Name(_)) && *self.peek_n(1) == Tok::Punct(":") {
                    let (name, _) = self.name()?;
                    self.bump();
                    let ty = self.ty()?;
                    args.push(AsmArg::Typed { dir, reg, name, ty });
                } else {
                    let value = self.expr()?;
                    args.push(AsmArg::Operand { dir, reg, value });
                }
            } else if self.eat_word("clobber_abi") {
                self.expect_p("(")?;
                args.push(AsmArg::ClobberAbi(self.string()?));
                self.expect_p(")")?;
            } else {
                args.push(AsmArg::Option(self.name()?.0));
            }
        }
        self.expect_p(")")?;
        Ok(self.mk(start, ExprKind::Asm { template, args }))
    }

    // ---- patterns

    pub fn pattern(&mut self) -> P<Pattern> {
        let start = self.span();
        let first = self.alt()?;
        if !self.is_p("|") {
            return Ok(first);
        }
        let mut alts = vec![first];
        while self.eat_p("|") {
            alts.push(self.alt()?);
        }
        Ok(Pattern {
            kind: PatKind::Or(alts),
            span: self.since(start),
        })
    }

    fn alt(&mut self) -> P<Pattern> {
        let start = self.span();
        let mut p = self.alt_base()?;
        while self.eat_kw("as") {
            let (n, _) = self.name()?;
            p = Pattern {
                kind: PatKind::As(Box::new(p), n),
                span: self.since(start),
            };
        }
        Ok(p)
    }

    fn pat_literal(&mut self) -> P<Option<Expr>> {
        let start = self.span();
        if self.is_p("-") && matches!(self.peek_n(1), Tok::Int(_) | Tok::Float(_)) {
            self.bump();
            let kind = self.literal()?.expect("checked");
            let inner = self.mk(start, kind);
            return Ok(Some(
                self.mk(start, ExprKind::Unary(UnOp::Neg, Box::new(inner))),
            ));
        }
        Ok(self.literal()?.map(|k| self.mk(start, k)))
    }

    fn alt_base(&mut self) -> P<Pattern> {
        let start = self.span();
        if let Some(lit) = self.pat_literal()? {
            let kind = if self.is_p("..") || self.is_p("..=") {
                let inclusive = self.is_p("..=");
                self.bump();
                let Some(hi) = self.pat_literal()? else {
                    return self.fail("a literal");
                };
                PatKind::Range {
                    lo: Box::new(lit),
                    hi: Box::new(hi),
                    inclusive,
                }
            } else {
                PatKind::Lit(Box::new(lit))
            };
            return Ok(Pattern {
                kind,
                span: self.since(start),
            });
        }
        let kind = match self.peek() {
            Tok::Kw("_") => {
                self.bump();
                PatKind::Wild
            }
            Tok::Name(n) => {
                self.bump();
                PatKind::Bind(n.clone())
            }
            Tok::TypeName(n) => {
                self.bump();
                let name = n.clone();
                let args = if self.eat_p("(") {
                    let mut ps = Vec::new();
                    while !self.is_p(")") {
                        ps.push(self.ppat()?);
                        if !self.eat_p(",") {
                            break;
                        }
                    }
                    self.expect_p(")")?;
                    Some(ps)
                } else {
                    None
                };
                PatKind::Ctor { name, args }
            }
            Tok::Punct("(") | Tok::SplParen => {
                self.bump();
                let first = self.pattern()?;
                if !self.eat_p(",") {
                    self.expect_p(")")?;
                    return Ok(first);
                }
                let mut ps = vec![first];
                while !self.is_p(")") {
                    ps.push(self.pattern()?);
                    if !self.eat_p(",") {
                        break;
                    }
                }
                self.expect_p(")")?;
                PatKind::Tuple(ps)
            }
            Tok::Punct("[") => {
                self.bump();
                let mut ps = Vec::new();
                while !self.is_p("]") {
                    if self.eat_p("..") {
                        let rest = match self.peek() {
                            Tok::Name(n) => {
                                let n = n.clone();
                                self.bump();
                                Some(n)
                            }
                            _ => None,
                        };
                        ps.push(SPat::Rest(rest));
                    } else {
                        ps.push(SPat::Pat(self.pattern()?));
                    }
                    if !self.eat_p(",") {
                        break;
                    }
                }
                self.expect_p("]")?;
                PatKind::Slice(ps)
            }
            _ => return self.fail("a pattern"),
        };
        Ok(Pattern {
            kind,
            span: self.since(start),
        })
    }

    fn ppat(&mut self) -> P<PPat> {
        if self.eat_p("..") {
            return Ok(PPat::Rest);
        }
        if let Tok::Name(n) = self.peek()
            && *self.peek_n(1) == Tok::Punct(":")
        {
            let n = n.clone();
            self.bump();
            self.bump();
            if self.is_p(",") || self.is_p(")") {
                return Ok(PPat::Named(n, None));
            }
            return Ok(PPat::Named(n, Some(self.pattern()?)));
        }
        Ok(PPat::Pos(self.pattern()?))
    }

    // ---- types

    fn at_fn_type(&self) -> bool {
        self.is_kw("par") || (self.is_p("(") && self.closes_with_arrow(0))
    }

    pub fn ty(&mut self) -> P<Type> {
        if self.at_fn_type() {
            return self.fn_type();
        }
        self.vtype()
    }

    fn fn_type(&mut self) -> P<Type> {
        let start = self.span();
        let par = self.eat_kw("par");
        self.expect_p("(")?;
        let params = self.types_until(")")?;
        self.expect_p("->")?;
        let (ret, fail, uses) = if self.at_fn_type() {
            (self.fn_type()?, None, None)
        } else {
            let ret = self.vtype()?;
            let fail = if self.eat_kw("fail") {
                Some(Box::new(self.vtype()?))
            } else {
                None
            };
            let uses = if self.eat_word("uses") {
                Some(if self.eat_p("(") {
                    let items = self.cap_items()?;
                    self.expect_p(")")?;
                    items
                } else {
                    vec![self.cap_item()?]
                })
            } else {
                None
            };
            (ret, fail, uses)
        };
        Ok(Type {
            kind: TypeKind::Fn {
                par,
                params,
                ret: Box::new(ret),
                fail,
                uses,
            },
            span: self.since(start),
        })
    }

    /// Types separated by commas, through `close`.
    fn types_until(&mut self, close: &str) -> P<Vec<Type>> {
        let mut out = Vec::new();
        while !self.is_p(close) {
            out.push(self.ty()?);
            if !self.eat_p(",") {
                break;
            }
        }
        self.expect_p(close)?;
        Ok(out)
    }

    fn vtype(&mut self) -> P<Type> {
        let start = self.span();
        let base = self.base_type()?;
        if self.eat_p("?") {
            return Ok(Type {
                kind: TypeKind::Option(Box::new(base)),
                span: self.since(start),
            });
        }
        if !self.is_p("|") {
            return Ok(base);
        }
        let mut alts = vec![base];
        while self.eat_p("|") {
            alts.push(self.base_type()?);
        }
        Ok(Type {
            kind: TypeKind::Union(alts),
            span: self.since(start),
        })
    }

    fn tpath(&mut self) -> P<Vec<Name>> {
        let mut path = Vec::new();
        loop {
            match self.peek() {
                Tok::Name(n) if *self.peek_n(1) == Tok::Punct(".") => {
                    path.push(n.clone());
                    self.bump();
                    self.bump();
                }
                Tok::TypeName(n) => {
                    path.push(n.clone());
                    self.bump();
                    return Ok(path);
                }
                _ => return self.fail("a type name"),
            }
        }
    }

    fn base_type(&mut self) -> P<Type> {
        let start = self.span();
        let kind = match self.peek() {
            Tok::Punct("{") => {
                self.bump();
                let mut fields = Vec::new();
                while !self.is_p("}") {
                    fields.push(self.field()?);
                    if !self.eat_p(",") {
                        break;
                    }
                }
                self.expect_p("}")?;
                TypeKind::Record(fields)
            }
            Tok::Punct("(") => {
                self.bump();
                if self.eat_p(")") {
                    TypeKind::Unit
                } else {
                    let first = self.ty()?;
                    if self.eat_p(",") {
                        let mut items = vec![first];
                        items.extend(self.types_until(")")?);
                        TypeKind::Tuple(items)
                    } else {
                        self.expect_p(")")?;
                        return Ok(first);
                    }
                }
            }
            Tok::Kw("dyn") => {
                self.bump();
                let (name, _) = self.type_name()?;
                let args = if self.eat_p("[") {
                    self.types_until("]")?
                } else {
                    Vec::new()
                };
                TypeKind::Dyn { name, args }
            }
            Tok::TypeName(n)
                if (n == "Ref" || n == "Ptr") && *self.peek_n(1) == Tok::Punct("[") =>
            {
                let is_ref = n == "Ref";
                self.bump();
                self.bump();
                let t = Box::new(self.ty()?);
                self.expect_p("]")?;
                if is_ref {
                    TypeKind::Ref(t)
                } else {
                    TypeKind::Ptr(t)
                }
            }
            Tok::TypeName(n) if n == "Vec" && *self.peek_n(1) == Tok::Punct("[") => {
                self.bump();
                self.bump();
                let lanes = match self.peek() {
                    Tok::Int(s) => {
                        let s = s.clone();
                        self.bump();
                        s
                    }
                    _ => return self.fail("a lane count"),
                };
                self.expect_p(",")?;
                let (elem, _) = self.name()?;
                self.expect_p("]")?;
                TypeKind::Vec { lanes, elem }
            }
            Tok::TypeName(_) => self.path_type()?,
            Tok::Name(_) if self.at_type_call() => {
                let mut path = Vec::new();
                while let Tok::Name(n) = self.peek() {
                    path.push(n.clone());
                    self.bump();
                    if !self.eat_p(".") {
                        break;
                    }
                }
                self.expect_p("(")?;
                TypeKind::Call {
                    path,
                    args: self.args()?,
                }
            }
            Tok::Name(n) if *self.peek_n(1) == Tok::Punct(".") => {
                let _ = n;
                self.path_type()?
            }
            Tok::Name(n) => {
                let n = n.clone();
                self.bump();
                TypeKind::Path {
                    path: vec![n],
                    args: Vec::new(),
                }
            }
            _ => return self.fail("a type"),
        };
        Ok(Type {
            kind,
            span: self.since(start),
        })
    }

    /// At `f(` or `m.f(`: a type function applied in type position.
    fn at_type_call(&self) -> bool {
        let mut k = 0;
        loop {
            if !matches!(self.peek_n(k), Tok::Name(_)) {
                return false;
            }
            match self.peek_n(k + 1) {
                Tok::Punct(".") => k += 2,
                Tok::Punct("(") => return true,
                _ => return false,
            }
        }
    }

    fn path_type(&mut self) -> P<TypeKind> {
        let path = self.tpath()?;
        let mut args = Vec::new();
        if self.eat_p("[") {
            while !self.is_p("]") {
                if let Tok::Int(s) = self.peek() {
                    args.push(TArg::Nat(s.clone()));
                    self.bump();
                } else {
                    args.push(TArg::Type(self.ty()?));
                }
                if !self.eat_p(",") {
                    break;
                }
            }
            self.expect_p("]")?;
        }
        Ok(TypeKind::Path { path, args })
    }
}
