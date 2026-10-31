//! The canonical Jian printer (14.3).

use crate::ast::*;

pub fn print(m: &Module) -> String {
    let mut p = Printer { out: String::new() };
    for h in &m.header {
        p.out.push_str(h);
        p.out.push('\n');
    }
    if !m.header.is_empty() {
        p.out.push('\n');
    }
    for a in &m.attrs {
        p.out.push_str(&format!("@@{}\n", decorator(a)));
    }
    if let Some(d) = &m.doc {
        p.out.push_str(&docstring(d));
        p.out.push('\n');
    }
    for (i, it) in m.items.iter().enumerate() {
        if i > 0 || !m.attrs.is_empty() || m.doc.is_some() {
            p.out.push('\n');
        }
        p.item(it);
    }
    p.out
}

struct Printer {
    out: String,
}

fn comma<T>(xs: &[T], f: impl Fn(&T) -> String) -> String {
    xs.iter().map(f).collect::<Vec<_>>().join(", ")
}

fn path(p: &[Name]) -> String {
    p.join(".")
}

fn decorator(d: &Decorator) -> String {
    match &d.args {
        Some(a) => format!("{}({})", path(&d.path), args(a)),
        None => path(&d.path),
    }
}

fn docstring(s: &str) -> String {
    format!(
        "\"\"\"{}\"\"\"",
        s.replace('\\', "\\\\").replace("\"\"\"", "\\\"\\\"\\\"")
    )
}

fn escape(s: &str, braces: bool) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\0' => out.push_str("\\0"),
            '{' if braces => out.push_str("{{"),
            '}' if braces => out.push_str("}}"),
            c => out.push(c),
        }
    }
    out
}

pub fn ty(t: &Type) -> String {
    match t {
        Type::Named { path: p, args } if args.is_empty() => path(p),
        Type::Named { path: p, args } => format!("{}[{}]", path(p), comma(args, targ)),
        Type::Callable {
            par,
            params,
            ret,
            fail,
            uses,
        } => {
            let mut s = format!(
                "{}Callable[[{}], {}",
                if *par { "par " } else { "" },
                comma(params, ty),
                ty(ret)
            );
            if !fail.is_empty() {
                s.push_str(&format!(", fail({})", errtype(fail)));
            }
            if !uses.is_empty() {
                s.push_str(&format!(", uses({})", uses.join(", ")));
            }
            s.push(']');
            s
        }
        Type::Optional(t) => format!("{} | None", ty(t)),
        Type::None => "None".into(),
    }
}

fn targ(a: &TArg) -> String {
    match a {
        TArg::Type(t) => ty(t),
        TArg::Int(s) | TArg::Text(s) => s.clone(),
    }
}

fn errtype(ps: &[Path]) -> String {
    ps.iter().map(|p| path(p)).collect::<Vec<_>>().join(" | ")
}

fn tparams(ps: &[TParam]) -> String {
    if ps.is_empty() {
        return String::new();
    }
    let one = |p: &TParam| match p {
        TParam::Type { name, bounds } if bounds.is_empty() => name.clone(),
        TParam::Type { name, bounds } => {
            let bs: Vec<String> = bounds
                .iter()
                .map(|b| match b {
                    Bound::Nat => "Nat".into(),
                    Bound::Path { path: p, args } if args.is_empty() => path(p),
                    Bound::Path { path: p, args } => format!("{}[{}]", path(p), comma(args, ty)),
                })
                .collect();
            format!("{name}: {}", bs.join(" + "))
        }
        TParam::Value { name, ty: t } => format!("{name}: {}", ty(t)),
    };
    format!("[{}]", comma(ps, one))
}

fn params(ps: &[Param]) -> String {
    comma(ps, |p| match p {
        Param::Slash => "/".into(),
        Param::Pack { name, ty: t } => format!("**{name}: {}", ty(t)),
        Param::Value {
            conv,
            name,
            ty: t,
            default,
            ..
        } => {
            let c = match conv {
                Some(Conv::Var) => "var ",
                Some(Conv::Take) => "take ",
                Some(Conv::Ghost) => "ghost ",
                None => "",
            };
            let d = default
                .as_ref()
                .map(|e| format!(" = {}", expr(e, 0)))
                .unwrap_or_default();
            format!("{c}{name}: {}{d}", ty(t))
        }
    })
}

