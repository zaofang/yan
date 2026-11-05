//! Canonical printer for `.yir` text.
//!
//! SPEC-Q: spec 3.5 fixes the grammar but not the canonical layout. Assumed: four-space
//! indentation, one statement per line, contracts on their own lines before the body's `{`,
//! records and sums on one line, a blank line between items. Only the file's leading
//! comment lines are kept.

use std::fmt::Write;

use crate::ast::*;

pub fn print(m: &Module) -> String {
    let mut p = Printer {
        out: String::new(),
        indent: 0,
    };
    for line in &m.header {
        p.line(line);
    }
    p.line(&format!("module {}", m.path.join(".")));
    for item in &m.items {
        p.out.push('\n');
        p.item(item);
    }
    p.out
}

/// One item's canonical text, as the item hash reads it (3.6).
pub(crate) fn item(i: &Item) -> String {
    let mut p = Printer {
        out: String::new(),
        indent: 0,
    };
    p.item(i);
    p.out
}

struct Printer {
    out: String,
    indent: usize,
}

fn comma<T>(xs: &[T], f: impl Fn(&T) -> String) -> String {
    xs.iter().map(f).collect::<Vec<_>>().join(", ")
}

fn path(p: &[Name]) -> String {
    p.join(".")
}

fn conv(c: Conv) -> &'static str {
    match c {
        Conv::Let => "let",
        Conv::Inout => "inout",
        Conv::Sink => "sink",
    }
}

fn escape(c: char, quote: char) -> String {
    match c {
        '\n' => "\\n".into(),
        '\r' => "\\r".into(),
        '\t' => "\\t".into(),
        '\0' => "\\0".into(),
        '\\' => "\\\\".into(),
        c if c == quote => format!("\\{c}"),
        c if c.is_control() => format!("\\u{{{:x}}}", c as u32),
        c => c.to_string(),
    }
}

fn string(s: &str) -> String {
    format!(
        "\"{}\"",
        s.chars().map(|c| escape(c, '"')).collect::<String>()
    )
}

fn literal(l: &Literal) -> String {
    match l {
        Literal::Int(s) | Literal::Float(s) => s.clone(),
        Literal::Str(s) => string(s),
        Literal::Char(c) => format!("'{}'", escape(*c, '\'')),
        Literal::Bool(b) => b.to_string(),
        Literal::Unit => "()".into(),
    }
}

fn atom(a: &Atom) -> String {
    match a {
        Atom::Name(n, _) => n.clone(),
        Atom::Lit(l, _) => literal(l),
    }
}

fn targs(args: &[TArg]) -> String {
    if args.is_empty() {
        return String::new();
    }
    format!(
        "[{}]",
        comma(args, |a| match a {
            TArg::Type(t) => ty(t),
            TArg::Nat(n) => n.clone(),
        })
    )
}

fn named(n: &Named) -> String {
    format!("{}{}", path(&n.path), targs(&n.args))
}

fn cap_set(c: &CapSet) -> String {
    match c {
        CapSet::None => "none".into(),
        CapSet::Names(ns) => ns.join(", "),
    }
}

fn fn_type(f: &FnType) -> String {
    let mut s = format!(
        "Fn({}) -> {}",
        comma(&f.params, |(c, t)| format!("{} {}", conv(*c), ty(t))),
        ty(&f.ret)
    );
    if let Some(u) = &f.uses {
        write!(s, " uses {}", cap_set(u)).unwrap();
    }
    if let Some(e) = &f.fail {
        write!(s, " fail {}", ty(e)).unwrap();
    }
    s
}

/// Canonical text of a type, as it is printed in a module.
pub fn ty(t: &Type) -> String {
    match t {
        Type::Named(n) => named(n),
        Type::Tuple(ts) => format!("Tuple({})", comma(ts, ty)),
        Type::Dyn(n) => format!("dyn {}", named(n)),
        Type::Fn(f) => fn_type(f),
        Type::Refined { name, ty: t, pred } => format!("{{{name}: {} | {}}}", ty(t), expr(pred)),
        Type::Union(ts) => ts.iter().map(ty).collect::<Vec<_>>().join(" | "),
    }
}

fn call(c: &Call) -> String {
    let args = comma(&c.args, |a| match a {
        Arg::Value(cv, a, _) => format!("{} {}", conv(*cv), atom(a)),
        Arg::Cap(n, _) => format!("cap {n}"),
    });
    format!("{}{}({args})", path(&c.path), targs(&c.targs))
}

fn expr(e: &Expr) -> String {
    match e {
        Expr::Atom(a) => atom(a),
        Expr::Call(c) => call(c),
    }
}

