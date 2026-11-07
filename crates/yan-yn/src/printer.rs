//! The canonical Yan printer (spec 15.4): 2-space blocks, minimal parentheses plus those E101
//! requires, signature layout of 15.3.
//!
//! Comments and author blank lines come back from the module's trivia (18.5, R376).

use yan_diag::wrap::wrap;
use yan_diag::{Lead, Trivia};

use crate::ast::*;

const WIDTH: usize = 100;

pub fn print(m: &Module) -> String {
    let mut p = Printer {
        trivia: m.trivia.clone(),
        ..Printer::default()
    };
    for a in &m.attrs {
        p.line(&format!("@{}", attr(a)));
    }
    let mut end = None;
    for (i, item) in m.items.iter().enumerate() {
        if i > 0 || !m.attrs.is_empty() {
            p.out.push('\n');
        }
        let (lead, blank) = p.trivia.leading(None, item.span.line);
        p.comments(&lead, blank);
        p.pending = p.trivia.trailing(item.span.line);
        p.item(item);
        end = Some(item.span.end_line);
    }
    let rest = p.trivia.rest(end);
    if rest.first().is_some_and(|(gap, _)| !gap) {
        p.out.push('\n');
    }
    p.comments(&rest, false);
    p.out
}

#[derive(Default)]
struct Printer {
    out: String,
    indent: usize,
    trivia: Trivia,
    /// A trailing comment for the next line printed.
    pending: Option<String>,
}

fn attr(a: &Attr) -> String {
    if a.args.is_empty() {
        format!("@{}", a.name)
    } else {
        format!("@{}({})", a.name, args(&a.args))
    }
}

fn comma<T>(xs: &[T], f: impl Fn(&T) -> String) -> String {
    xs.iter().map(f).collect::<Vec<_>>().join(", ")
}

impl Printer {
    fn line(&mut self, s: &str) {
        let comment = self.pending.take();
        for (i, (rel, text)) in wrap(s, WIDTH.saturating_sub(self.indent), 4, true)
            .iter()
            .enumerate()
        {
            self.out.push_str(&" ".repeat(self.indent + rel));
            self.out.push_str(text);
            if let (0, Some(c)) = (i, &comment) {
                self.out.push_str("  ");
                self.out.push_str(c);
            }
            self.out.push('\n');
        }
    }

    /// Own-line comments, then one blank line when `blank` or the author left one before a comment.
    fn comments(&mut self, lead: &[Lead], blank: bool) {
        for (gap, text) in lead {
            if *gap {
                self.out.push('\n');
            }
            self.out.push_str(&" ".repeat(self.indent));
            self.out.push_str(text);
            self.out.push('\n');
        }
        if blank {
            self.out.push('\n');
        }
    }

    fn nested(&mut self, f: impl FnOnce(&mut Self)) {
        self.indent += 2;
        f(self);
        self.indent -= 2;
    }

    fn item(&mut self, item: &Item) {
        for a in &item.attrs {
            self.line(&attr(a));
        }
        let pub_ = if item.public { "pub " } else { "" };
        match &item.kind {
            ItemKind::Fn(f) => self.fn_def(pub_, f),
            ItemKind::Type(t) => {
                let mut s = format!("{pub_}type {}{} = ", t.name, tparams(&t.tparams));
                s.push_str(&match &t.rhs {
                    TypeRhs::Record { fields, positional } => record(fields, *positional),
                    TypeRhs::Sum(vs) => vs
                        .iter()
                        .map(|v| {
                            let attrs: String = v.attrs.iter().map(|a| attr(a) + " ").collect();
                            format!("{attrs}{}({})", v.name, comma(&v.fields, field))
                        })
                        .collect::<Vec<_>>()
                        .join(" | "),
                    TypeRhs::Alias(ty) => type_(ty),
                    TypeRhs::Newtype(ty) => format!("newtype[{}]", type_(ty)),
                    TypeRhs::CapSet(caps) => format!("uses {}", comma(caps, cap)),
                });
                if let Some(e) = &t.refine {
                    s.push_str(&format!(" where {}", expr(e)));
                }
                self.line(&s);
            }
            ItemKind::Trait(t) => {
                self.line(&format!("{pub_}trait {}{}", t.name, tparams(&t.tparams)));
                self.nested(|p| {
                    for a in &t.assoc {
                        p.line(&format!("type {a}"));
                    }
                    for m in &t.methods {
                        for a in &m.attrs {
                            p.line(&attr(a));
                        }
                        match &m.default {
                            Some(body) => p.with_body(&sig(&m.sig), body),
                            None => p.line(&sig(&m.sig)),
                        }
                    }
                    if let Some(body) = &t.derived {
                        p.with_body("derived", body);
                    }
                });
            }
            ItemKind::Impl(d) => {
                let unsafe_ = if d.unsafe_ { "unsafe " } else { "" };
                let targs = if d.trait_args.is_empty() {
                    String::new()
                } else {
                    format!("[{}]", comma(&d.trait_args, type_))
                };
                self.line(&format!(
                    "{pub_}{unsafe_}impl {}{targs} for {}",
                    d.trait_path.join("."),
                    type_(&d.for_type)
                ));
                self.nested(|p| {
                    for (n, t) in &d.assoc {
                        p.line(&format!("type {n} = {}", type_(t)));
                    }
                    for i in &d.items {
                        p.item(i);
                    }
                });
            }
            ItemKind::Const(c) => self.line(&format!(
                "{pub_}let {}: {} = {}",
                c.name,
                type_(&c.ty),
                expr(&c.value)
            )),
            ItemKind::Extern(x) => {
                let link = x
                    .link
                    .as_ref()
                    .map(|l| format!(" link {}", quoted(l)))
                    .unwrap_or_default();
                self.line(&format!(
                    "{pub_}unsafe extern {} lib {}{link}",
                    quoted(&x.abi),
                    x.lib
                ));
                self.nested(|p| x.items.iter().for_each(|i| p.xitem(i)));
            }
            ItemKind::Use(u) => {
                let dot = if u.relative { "." } else { "" };
                let alias = u
                    .alias
                    .as_ref()
                    .map(|a| format!(" as {a}"))
                    .unwrap_or_default();
                self.line(&format!("use {dot}{}{alias}", u.path.join(".")));
            }
        }
    }