fn caprefs(cs: &[CapRef]) -> String {
    comma(cs, |c| match &c.alias {
        Some(a) => format!("{} as {a}", path(&c.path)),
        None => path(&c.path),
    })
}

fn signature(f: &FuncDef, kw: &str) -> String {
    let mut s = format!(
        "{kw} {}{}({})",
        f.name,
        tparams(&f.tparams),
        params(&f.params)
    );
    if let Some(r) = &f.ret {
        match (&r.ty, r.fail.is_empty()) {
            (Some(t), true) => s.push_str(&format!(" -> {}", ty(t))),
            (Some(t), false) => s.push_str(&format!(" -> {} fail {}", ty(t), errtype(&r.fail))),
            (None, false) => s.push_str(&format!(" -> fail {}", errtype(&r.fail))),
            (None, true) => {}
        }
        if !r.from.is_empty() {
            s.push_str(&format!(" from {}", r.from.join(", ")));
        }
    }
    match &f.uses {
        Some(Uses::None) => s.push_str(" uses none"),
        Some(Uses::Caps(cs)) => s.push_str(&format!(" uses {}", caprefs(cs))),
        None => {}
    }
    s.push(':');
    s
}

fn args(a: &[Arg]) -> String {
    comma(a, |a| {
        let v = match &a.kind {
            ArgKind::Expr(e) => expr(e, 0),
            ArgKind::Ref(e) => format!("&{}", expr(e, 15)),
            ArgKind::Become(e) => format!("become {}", expr(e, 15)),
        };
        match &a.name {
            Some(n) => format!("{n}={v}"),
            None => v,
        }
    })
}

fn lit(l: &Lit) -> String {
    match l {
        Lit::Int(s) | Lit::Float(s) => s.clone(),
        Lit::Str { prefix, value } if prefix.contains('r') => format!("{prefix}\"{value}\""),
        Lit::Str { prefix, value } => format!("{prefix}\"{}\"", escape(value, false)),
        Lit::Char(c) => match c {
            '\'' => "c'\\''".into(),
            c => format!("c'{}'", escape(&c.to_string(), false)),
        },
        Lit::Bool(true) => "True".into(),
        Lit::Bool(false) => "False".into(),
        Lit::None => "None".into(),
    }
}

/// Binding strength, loosest 0 (lambda) to tightest 15 (postfix and atoms).
fn level(e: &Expr) -> u8 {
    match &e.kind {
        ExprKind::Lambda(..) => 0,
        ExprKind::Fallback(..) => 1,
        ExprKind::Cond { .. } => 2,
        ExprKind::Binary(op, ..) => binary_level(op),
        ExprKind::Not(_) => 5,
        ExprKind::Compare(..) => 6,
        ExprKind::Unary(..) => 13,
        ExprKind::Lit(Lit::Int(s) | Lit::Float(s)) if s.starts_with('-') => 13,
        _ => 15,
    }
}

fn binary_level(op: &str) -> u8 {
    match op {
        "or" => 3,
        "and" => 4,
        "|" => 7,
        "^" => 8,
        "&" => 9,
        "<<" | ">>" => 10,
        "+" | "-" => 11,
        "**" => 14,
        _ => 12,
    }
}

fn bin_op(e: &Expr) -> Option<&'static str> {
    match &e.kind {
        ExprKind::Binary(op, ..) => Some(op),
        _ => None,
    }
}

const BITWISE: [&str; 5] = ["|", "^", "&", "<<", ">>"];

/// Parentheses E101 requires on `child` under `parent` even where precedence would not.
fn required(parent: &str, child: &Expr) -> bool {
    let Some(c) = bin_op(child) else { return false };
    match parent {
        "or" => c == "and",
        "and" => c == "or",
        "%" => ["*", "/", "//"].contains(&c),
        "*" | "/" | "//" => c == "%",
        p if BITWISE.contains(&p) => c != p && (BITWISE.contains(&c) || binary_level(c) >= 11),
        "cmp" => BITWISE.contains(&c),
        _ => false,
    }
}