fn place(p: &Place) -> String {
    let mut s = p.root.clone();
    for proj in &p.projs {
        match proj {
            Proj::Field(f, _) => write!(s, ".{f}").unwrap(),
            Proj::Index(i) => write!(s, "[{}]", atom(i)).unwrap(),
        }
    }
    s
}

fn ctor(c: &Ctor) -> String {
    match c {
        Ctor::Named {
            path: p,
            targs: a,
            fields,
        } => {
            let fs = comma(fields, |(n, v)| format!("{n}: {}", atom(v)));
            if fs.is_empty() {
                format!("{}{} {{}}", path(p), targs(a))
            } else {
                format!("{}{} {{ {fs} }}", path(p), targs(a))
            }
        }
        Ctor::Tuple(xs) => format!("({})", comma(xs, atom)),
    }
}

fn param(p: &Param) -> String {
    match p {
        Param::Value {
            comptime,
            conv: c,
            name,
            ty: t,
        } => {
            format!(
                "{}{} {name}: {}",
                if *comptime { "comptime " } else { "" },
                conv(*c),
                ty(t)
            )
        }
        Param::Cap { name, ty: t } => format!("cap {name}: {}", named(t)),
    }
}

fn tparams(ps: &[TParam]) -> String {
    if ps.is_empty() {
        return String::new();
    }
    let one = |p: &TParam| {
        let kind = match p.kind {
            Kind::Type => "Type",
            Kind::Nat => "Nat",
            Kind::Dim => "Dim",
        };
        let mut s = format!(
            "{}{}: {kind}",
            if p.comptime { "comptime " } else { "" },
            p.name
        );
        if !p.bounds.is_empty() {
            write!(
                s,
                " is {}",
                p.bounds
                    .iter()
                    .map(|b| path(b))
                    .collect::<Vec<_>>()
                    .join(" + ")
            )
            .unwrap();
        }
        s
    };
    format!("[{}]", comma(ps, one))
}

fn field(f: &Field) -> String {
    let mut s = format!(
        "{}{}: {}",
        if f.is_pub { "pub " } else { "" },
        f.name,
        ty(&f.ty)
    );
    if let Some(d) = &f.default {
        write!(s, " = {}", const_val(d)).unwrap();
    }
    s
}

fn fields(fs: &[Field]) -> String {
    if fs.is_empty() {
        "{}".into()
    } else {
        format!("{{ {} }}", comma(fs, field))
    }
}

fn const_val(c: &ConstVal) -> String {
    match c {
        ConstVal::Lit(l) => literal(l),
        ConstVal::Path {
            path: p,
            fields: None,
        } => path(p),
        ConstVal::Path {
            path: p,
            fields: Some(fs),
        } if fs.is_empty() => format!("{} {{}}", path(p)),
        ConstVal::Path {
            path: p,
            fields: Some(fs),
        } => {
            format!(
                "{} {{ {} }}",
                path(p),
                comma(fs, |(n, v)| format!("{n}: {}", const_val(v)))
            )
        }
        ConstVal::List(xs) => format!("[{}]", comma(xs, const_val)),
        ConstVal::Tuple(xs) => format!("({})", comma(xs, const_val)),
    }
}

fn pat(p: &Pat) -> String {
    match p {
        Pat::Wild => "_".into(),
        Pat::Lit(l) => literal(l),
        Pat::Bind(n) => n.clone(),
        Pat::Ctor {
            path: p,
            fields: None,
        } => path(p),
        Pat::Ctor {
            path: p,
            fields: Some((fs, rest)),
        } => {
            let mut parts: Vec<String> =
                fs.iter().map(|(n, v)| format!("{n}: {}", pat(v))).collect();
            if *rest {
                parts.push("..".into());
            }
            format!("{} {{ {} }}", path(p), parts.join(", "))
        }
        Pat::Tuple(ps) => format!("({})", comma(ps, pat)),
        Pat::Or(ps) => ps.iter().map(pat).collect::<Vec<_>>().join(" | "),
        Pat::Range(a, b) => format!("{}..={}", literal(a), literal(b)),
        Pat::Slice { elems, rest } => {
            let mut parts: Vec<String> = elems.iter().map(pat).collect();
            match rest {
                Some(Some(n)) => parts.push(format!(".. {n}")),
                Some(None) => parts.push("..".into()),
                None => {}
            }
            format!("[{}]", parts.join(", "))
        }
    }
}