    fn xitem(&mut self, x: &XItem) {
        match x {
            XItem::Fn {
                attrs,
                safe,
                name,
                params,
                ret,
                from,
                uses_none,
                nonblocking,
                ..
            } => {
                attrs.iter().for_each(|a| self.line(&attr(a)));
                let mut s = format!(
                    "{} {name}({})",
                    if *safe { "safe" } else { "unsafe" },
                    comma(params, |p| match p {
                        XParam::Variadic => "...".to_owned(),
                        XParam::Typed { name, callback, ty } => {
                            let cb = match callback {
                                Some(Callback::Scoped) => "scoped ",
                                Some(Callback::Escaping) => "escaping ",
                                None => "",
                            };
                            format!("{name}: {cb}{}", type_(ty))
                        }
                    })
                );
                if let Some(r) = ret {
                    s.push_str(&format!(" -> {}", type_(r)));
                }
                if !from.is_empty() {
                    s.push_str(&format!(" from {}", from.join(", ")));
                }
                if *uses_none {
                    s.push_str(" uses none");
                }
                if *nonblocking {
                    s.push_str(" nonblocking");
                }
                self.line(&s);
            }
            XItem::Static {
                attrs,
                safe,
                name,
                ty,
                ..
            } => {
                attrs.iter().for_each(|a| self.line(&attr(a)));
                let s = if *safe { "safe" } else { "unsafe" };
                self.line(&format!("{s} static {name}: {}", type_(ty)));
            }
            XItem::Type { name, drop, .. } => self.line(&format!("type {name} drop {drop}")),
        }
    }

    fn fn_def(&mut self, pub_: &str, f: &FnDef) {
        let kw = match f.kind {
            FnKind::Plain => "",
            FnKind::Gen => "gen ",
            FnKind::Sub => "sub ",
            FnKind::Lemma => "lemma ",
        };
        let prefix = format!("{pub_}{kw}");
        let one = format!("{prefix}{}", sig(&f.sig));
        if self.indent + one.len() + 2 <= WIDTH {
            return self.with_body(&one, &f.body);
        }
        // 15.3: clauses on their own lines indented 4, `=` after the last; parameters one per
        // line when the head still does not fit.
        let s = &f.sig;
        let clauses = sig_clauses(s);
        let head = format!(
            "{prefix}{}{}({})",
            s.name,
            tparams(&s.tparams),
            params(&s.params)
        );
        let ret = sig_ret(s);
        let mut lines = Vec::new();
        if self.indent + head.len() + ret.len() <= WIDTH {
            lines.push(format!("{head}{ret}"));
        } else {
            lines.push(format!("{prefix}{}{}(", s.name, tparams(&s.tparams)));
            for p in &s.params {
                lines.push(format!("    {},", param(p)));
            }
            lines.push(format!("){ret}"));
        }
        lines.extend(clauses.iter().map(|c| format!("    {c}")));
        let last = lines.pop().expect("a head line");
        for l in &lines {
            self.line(l);
        }
        self.with_body(&last, &f.body);
    }

    /// `head =` followed by the body.
    fn with_body(&mut self, head: &str, body: &Body) {
        match body {
            Body::Expr(e) => {
                let (line, tail) = expr_tail(e);
                self.line(&format!("{head} = {line}"));
                if let Some(stmts) = tail {
                    self.nested(|p| p.stmts(stmts));
                }
            }
            Body::Block { contracts, stmts } => {
                self.line(&format!("{head} ="));
                self.nested(|p| {
                    p.contracts(contracts);
                    p.stmts(stmts);
                });
            }
            Body::Clauses { contracts, arms } => {
                self.line(&format!("{head} ="));
                self.nested(|p| {
                    p.contracts(contracts);
                    arms.iter().for_each(|a| p.arm(a));
                });
            }
        }
    }