fn wrap(e: &Expr, min: u8, force: bool) -> String {
    let s = expr(e, 0);
    if force || level(e) < min {
        format!("({s})")
    } else {
        s
    }
}

pub fn expr(e: &Expr, min: u8) -> String {
    let s = match &e.kind {
        ExprKind::Name(n) => n.clone(),
        ExprKind::Lit(l) => lit(l),
        ExprKind::Hole => "...".into(),
        ExprKind::Wild => "_".into(),
        ExprKind::FStr { template, pieces } => {
            let mut s = String::from(if *template { "t\"" } else { "f\"" });
            for p in pieces {
                match p {
                    FPiece::Lit(l) => s.push_str(&escape(l, true)),
                    FPiece::Expr(e, fmt) => {
                        s.push('{');
                        s.push_str(&expr(e, 0));
                        if let Some(f) = fmt {
                            s.push(':');
                            s.push_str(f);
                        }
                        s.push('}');
                    }
                }
            }
            s.push('"');
            s
        }
        ExprKind::Tuple(xs) if xs.len() == 1 => format!("({},)", expr(&xs[0], 0)),
        ExprKind::Tuple(xs) => format!("({})", comma(xs, |x| expr(x, 0))),
        ExprKind::List(xs) => format!("[{}]", comma(xs, |x| expr(x, 0))),
        ExprKind::Set(xs) => format!("{{{}}}", comma(xs, |x| expr(x, 0))),
        ExprKind::Dict(kvs) => format!(
            "{{{}}}",
            comma(kvs, |(k, v)| format!("{}: {}", expr(k, 0), expr(v, 0)))
        ),
        ExprKind::Comp {
            kind,
            par,
            elem,
            value,
            target,
            iter,
            cond,
        } => {
            let (open, close) = if *kind == CompKind::List {
                ("[", "]")
            } else {
                ("{", "}")
            };
            let head = match value {
                Some(v) => format!("{}: {}", expr(elem, 0), expr(v, 0)),
                None => expr(elem, 0),
            };
            let mut s = String::new();
            if let Some(c) = par {
                s.push_str(if *c { "par concurrent " } else { "par " });
            }
            s.push_str(&format!(
                "{open}{head} for {} in {}",
                target.join(", "),
                expr(iter, 3)
            ));
            if let Some(c) = cond {
                s.push_str(&format!(" if {}", expr(c, 3)));
            }
            s.push_str(close);
            s
        }
        ExprKind::Attr(b, n, _) => format!("{}.{n}", expr(b, 15)),
        ExprKind::Call(f, a) => format!("{}({})", expr(f, 15), args(a)),
        ExprKind::Index(b, i) => format!("{}[{}]", expr(b, 15), expr(i, 0)),
        ExprKind::Slice(b, [lo, hi, step], has_step) => {
            let part = |x: &Option<Box<Expr>>| x.as_ref().map(|e| expr(e, 0)).unwrap_or_default();
            let mut s = format!("{}[{}:{}", expr(b, 15), part(lo), part(hi));
            if *has_step {
                s.push(':');
                s.push_str(&part(step));
            }
            s.push(']');
            s
        }
        ExprKind::Try(b) => format!("{}?", expr(b, 15)),
        ExprKind::Unary(op, x) => {
            let force = *op == "-" && bin_op(x) == Some("**");
            format!("{op}{}", wrap(x, 13, force))
        }
        ExprKind::Not(x) => format!("not {}", wrap(x, 5, false)),
        ExprKind::Binary(op, l, r) => {
            let lv = binary_level(op);
            let (lmin, rmin) = if *op == "**" { (15, 13) } else { (lv, lv + 1) };
            format!(
                "{} {op} {}",
                wrap(l, lmin, required(op, l)),
                wrap(r, rmin, required(op, r))
            )
        }
        ExprKind::Compare(op, l, r) => {
            format!(
                "{} {op} {}",
                wrap(l, 7, required("cmp", l)),
                wrap(r, 7, required("cmp", r))
            )
        }
        ExprKind::Cond { then, cond, els } => format!(
            "{} if {} else {}",
            wrap(then, 3, false),
            wrap(cond, 3, false),
            wrap(els, 2, false)
        ),
        ExprKind::Fallback(l, fb) => {
            let left = wrap(l, 2, matches!(l.kind, ExprKind::Cond { .. }));
            let right = match &**fb {
                Fallback::Value(v) => {
                    let force = matches!(v.kind, ExprKind::Cond { .. })
                        || bin_op(v).is_some_and(|o| binary_level(o) >= 7);
                    wrap(v, 2, force)
                }
                Fallback::Bind(n, v) => format!("{n} -> {}", wrap(v, 2, false)),
                Fallback::Return(None) => "return".into(),
                Fallback::Return(Some(v)) => format!("return {}", wrap(v, 2, false)),
                Fallback::Fail(v) => format!("fail {}", wrap(v, 2, false)),
                Fallback::Break => "break".into(),
                Fallback::Continue => "continue".into(),
            };
            format!("{left} else {right}")
        }
        ExprKind::Lambda(ps, b) => {
            if ps.is_empty() {
                format!("lambda: {}", expr(b, 0))
            } else {
                format!("lambda {}: {}", ps.join(", "), expr(b, 0))
            }
        }
    };
    if level(e) < min { format!("({s})") } else { s }
}