fn attr(a: &Attr) -> String {
    match &a.args {
        None => format!("@{}", a.name),
        Some(args) => format!(
            "@{}({})",
            a.name,
            comma(args, |x| match &x.name {
                Some(n) => format!("{n} = {}", const_val(&x.value)),
                None => const_val(&x.value),
            })
        ),
    }
}

impl Printer {
    fn line(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn block(&mut self, head: &str, b: &Block) {
        if b.stmts.is_empty() {
            self.line(&format!("{head}{{}}"));
            return;
        }
        self.line(&format!("{head}{{"));
        self.body(b);
        self.line("}");
    }

    fn item(&mut self, item: &Item) {
        for a in &item.attrs {
            self.line(&attr(a));
        }
        let vis = |is_pub: bool| if is_pub { "pub " } else { "" };
        match &item.kind {
            ItemKind::Fn(f) => self.function(f, "fn", &tparams(&f.tparams)),
            ItemKind::Gen(f) => self.function(f, "gen", &tparams(&f.tparams)),
            ItemKind::Inst(i) => self.function(&i.def, "inst fn", &targs(&i.args)),
            ItemKind::Record(r) => self.line(&format!(
                "{}record {}{} {}",
                vis(r.is_pub),
                path(&r.path),
                tparams(&r.tparams),
                fields(&r.fields)
            )),
            ItemKind::Sum(s) => {
                let vs = comma(&s.variants, |v| match &v.fields {
                    None => v.name.clone(),
                    Some(fs) => format!("{} {}", v.name, fields(fs)),
                });
                self.line(&format!(
                    "{}sum {}{} {{ {vs} }}",
                    vis(s.is_pub),
                    path(&s.path),
                    tparams(&s.tparams)
                ))
            }
            ItemKind::Newtype(n) => {
                let mut s = format!(
                    "{}newtype {}{} = {}",
                    vis(n.is_pub),
                    path(&n.path),
                    tparams(&n.tparams),
                    ty(&n.ty)
                );
                if let Some(p) = &n.pred {
                    write!(s, " where {}", expr(p)).unwrap();
                }
                self.line(&s)
            }
            ItemKind::Trait(t) => {
                self.line(&format!(
                    "{}trait {}{} {{",
                    vis(t.is_pub),
                    path(&t.path),
                    tparams(&t.tparams)
                ));
                self.indent += 1;
                for a in &t.assoc {
                    self.line(&format!("type {a}"));
                }
                for s in &t.sigs {
                    self.function(s, "fn", &tparams(&s.tparams));
                }
                if let Some(d) = &t.derived {
                    self.block("derived ", d);
                }
                self.indent -= 1;
                self.line("}");
            }
            ItemKind::Impl(i) => {
                let u = if i.is_unsafe { "unsafe " } else { "" };
                self.line(&format!(
                    "{u}impl {} for {} {{",
                    named(&i.trait_),
                    ty(&i.for_ty)
                ));
                self.indent += 1;
                for (n, t) in &i.assoc {
                    self.line(&format!("type {n} = {}", ty(t)));
                }
                for f in &i.fns {
                    self.function(f, "fn", &tparams(&f.tparams));
                }
                self.indent -= 1;
                self.line("}");
            }
            ItemKind::Const(c) => self.line(&format!(
                "{}const {}: {} = {}",
                vis(c.is_pub),
                path(&c.path),
                ty(&c.ty),
                const_val(&c.value)
            )),
            ItemKind::Test(t) => self.block(&format!("test {} ", path(&t.path)), &t.body),
            ItemKind::Extern(e) => {
                let mut head = format!("unsafe extern {} lib {}", string(&e.abi), e.lib);
                if let Some(l) = &e.link {
                    write!(head, " link {}", string(l)).unwrap();
                }
                self.line(&format!("{head} {{"));
                self.indent += 1;
                for x in &e.items {
                    let safety = |s: bool| if s { "safe" } else { "unsafe" };
                    let line = match x {
                        XItem::Fn {
                            safe,
                            name,
                            params,
                            variadic,
                            ret,
                            from,
                            uses_none,
                            nonblocking,
                        } => {
                            let mut ps: Vec<String> = params
                                .iter()
                                .map(|p| match p {
                                    XParam::Value { name, ty: t } => format!("{name}: {}", ty(t)),
                                    XParam::Callback {
                                        name,
                                        escaping,
                                        ty: t,
                                    } => {
                                        format!(
                                            "{name}: {} {}",
                                            if *escaping { "escaping" } else { "scoped" },
                                            fn_type(t)
                                        )
                                    }
                                })
                                .collect();
                            if *variadic {
                                ps.push("...".into());
                            }
                            let mut s = format!(
                                "{} fn {name}({}) -> {}",
                                safety(*safe),
                                ps.join(", "),
                                ty(ret)
                            );
                            if !from.is_empty() {
                                write!(s, " from {}", from.join(", ")).unwrap();
                            }
                            if *uses_none {
                                s.push_str(" uses none");
                            }
                            if *nonblocking {
                                s.push_str(" nonblocking");
                            }
                            s
                        }
                        XItem::Type { name, drop } => format!("type {name} drop {}", path(drop)),
                        XItem::Static { safe, name, ty: t } => {
                            format!("{} static {name}: {}", safety(*safe), ty(t))
                        }
                    };
                    self.line(&line);
                }
                self.indent -= 1;
                self.line("}");
            }
        }
    }

    /// `brackets` is the type parameters, or an instance's arguments.
    fn function(&mut self, f: &FnDef, keyword: &str, brackets: &str) {
        let mut head = String::new();
        if f.is_pub {
            head.push_str("pub ");
        }
        if f.is_unsafe {
            head.push_str("unsafe ");
        }
        write!(
            head,
            "{keyword} {}{brackets}({}) -> {}",
            path(&f.path),
            comma(&f.params, param),
            ty(&f.ret)
        )
        .unwrap();
        if !f.from.is_empty() {
            write!(head, " from {}", f.from.join(", ")).unwrap();
        }
        if let Some(e) = &f.fail {
            write!(head, " fail {}", ty(e)).unwrap();
        }
        let Some(body) = &f.body else {
            self.line(&head);
            self.contracts(&f.contracts);
            return;
        };
        if f.contracts.is_empty() {
            self.block(&format!("{head} "), body);
        } else {
            self.line(&head);
            self.contracts(&f.contracts);
            self.block("", body);
        }
    }

    fn contracts(&mut self, cs: &[Contract]) {
        self.indent += 1;
        for c in cs {
            let s = match c {
                Contract::Requires(e) => format!("requires {}", expr(e)),
                Contract::Ensures(e) => format!("ensures {}", expr(e)),
                Contract::Decreases(es) => format!("decreases {}", comma(es, expr)),
                Contract::Example(e) => format!("example {}", expr(e)),
            };
            self.line(&s);
        }
        self.indent -= 1;
    }

    fn rhs(r: &Rhs) -> Option<String> {
        Some(match r {
            Rhs::Atom(a) => atom(a),
            Rhs::Call(c) => call(c),
            Rhs::Propagate {
                call: c,
                via,
                drops,
            } => {
                let mut s = format!("{}?", call(c));
                if let Some(v) = via {
                    s += &format!(" via {}", path(v));
                }
                if !drops.is_empty() {
                    s += &format!(" drop {}", drops.join(", "));
                }
                s
            }
            Rhs::Try(c) => format!("try {}", call(c)),
            Rhs::Ctor(c) => ctor(c),
            Rhs::Proj(p) => place(p),
            Rhs::Copy(p) => format!("copy {}", place(p)),
            Rhs::Move(p) => format!("move {}", place(p)),
            Rhs::Closure(_) => return None,
        })
    }

    /// Print `head` + rhs; a closure spans several lines.
    fn rhs_line(&mut self, head: &str, r: &Rhs) {
        match (Self::rhs(r), r) {
            (Some(s), _) => self.line(&format!("{head}{s}")),
            (None, Rhs::Closure(c)) => {
                let caps = c
                    .captures
                    .iter()
                    .map(|(cv, n, _)| format!("{} {n}", conv(*cv)))
                    .collect::<Vec<_>>()
                    .join(" ");
                let mut s = format!(
                    "{head}fn [{caps}]({}) -> {}",
                    comma(&c.params, param),
                    ty(&c.ret)
                );
                if let Some(u) = &c.uses {
                    write!(s, " uses {}", cap_set(u)).unwrap();
                }
                if let Some(e) = &c.fail {
                    write!(s, " fail {}", ty(e)).unwrap();
                }
                s.push(' ');
                self.block(&s, &c.body);
            }
            (None, _) => unreachable!("only closures print over several lines"),
        }
    }

    fn body(&mut self, b: &Block) {
        self.indent += 1;
        for s in &b.stmts {
            self.stmt(s);
        }
        self.indent -= 1;
    }

    fn if_else(&mut self, head: &str, then: &Block, els: &Option<Block>) {
        let Some(e) = els else {
            return self.block(head, then);
        };
        self.line(&format!("{head}{{"));
        self.body(then);
        self.line("} else {");
        self.body(e);
        self.line("}");
    }

    fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Let {
                mutable,
                pat: p,
                ty: t,
                rhs,
                ..
            } => self.rhs_line(
                &format!(
                    "{} {}: {} = ",
                    if *mutable { "var" } else { "let" },
                    pat(p),
                    ty(t)
                ),
                rhs,
            ),
            StmtKind::Assign { place: p, rhs, .. } => {
                self.rhs_line(&format!("{} = ", place(p)), rhs)
            }
            StmtKind::If { cond, then, els } => {
                self.if_else(&format!("if {} ", atom(cond)), then, els)
            }
            StmtKind::Match { scrutinee, arms } => {
                self.line(&format!("match {} {{", atom(scrutinee)));
                self.indent += 1;
                for (p, b) in arms {
                    self.block(&format!("{} => ", pat(p)), b);
                }
                self.indent -= 1;
                self.line("}");
            }
            StmtKind::Loop {
                label,
                invariants,
                decreases,
                body,
            } => {
                let mut head = format!("loop '{label}");
                for i in invariants {
                    write!(head, " invariant {}", expr(i)).unwrap();
                }
                if !decreases.is_empty() {
                    write!(head, " decreases {}", comma(decreases, expr)).unwrap();
                }
                head.push(' ');
                self.block(&head, body);
            }
            StmtKind::Break { label, value: None } => self.line(&format!("break '{label}")),
            StmtKind::Break {
                label,
                value: Some(v),
            } => self.line(&format!("break '{label} {}", atom(v))),
            StmtKind::Continue { label } => self.line(&format!("continue '{label}")),
            StmtKind::Jp { name, params, body } => {
                self.block(&format!("jp {name}({}) ", comma(params, param)), body)
            }
            StmtKind::Jump { name, args } => {
                self.line(&format!("jump {name}({})", comma(args, atom)))
            }
            StmtKind::Ret(a) => self.line(&format!("ret {}", atom(a))),
            StmtKind::Fail(a) => self.line(&format!("fail {}", atom(a))),
            StmtKind::Yield(a) => self.line(&format!("yield {}", atom(a))),
            StmtKind::Become(c) => self.line(&format!("become {}", call(c))),
            StmtKind::With {
                name,
                dyn_,
                handler,
                body,
            } => {
                let d = if *dyn_ { "dyn " } else { "" };
                self.block(&format!("with {name} = {d}{} ", atom(handler)), body)
            }
            StmtKind::Deadline { at, body } => self.block(&format!("deadline {} ", atom(at)), body),
            StmtKind::Par { concurrent, blocks } => {
                self.line(if *concurrent {
                    "par concurrent {"
                } else {
                    "par {"
                });
                self.indent += 1;
                for b in blocks {
                    self.block("", b);
                }
                self.indent -= 1;
                self.line("}");
            }
            StmtKind::ComptimeFor {
                name,
                ty: t,
                iter,
                body,
            } => self.block(
                &format!("comptime for {name}: {} in {} ", ty(t), atom(iter)),
                body,
            ),
            StmtKind::ComptimeIf { cond, then, els } => {
                self.if_else(&format!("comptime if {} ", atom(cond)), then, els)
            }
            StmtKind::Assert(e) => self.line(&format!("assert {}", expr(e))),
            StmtKind::Unreachable => self.line("unreachable"),
            StmtKind::Unsafe(b) => self.block("unsafe ", b),
            StmtKind::Asm { template, operands } => {
                let mut parts = vec![string(template)];
                for o in operands {
                    parts.push(match o {
                        AsmOperand::Reg { dir, reg, value } => {
                            let d = match dir {
                                AsmDir::In => "in",
                                AsmDir::Out => "out",
                                AsmDir::Inout => "inout",
                                AsmDir::Lateout => "lateout",
                            };
                            let r = match reg {
                                AsmReg::Class(n) => n.clone(),
                                AsmReg::Explicit(s) => string(s),
                            };
                            format!("{d}({r}) {}", atom(value))
                        }
                        AsmOperand::ClobberAbi(s) => format!("clobber_abi({})", string(s)),
                        AsmOperand::Options(ns) => format!("options({})", ns.join(", ")),
                    });
                }
                self.line(&format!("asm({})", parts.join(", ")))
            }
            StmtKind::Drop(n) => self.line(&format!("drop {n}")),
            StmtKind::Reuse { name, ctor: c } => self.line(&format!("reuse {name} as {}", ctor(c))),
            StmtKind::Rhs(r) => self.rhs_line("", r),
        }
    }
}