    fn contracts(&mut self, cs: &[Contract]) {
        let mut i = 0;
        while i < cs.len() {
            let c = &cs[i];
            i += 1;
            let kw = match c.kind {
                ContractKind::Pre => "pre",
                ContractKind::Post => "post",
                ContractKind::Ex => "ex",
                ContractKind::Decreases => "decreases",
                ContractKind::Invariant => "invariant",
                ContractKind::CostSteps | ContractKind::CostAlloc => {
                    let word = |k| {
                        if k == ContractKind::CostSteps {
                            "steps"
                        } else {
                            "alloc"
                        }
                    };
                    let mut line = format!("cost {} <= {}", word(c.kind), expr(&c.expr));
                    if let Some(n) = cs.get(i)
                        && n.kind != c.kind
                        && matches!(n.kind, ContractKind::CostSteps | ContractKind::CostAlloc)
                        && n.span.line == c.span.line
                    {
                        i += 1;
                        line.push_str(&format!(", {} <= {}", word(n.kind), expr(&n.expr)));
                    }
                    self.line(&line);
                    continue;
                }
            };
            self.line(&format!("{kw} {}", expr(&c.expr)));
        }
    }

    fn stmts(&mut self, stmts: &[Stmt]) {
        let mut prev = None;
        for s in stmts {
            let (lead, blank) = self.trivia.leading(prev, s.span.line);
            self.comments(&lead, blank);
            self.pending = self.trivia.trailing(s.span.line);
            self.stmt(s);
            prev = Some(s.span.end_line);
        }
        if let (Some(end), Some(first)) = (prev, stmts.first()) {
            let dangling = self.trivia.dangling(end, first.span.col);
            self.comments(&dangling, false);
        }
    }

    fn block(&mut self, head: &str, stmts: &[Stmt]) {
        self.line(head);
        self.nested(|p| p.stmts(stmts));
    }

    fn arm(&mut self, a: &Arm) {
        let guard = a
            .guard
            .as_ref()
            .map(|g| format!(" if {}", expr(g)))
            .unwrap_or_default();
        self.arm_body(&format!("{}{guard} ->", pattern(&a.pattern)), &a.body);
    }

    fn arm_body(&mut self, head: &str, body: &ArmBody) {
        match body {
            ArmBody::Expr(e) => {
                let (line, tail) = expr_tail(e);
                self.line(&format!("{head} {line}"));
                if let Some(stmts) = tail {
                    self.nested(|p| p.stmts(stmts));
                }
            }
            ArmBody::Block(stmts) => self.block(head, stmts),
        }
    }