/// An expression list at the top of a statement: a tuple prints bare unless written in parentheses.
fn exprs(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Tuple(xs) if !e.paren && xs.len() > 1 => comma(xs, |x| expr(x, 0)),
        _ => expr(e, 0),
    }
}

fn pat(p: &Pat) -> String {
    match p {
        Pat::Wild => "_".into(),
        Pat::Lit(e) => expr(e, 0),
        Pat::Path {
            path: p,
            args: None,
        } => path(p),
        Pat::Path {
            path: p,
            args: Some(a),
        } => format!(
            "{}({})",
            path(p),
            comma(a, |(n, q)| match n {
                Some(n) => format!("{n}={}", pat(q)),
                None => pat(q),
            })
        ),
        Pat::Tuple(ps) => format!("({})", comma(ps, pat)),
        Pat::As(q, n) => format!("{} as {n}", pat(q)),
    }
}

impl Printer {
    fn line(&mut self, depth: usize, s: &str) {
        for _ in 0..depth {
            self.out.push_str("    ");
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn item(&mut self, it: &Item) {
        for d in &it.decorators {
            self.line(0, &format!("@{}", decorator(d)));
        }
        let pubs = if it.is_pub { "pub " } else { "" };
        match &it.kind {
            ItemKind::Func(f) => self.func(f, pubs, "def", 0),
            ItemKind::Lemma(f) => self.func(f, pubs, "lemma", 0),
            ItemKind::Class(c) => {
                self.line(
                    0,
                    &format!("{pubs}class {}{}:", c.name, tparams(&c.tparams)),
                );
                if let Some(d) = &c.doc {
                    self.line(1, &docstring(d));
                }
                if c.fields.is_empty() {
                    self.line(1, "pass");
                }
                for f in &c.fields {
                    self.line(1, &field(f));
                }
            }
            ItemKind::Type(t) => {
                let rhs = match &t.rhs {
                    TypeRhs::Sum(vs) => vs
                        .iter()
                        .map(|v| format!("{}({})", v.name, comma(&v.fields, field)))
                        .collect::<Vec<_>>()
                        .join(" | "),
                    TypeRhs::Alias(ts) => ts.iter().map(ty).collect::<Vec<_>>().join(" | "),
                    TypeRhs::Caps(cs) => format!("uses {}", caprefs(cs)),
                    TypeRhs::Newtype(t) => format!("newtype[{}]", ty(t)),
                };
                let w = t
                    .where_
                    .as_ref()
                    .map(|e| format!(" where {}", expr(e, 0)))
                    .unwrap_or_default();
                self.line(
                    0,
                    &format!("{pubs}type {}{} = {rhs}{w}", t.name, tparams(&t.tparams)),
                );
            }
            ItemKind::Trait(t) => {
                self.line(
                    0,
                    &format!("{pubs}trait {}{}:", t.name, tparams(&t.tparams)),
                );
                if let Some(d) = &t.doc {
                    self.line(1, &docstring(d));
                }
                for a in &t.assoc {
                    self.line(1, &format!("type {a}"));
                }
                for (decs, f) in &t.sigs {
                    for d in decs {
                        self.line(1, &format!("@{}", decorator(d)));
                    }
                    self.func(f, "", "def", 1);
                }
            }
            ItemKind::Impl(i) => {
                let targs = if i.args.is_empty() {
                    String::new()
                } else {
                    format!("[{}]", comma(&i.args, ty))
                };
                self.line(
                    0,
                    &format!(
                        "{pubs}impl {}{targs} for {}:",
                        path(&i.trait_),
                        ty(&i.for_ty)
                    ),
                );
                for (n, t) in &i.assoc {
                    self.line(1, &format!("type {n} = {}", ty(t)));
                }
                for f in &i.fns {
                    self.func(f, "", "def", 1);
                }
            }
            ItemKind::Const { name, ty: t, value } => {
                self.line(0, &format!("{pubs}{name}: {} = {}", ty(t), expr(value, 0)))
            }
        }
    }

    fn func(&mut self, f: &FuncDef, pubs: &str, kw: &str, depth: usize) {
        let sig = signature(f, kw);
        let Some(b) = &f.body else {
            self.line(depth, &format!("{pubs}{sig} ..."));
            return;
        };
        self.line(depth, &format!("{pubs}{sig}"));
        let d = depth + 1;
        if let Some(doc) = &b.doc {
            self.line(d, &docstring(doc));
        }
        let order = |c: &Contract| match c {
            Contract::Pre(_) => 0,
            Contract::Post(_) => 1,
            Contract::Ex { .. } => 2,
            Contract::Decreases(_) => 3,
        };
        let mut cs: Vec<&Contract> = b.contracts.iter().collect();
        cs.sort_by_key(|c| order(c));
        for c in cs {
            let s = match c {
                Contract::Pre(e) => format!("pre {}", expr(e, 0)),
                Contract::Post(e) => format!("post {}", expr(e, 0)),
                Contract::Decreases(e) => format!("decreases {}", expr(e, 0)),
                Contract::Ex { expr: e, with } if with.is_empty() => format!("ex {}", expr(e, 0)),
                Contract::Ex { expr: e, with } => {
                    format!(
                        "ex {} with {}",
                        expr(e, 0),
                        comma(with, |(n, v)| format!("{n} = {}", expr(v, 0)))
                    )
                }
            };
            self.line(d, &s);
        }
        if b.stmts.is_empty() && b.doc.is_none() && b.contracts.is_empty() {
            self.line(d, "pass");
        }
        self.stmts(&b.stmts, d);
    }

    fn stmts(&mut self, ss: &[Stmt], d: usize) {
        for s in ss {
            self.stmt(s, d);
        }
    }

    fn block(&mut self, head: String, b: &Block, d: usize) {
        if b.inline && b.stmts.len() == 1 && simple(&b.stmts[0]).is_some() {
            self.line(
                d,
                &format!("{head} {}", simple(&b.stmts[0]).expect("checked")),
            );
        } else {
            self.line(d, &head);
            self.stmts(&b.stmts, d + 1);
        }
    }

    fn loop_body(&mut self, head: String, cs: &[LoopContract], body: &[Stmt], d: usize) {
        self.line(d, &head);
        for c in cs {
            match c {
                LoopContract::Invariant(e) => {
                    self.line(d + 1, &format!("invariant {}", expr(e, 0)))
                }
                LoopContract::Decreases(e) => {
                    self.line(d + 1, &format!("decreases {}", expr(e, 0)))
                }
            }
        }
        self.stmts(body, d + 1);
    }

    fn stmt(&mut self, s: &Stmt, d: usize) {
        if let Some(line) = simple(s) {
            self.line(d, &line);
            return;
        }
        match &s.kind {
            StmtKind::If { arms, els } => {
                for (i, (c, b)) in arms.iter().enumerate() {
                    let kw = if i == 0 { "if" } else { "elif" };
                    self.block(format!("{kw} {}:", expr(c, 0)), b, d);
                }
                if let Some(b) = els {
                    self.block("else:".into(), b, d);
                }
            }
            StmtKind::While {
                cond,
                contracts,
                body,
            } => self.loop_body(format!("while {}:", expr(cond, 0)), contracts, body, d),
            StmtKind::For {
                target,
                iter,
                contracts,
                body,
            } => self.loop_body(
                format!("for {} in {}:", target.join(", "), expr(iter, 0)),
                contracts,
                body,
                d,
            ),
            StmtKind::Match { scrutinee, cases } => {
                self.line(d, &format!("match {}:", expr(scrutinee, 0)));
                for c in cases {
                    let g = c
                        .guard
                        .as_ref()
                        .map(|g| format!(" if {}", expr(g, 0)))
                        .unwrap_or_default();
                    self.block(format!("case {}{g}:", pat(&c.pat)), &c.body, d + 1);
                }
            }
            StmtKind::With { kind, body } => {
                let head = match kind {
                    WithKind::Bind(n, e) => format!("with {n} = {}:", expr(e, 0)),
                    WithKind::Deadline(e) => format!("with deadline({}):", expr(e, 0)),
                    WithKind::Timeout(e) => format!("with timeout({}):", expr(e, 0)),
                };
                self.block(head, body, d);
            }
            StmtKind::Par {
                concurrent,
                limit,
                body,
            } => {
                let mut head = String::from("par");
                if *concurrent {
                    head.push_str(" concurrent");
                }
                if let Some(l) = limit {
                    head.push_str(&format!("(limit={})", expr(l, 0)));
                }
                match body {
                    ParBody::Block(ss) => {
                        self.line(d, &format!("{head}:"));
                        self.stmts(ss, d + 1);
                    }
                    ParBody::For {
                        target,
                        iter,
                        contracts,
                        body,
                    } => self.loop_body(
                        format!("{head} for {} in {}:", target.join(", "), expr(iter, 0)),
                        contracts,
                        body,
                        d,
                    ),
                }
            }
            _ => unreachable!("simple statements print above"),
        }
    }
}

fn field(f: &Field) -> String {
    match &f.default {
        Some(e) => format!("{}: {} = {}", f.name, ty(&f.ty), expr(e, 0)),
        None => format!("{}: {}", f.name, ty(&f.ty)),
    }
}

/// The one-line text of a simple statement, or `None` for a compound one.
fn simple(s: &Stmt) -> Option<String> {
    Some(match &s.kind {
        StmtKind::Expr(e) => exprs(e),
        StmtKind::Assign {
            target,
            ty: Some(t),
            value,
        } => format!("{}: {} = {}", exprs(target), ty(t), exprs(value)),
        StmtKind::Assign {
            target,
            ty: None,
            value,
        } => format!("{} = {}", exprs(target), exprs(value)),
        StmtKind::AugAssign { target, op, value } => {
            format!("{} {op} {}", exprs(target), expr(value, 0))
        }
        StmtKind::Ghost {
            name,
            ty: Some(t),
            value,
        } => format!("ghost {name}: {} = {}", ty(t), expr(value, 0)),
        StmtKind::Ghost {
            name,
            ty: None,
            value,
        } => format!("ghost {name} = {}", expr(value, 0)),
        StmtKind::Return(None) => "return".into(),
        StmtKind::Return(Some(e)) => format!("return {}", exprs(e)),
        StmtKind::Become(e) => format!("become {}", expr(e, 15)),
        StmtKind::Fail(e) => format!("fail {}", expr(e, 0)),
        StmtKind::Break => "break".into(),
        StmtKind::Continue => "continue".into(),
        StmtKind::Pass => "pass".into(),
        StmtKind::Assert(e) => format!("assert {}", expr(e, 0)),
        StmtKind::Unreachable => "unreachable".into(),
        StmtKind::Reveal(p) => format!("reveal {}", path(p)),
        _ => return None,
    })
}