    /// A line ending in an expression that may open a block.
    fn tail_line(&mut self, head: &str, e: &Expr) {
        let (line, tail) = expr_tail(e);
        let sep = if head.is_empty() || line.is_empty() {
            ""
        } else {
            " "
        };
        self.line(&format!("{head}{sep}{line}"));
        if let Some(stmts) = tail {
            self.nested(|p| p.stmts(stmts));
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        let label = |l: &Option<Name>| l.as_ref().map(|l| format!("{l}: ")).unwrap_or_default();
        match &s.kind {
            StmtKind::Expr(e) => self.tail_line("", e),
            StmtKind::Assign { place, op, value } => {
                self.tail_line(&format!("{} {op}", expr(place)), value)
            }
            StmtKind::Let {
                mutable,
                ghost,
                pattern: pat,
                ty,
                value,
            } => {
                let g = if *ghost { "ghost " } else { "" };
                let kw = if *mutable { "var" } else { "let" };
                let ty = ty
                    .as_ref()
                    .map(|t| format!(": {}", type_(t)))
                    .unwrap_or_default();
                self.tail_line(&format!("{g}{kw} {}{ty} =", pattern(pat)), value);
            }
            StmtKind::Return(e) => match e {
                Some(e) => self.tail_line("return", e),
                None => self.line("return"),
            },
            StmtKind::Fail(e) => self.tail_line("fail", e),
            StmtKind::Yield(e) => self.tail_line("yield", e),
            StmtKind::Reveal(path) => self.line(&format!("reveal {}", path.join("."))),
            StmtKind::Break(l) => self.line(&labelled("break", l)),
            StmtKind::Continue(l) => self.line(&labelled("continue", l)),
            StmtKind::Assert(e) => self.line(&format!("assert {}", expr(e))),
            StmtKind::Unreachable => self.line("unreachable"),
            StmtKind::Become(e) => self.line(&format!("become {}", expr(e))),
            StmtKind::If {
                branches,
                otherwise,
            } => {
                for (i, (c, body)) in branches.iter().enumerate() {
                    let kw = if i == 0 { "if" } else { "else if" };
                    self.block(&format!("{kw} {}", expr(c)), body);
                }
                if let Some(body) = otherwise {
                    self.block("else", body);
                }
            }
            StmtKind::Match { scrutinee, arms } => {
                self.line(&format!("match {}", expr(scrutinee)));
                self.nested(|p| arms.iter().for_each(|a| p.arm(a)));
            }
            StmtKind::For {
                label: l,
                mutable,
                pattern: pat,
                iter,
                invariants,
                body,
            } => {
                let var = if *mutable { "var " } else { "" };
                let head = format!("{}for {var}{} in {}", label(l), pattern(pat), expr(iter));
                self.loop_body(&head, invariants, body);
            }
            StmtKind::While {
                label: l,
                cond,
                invariants,
                body,
            } => self.loop_body(
                &format!("{}while {}", label(l), expr(cond)),
                invariants,
                body,
            ),
            StmtKind::Loop {
                label: l,
                invariants,
                body,
            } => self.loop_body(&format!("{}loop", label(l)), invariants, body),
            StmtKind::Par { concurrent, body } => {
                self.block(if *concurrent { "par concurrent" } else { "par" }, body)
            }
            StmtKind::Select(arms) => {
                self.line("select");
                self.nested(|p| {
                    for a in arms {
                        let pat = a
                            .pattern
                            .as_ref()
                            .map(|q| format!(" is {}", pattern(q)))
                            .unwrap_or_default();
                        p.arm_body(&format!("{}{pat} ->", expr(&a.op)), &a.body);
                    }
                });
            }
            StmtKind::With { kind, body } => {
                let head = match kind {
                    WithKind::Bind {
                        name,
                        dynamic,
                        value,
                    } => format!(
                        "with {name} = {}{}",
                        if *dynamic { "dyn " } else { "" },
                        expr(value)
                    ),
                    WithKind::Deadline(e) => format!("with deadline({})", expr(e)),
                    WithKind::Timeout(e) => format!("with timeout({})", expr(e)),
                };
                self.block(&head, body);
            }
            StmtKind::Unsafe(body) => self.block("unsafe", body),
            StmtKind::StageFor {
                pattern: pat,
                iter,
                body,
            } => self.block(&format!("#for {} in {}", pattern(pat), expr(iter)), body),
            StmtKind::StageIf {
                cond,
                then,
                otherwise,
            } => {
                self.block(&format!("#if {}", expr(cond)), then);
                if let Some(body) = otherwise {
                    self.block("else", body);
                }
            }
        }
    }

    fn loop_body(&mut self, head: &str, invariants: &[Contract], body: &[Stmt]) {
        self.line(head);
        self.nested(|p| {
            p.contracts(invariants);
            p.stmts(body);
        });
    }
}

fn labelled(kw: &str, l: &Option<Name>) -> String {
    match l {
        Some(l) => format!("{kw} {l}"),
        None => kw.to_owned(),
    }
}

/// The inline text of `e` and the block that follows it on the next lines, if any.
fn expr_tail(e: &Expr) -> (String, Option<&Vec<Stmt>>) {
    match &e.kind {
        ExprKind::Block(stmts) => (String::new(), Some(stmts)),
        ExprKind::ParFor {
            concurrent,
            limit,
            pattern: pat,
            iter,
            body,
        } => {
            let c = if *concurrent { " concurrent" } else { "" };
            let l = limit
                .as_ref()
                .map(|l| format!(" (limit: {})", expr(l)))
                .unwrap_or_default();
            (
                format!("par{c}{l} for {} in {}", pattern(pat), expr(iter)),
                Some(body),
            )
        }
        ExprKind::Fallback(lhs, fb) => match fb.as_ref() {
            Fallback::Block(binder, stmts) => {
                let b = binder
                    .as_ref()
                    .map(|b| format!(" {b} ->"))
                    .unwrap_or_default();
                let force = matches!(strip(lhs).kind, ExprKind::If { .. });
                (
                    format!("{} else{b}", operand(lhs, COND, force)),
                    Some(stmts),
                )
            }
            _ => (expr(e), None),
        },
        _ => (expr(e), None),
    }
}

fn sig_ret(s: &Sig) -> String {
    let mut out = String::new();
    if let Some(r) = &s.ret {
        out.push_str(&format!(" -> {}", type_(r)));
        if !s.from.is_empty() {
            out.push_str(&format!(" from {}", s.from.join(", ")));
        }
    }
    out
}

fn sig_clauses(s: &Sig) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(f) = &s.fail {
        out.push(format!("fail {}", type_(f)));
    }
    if let Some(u) = &s.uses {
        out.push(format!("uses {}", comma(u, cap)));
    }
    out
}

fn sig(s: &Sig) -> String {
    let mut out = format!(
        "{}{}({}){}",
        s.name,
        tparams(&s.tparams),
        params(&s.params),
        sig_ret(s)
    );
    for c in sig_clauses(s) {
        out.push(' ');
        out.push_str(&c);
    }
    out
}

fn tparams(ts: &[TParam]) -> String {
    if ts.is_empty() {
        return String::new();
    }
    let body = comma(ts, |t| match t {
        TParam::Type { name, bounds } if bounds.is_empty() => name.clone(),
        TParam::Type { name, bounds } => format!(
            "{name}: {}",
            bounds
                .iter()
                .map(|b| match b {
                    Bound::Nat => "Nat".to_owned(),
                    Bound::Trait { path, args } if args.is_empty() => path.join("."),
                    Bound::Trait { path, args } =>
                        format!("{}[{}]", path.join("."), comma(args, type_)),
                })
                .collect::<Vec<_>>()
                .join(" + ")
        ),
        TParam::Value { name, ty } => format!("{name}: {}", type_(ty)),
    });
    format!("[{body}]")
}

fn param(p: &Param) -> String {
    match p {
        Param::PosOnly(_) => "/".to_owned(),
        Param::Rest { name, ty, .. } => format!("**{name}: {}", type_(ty)),
        Param::Value {
            mode,
            name,
            ty,
            default,
            ..
        } => {
            let m = match mode {
                ParamMode::Let => "",
                ParamMode::Var => "var ",
                ParamMode::Take => "take ",
                ParamMode::Ghost => "ghost ",
            };
            let ty = ty
                .as_ref()
                .map(|t| format!(": {}", type_(t)))
                .unwrap_or_default();
            let d = default
                .as_ref()
                .map(|d| format!(" = {}", expr(d)))
                .unwrap_or_default();
            format!("{m}{name}{ty}{d}")
        }
    }
}

fn params(ps: &[Param]) -> String {
    comma(ps, param)
}

fn cap(c: &CapItem) -> String {
    match c {
        CapItem::Named { name, ty: None } => name.clone(),
        CapItem::Named { name, ty: Some(t) } => format!("{name}: {t}"),
        CapItem::Set(s) => s.clone(),
        CapItem::None => "none".to_owned(),
    }
}

fn field(f: &Field) -> String {
    let attrs: String = f.attrs.iter().map(|a| attr(a) + " ").collect();
    let d = f
        .default
        .as_ref()
        .map(|d| format!(" = {}", expr(d)))
        .unwrap_or_default();
    format!("{attrs}{}: {}{d}", f.name, type_(&f.ty))
}

fn record(fs: &[Field], positional: Option<usize>) -> String {
    let mut parts: Vec<_> = fs.iter().map(field).collect();
    if let Some(n) = positional {
        parts.insert(n, "/".to_owned());
    }
    if parts.is_empty() {
        return "{}".to_owned();
    }
    format!("{{ {} }}", parts.join(", "))
}

pub fn type_(t: &Type) -> String {
    match &t.kind {
        TypeKind::Path { path, args } if args.is_empty() => path.join("."),
        TypeKind::Path { path, args } => format!(
            "{}[{}]",
            path.join("."),
            comma(args, |a| match a {
                TArg::Type(t) => type_(t),
                TArg::Nat(n) => n.clone(),
            })
        ),
        TypeKind::Dyn { name, args } if args.is_empty() => format!("dyn {name}"),
        TypeKind::Dyn { name, args } => format!("dyn {name}[{}]", comma(args, type_)),
        TypeKind::Unit => "()".to_owned(),
        TypeKind::Tuple(ts) => format!("({})", comma(ts, type_)),
        TypeKind::Ref(t) => format!("Ref[{}]", type_(t)),
        TypeKind::Ptr(t) => format!("Ptr[{}]", type_(t)),
        TypeKind::Vec { lanes, elem } => format!("Vec[{lanes}, {elem}]"),
        TypeKind::Record(fs) if fs.is_empty() => "{}".to_owned(),
        TypeKind::Record(fs) => format!("{{ {} }}", comma(fs, field)),
        TypeKind::Option(t) => match t.kind {
            TypeKind::Union(_) | TypeKind::Fn { .. } => format!("({})?", type_(t)),
            _ => format!("{}?", type_(t)),
        },
        TypeKind::Union(ts) => ts.iter().map(type_).collect::<Vec<_>>().join(" | "),
        TypeKind::Fn {
            par,
            params,
            ret,
            fail,
            uses,
        } => {
            let mut s = format!(
                "{}({}) -> {}",
                if *par { "par " } else { "" },
                comma(params, type_),
                type_(ret)
            );
            if let Some(f) = fail {
                s.push_str(&format!(" fail {}", type_(f)));
            }
            match uses.as_deref() {
                Some([one]) => s.push_str(&format!(" uses {}", cap(one))),
                Some(many) => s.push_str(&format!(" uses ({})", comma(many, cap))),
                None => {}
            }
            s
        }
    }
}

pub fn pattern(p: &Pattern) -> String {
    match &p.kind {
        PatKind::Wild => "_".to_owned(),
        PatKind::Lit(e) => expr(e),
        PatKind::Range { lo, hi, inclusive } => {
            format!(
                "{}{}{}",
                expr(lo),
                if *inclusive { "..=" } else { ".." },
                expr(hi)
            )
        }
        PatKind::Bind(n) => n.clone(),
        PatKind::Ctor { name, args: None } => name.clone(),
        PatKind::Ctor {
            name,
            args: Some(args),
        } => format!(
            "{name}({})",
            comma(args, |a| match a {
                PPat::Pos(p) => pattern(p),
                PPat::Named(n, None) => format!("{n}:"),
                PPat::Named(n, Some(p)) => format!("{n}: {}", pattern(p)),
                PPat::Rest => "..".to_owned(),
            })
        ),
        PatKind::Tuple(ps) => format!("({})", comma(ps, pattern)),
        PatKind::Slice(ps) => format!(
            "[{}]",
            comma(ps, |s| match s {
                SPat::Pat(p) => pattern(p),
                SPat::Rest(None) => "..".to_owned(),
                SPat::Rest(Some(n)) => format!("..{n}"),
            })
        ),
        PatKind::Or(ps) => ps.iter().map(pattern).collect::<Vec<_>>().join(" | "),
        PatKind::As(p, n) => format!("{} as {n}", pattern(p)),
    }
}

// Precedence levels, loosest first (15.2).
const LAMBDA: u8 = 0;
const FALLBACK: u8 = 1;
const COND: u8 = 2;
const OR: u8 = 3;
const AND: u8 = 4;
const NOT: u8 = 5;
const COMPARE: u8 = 6;
const UNARY: u8 = 13;
const POWER: u8 = 14;
const CAST: u8 = 15;
const POSTFIX: u8 = 16;
const ATOM: u8 = 17;

fn binop_prec(op: BinOp) -> u8 {
    match op {
        BinOp::Or => OR,
        BinOp::And => AND,
        BinOp::BitOr => 7,
        BinOp::BitXor => 8,
        BinOp::BitAnd => 9,
        BinOp::Shl | BinOp::Shr => 10,
        BinOp::Add | BinOp::Sub => 11,
        BinOp::Mul | BinOp::Div | BinOp::FloorDiv | BinOp::Mod | BinOp::MatMul => 12,
        BinOp::Pow => POWER,
        _ => COMPARE,
    }
}

fn binop_text(op: BinOp) -> &'static str {
    match op {
        BinOp::Or => "or",
        BinOp::And => "and",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::In => "in",
        BinOp::NotIn => "not in",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::BitAnd => "&",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::FloorDiv => "//",
        BinOp::Mod => "%",
        BinOp::MatMul => "@",
        BinOp::Pow => "**",
    }
}

fn strip(e: &Expr) -> &Expr {
    match &e.kind {
        ExprKind::Paren(inner) => strip(inner),
        _ => e,
    }
}

fn prec(e: &Expr) -> u8 {
    match &e.kind {
        ExprKind::Lambda { .. } => LAMBDA,
        ExprKind::Fallback(..) => FALLBACK,
        ExprKind::If { .. } => COND,
        ExprKind::Binary(op, ..) => binop_prec(*op),
        ExprKind::Unary(UnOp::Not, _) => NOT,
        ExprKind::Is { .. } => COMPARE,
        ExprKind::Unary(..) => UNARY,
        ExprKind::Cast(..) => CAST,
        ExprKind::Field(..)
        | ExprKind::TypeField(..)
        | ExprKind::Reflect(..)
        | ExprKind::Call(..)
        | ExprKind::Index(..)
        | ExprKind::Question(..)
        | ExprKind::Stage(_) => POSTFIX,
        ExprKind::Paren(inner) => prec(inner),
        _ => ATOM,
    }
}

/// `e` in a slot that needs at least `min`, parenthesized also when `force`.
fn operand(e: &Expr, min: u8, force: bool) -> String {
    let e = strip(e);
    let s = expr(e);
    if force || prec(e) < min {
        format!("({s})")
    } else {
        s
    }
}

fn binop_of(e: &Expr) -> Option<BinOp> {
    match strip(e).kind {
        ExprKind::Binary(op, ..) => Some(op),
        _ => None,
    }
}

/// Whether E101 requires parentheses around `child` as an operand of `op`.
fn e101(op: BinOp, child: &Expr) -> bool {
    let Some(c) = binop_of(child) else {
        return false;
    };
    let term = |k: BinOp| matches!(k, BinOp::Mul | BinOp::Div | BinOp::FloorDiv | BinOp::Mod);
    match op {
        BinOp::Or => c == BinOp::And,
        BinOp::And => c == BinOp::Or,
        _ if op.is_bitwise() => {
            c != op && (c.is_bitwise() || c.is_arithmetic() || c.is_comparison())
        }
        _ if op.is_comparison() => c.is_bitwise(),
        _ if term(op) => term(c) && ((op == BinOp::Mod) != (c == BinOp::Mod)),
        _ => false,
    }
}

fn str_lit(s: &StrLit) -> String {
    let prefix = match s.kind {
        StrKind::Plain => "",
        StrKind::Raw => "r",
        StrKind::Bytes => "b",
        StrKind::C => "c",
        StrKind::Template => "t",
    };
    let mut out = format!("{prefix}\"");
    for part in &s.parts {
        match part {
            StrPart::Text(t) if s.kind == StrKind::Raw => out.push_str(t),
            StrPart::Text(t) => {
                for c in t.chars() {
                    match c {
                        '{' | '}' if s.kind == StrKind::Template => {
                            out.push(c);
                            out.push(c);
                        }
                        c => out.push_str(&escape(c, '"')),
                    }
                }
            }
            StrPart::Interp { expr: e, spec } => {
                let spec = spec.as_ref().map(|s| format!(":{s}")).unwrap_or_default();
                if s.kind == StrKind::Template {
                    out.push_str(&format!("{{{}{spec}}}", expr(e)));
                } else {
                    out.push_str(&format!("\\({}{spec})", expr(e)));
                }
            }
        }
    }
    out.push('"');
    out
}

fn escape(c: char, quote: char) -> String {
    match c {
        '\\' => "\\\\".to_owned(),
        '\n' => "\\n".to_owned(),
        '\r' => "\\r".to_owned(),
        '\t' => "\\t".to_owned(),
        '\0' => "\\0".to_owned(),
        c if c == quote => format!("\\{c}"),
        c if c.is_control() || crate::lexer::forbidden(c) => format!("\\u{{{:x}}}", c as u32),
        c => c.to_string(),
    }
}

fn quoted(s: &str) -> String {
    let body: String = s.chars().map(|c| escape(c, '"')).collect();
    format!("\"{body}\"")
}

fn args(a: &[Arg]) -> String {
    comma(a, |a| {
        let v = match &a.kind {
            ArgKind::Value(e) => subject(e).unwrap_or_else(|| expr(e)),
            ArgKind::Ref(e) => format!("&{}", expr(e)),
            ArgKind::Become(e) => format!("become {}", expr(e)),
            ArgKind::Pun => String::new(),
        };
        match &a.label {
            Some(l) if v.is_empty() => format!("{l}:"),
            Some(l) => format!("{l}: {v}"),
            None => v,
        }
    })
}

/// `x -> x.a.b` prints as the implicit subject `.a.b` (15.4).
fn subject(e: &Expr) -> Option<String> {
    let ExprKind::Lambda { params, body } = &e.kind else {
        return None;
    };
    let [param] = params.as_slice() else {
        return None;
    };
    let mut fields = Vec::new();
    let mut cur = body.as_ref();
    while let ExprKind::Field(inner, f) = &cur.kind {
        fields.push(f.as_str());
        cur = inner;
    }
    match &cur.kind {
        ExprKind::Name(n) if n == param && !fields.is_empty() => {
            fields.reverse();
            Some(fields.iter().map(|f| format!(".{f}")).collect())
        }
        _ => None,
    }
}

fn ixarg(a: &IxArg) -> String {
    match a {
        IxArg::Expr(e) => expr(e),
        IxArg::Slice { lo, hi, inclusive } => format!(
            "{}{}{}",
            lo.as_ref()
                .map(|e| operand(e, 7, false))
                .unwrap_or_default(),
            if *inclusive { "..=" } else { ".." },
            hi.as_ref()
                .map(|e| operand(e, 7, false))
                .unwrap_or_default()
        ),
        IxArg::Labeled(n, inner) => format!("{n}: {}", ixarg(inner)),
    }
}

fn diverge(s: &Stmt) -> String {
    match &s.kind {
        StmtKind::Return(None) => "return".to_owned(),
        StmtKind::Return(Some(e)) => format!("return {}", operand(e, COND, false)),
        StmtKind::Fail(e) => format!("fail {}", operand(e, COND, false)),
        StmtKind::Break(l) => labelled("break", l),
        StmtKind::Continue(l) => labelled("continue", l),
        _ => "unreachable".to_owned(),
    }
}

pub fn expr(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Int(s) | ExprKind::Float(s) => s.clone(),
        ExprKind::UnitNum { num, unit } => format!("{num}{unit}"),
        ExprKind::Temporal { text, .. } => text.clone(),
        ExprKind::Str(s) => str_lit(s),
        ExprKind::Char(c) => format!(
            "c'{}'",
            c.chars().map(|c| escape(c, '\'')).collect::<String>()
        ),
        ExprKind::Bool(true) => "True".to_owned(),
        ExprKind::Bool(false) => "False".to_owned(),
        ExprKind::NoneLit => "None".to_owned(),
        ExprKind::Name(n) | ExprKind::TypeName(n) => n.clone(),
        ExprKind::Hole => "_".to_owned(),
        ExprKind::SelfValue => "self".to_owned(),
        ExprKind::Subject(n) => format!(".{n}"),
        ExprKind::Paren(inner) => expr(inner),
        ExprKind::Unit => "()".to_owned(),
        ExprKind::Tuple(xs) if xs.len() == 1 => format!("({},)", expr(&xs[0])),
        ExprKind::Tuple(xs) => format!("({})", comma(xs, expr)),
        ExprKind::List(xs) => format!("[{}]", comma(xs, expr)),
        ExprKind::Map(kvs) => format!(
            "{{{}}}",
            comma(kvs, |(k, v)| format!("{}: {}", expr(k), expr(v)))
        ),
        ExprKind::Set(xs) => format!("{{{}}}", comma(xs, expr)),
        ExprKind::Grid(rows) => format!(
            "[{}]",
            rows.iter()
                .map(|r| r.iter().map(grid_elem).collect::<Vec<_>>().join(" "))
                .collect::<Vec<_>>()
                .join("; ")
        ),
        ExprKind::Stage(inner) => format!("#{}", operand(inner, POSTFIX, false)),
        ExprKind::Asm { template, args } => {
            let mut parts = vec![quoted(template)];
            parts.extend(args.iter().map(|a| match a {
                AsmArg::Operand { dir, reg, value } => {
                    format!("{dir}({}) {}", asm_reg(reg), expr(value))
                }
                AsmArg::Typed { dir, reg, name, ty } => {
                    format!("{dir}({}) {name}: {}", asm_reg(reg), type_(ty))
                }
                AsmArg::ClobberAbi(s) => format!("clobber_abi({})", quoted(s)),
                AsmArg::Option(n) => n.clone(),
            }));
            format!("asm({})", parts.join(", "))
        }
        ExprKind::Field(inner, n) => format!("{}.{n}", operand(inner, POSTFIX, false)),
        ExprKind::TypeField(inner, n) => format!("{}.{n}", operand(inner, POSTFIX, false)),
        ExprKind::Reflect(inner, n) => format!("{}.({n})", operand(inner, POSTFIX, false)),
        ExprKind::Call(f, a) => format!("{}({})", operand(f, POSTFIX, false), args(a)),
        ExprKind::Index(x, a) => format!("{}[{}]", operand(x, POSTFIX, false), comma(a, ixarg)),
        ExprKind::Question(inner) => format!("{}?", operand(inner, POSTFIX, false)),
        ExprKind::Cast(inner, t) => format!("{} as {}", operand(inner, CAST, false), type_(t)),
        ExprKind::Unary(op, inner) => {
            let (text, min) = match op {
                UnOp::Neg => ("-", UNARY),
                UnOp::Pos => ("+", UNARY),
                UnOp::BitNot => ("~", UNARY),
                UnOp::Not => ("not ", NOT),
                UnOp::Copy => ("copy ", UNARY),
                UnOp::Share => ("share ", UNARY),
                UnOp::Try => ("try ", UNARY),
            };
            let force = *op == UnOp::Neg && binop_of(inner) == Some(BinOp::Pow);
            format!("{text}{}", operand(inner, min, force))
        }
        ExprKind::Binary(op, l, r) => {
            let p = binop_prec(*op);
            let (lmin, rmin) = match *op {
                BinOp::Pow => (CAST, UNARY),
                _ if op.is_comparison() => (p + 1, p + 1),
                _ => (p, p + 1),
            };
            format!(
                "{} {} {}",
                operand(l, lmin, e101(*op, l)),
                binop_text(*op),
                operand(r, rmin, e101(*op, r))
            )
        }
        ExprKind::Is {
            value,
            negated,
            pattern: p,
        } => format!(
            "{} is {}{}",
            operand(value, COMPARE + 1, false),
            if *negated { "not " } else { "" },
            pattern(p)
        ),
        ExprKind::If {
            cond,
            then,
            otherwise,
        } => format!(
            "if {} then {} else {}",
            operand(cond, OR, false),
            operand(then, OR, false),
            operand(otherwise, COND, false)
        ),
        ExprKind::Fallback(lhs, fb) => {
            let l = operand(lhs, COND, matches!(strip(lhs).kind, ExprKind::If { .. }));
            let r = match fb.as_ref() {
                Fallback::Value(v) => {
                    let force = matches!(strip(v).kind, ExprKind::If { .. })
                        || binop_of(v).is_some_and(|o| o.is_arithmetic() || o.is_bitwise());
                    operand(v, COND, force)
                }
                Fallback::Bind(n, v) => format!("{n} -> {}", expr(v)),
                Fallback::Diverge(s) => diverge(s),
                Fallback::Block(..) => unreachable!("blocks print through expr_tail"),
            };
            format!("{l} else {r}")
        }
        ExprKind::Lambda { params, body } => match params.as_slice() {
            [one] => format!("{one} -> {}", expr(body)),
            many => format!("({}) -> {}", many.join(", "), expr(body)),
        },
        ExprKind::Block(_) | ExprKind::ParFor { .. } => expr_tail(e).0,
    }
}

fn grid_elem(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Paren(inner) => format!("({})", expr(inner)),
        ExprKind::Unary(op @ (UnOp::Neg | UnOp::Pos), inner) => {
            format!(
                "{}{}",
                if *op == UnOp::Neg { "-" } else { "+" },
                grid_elem(inner)
            )
        }
        _ => expr(e),
    }
}

fn asm_reg(r: &AsmReg) -> String {
    match r {
        AsmReg::Class(n) => n.clone(),
        AsmReg::Explicit(s) => quoted(s),
    }
}
