//! `yan export --python` (17.8): typed Python 3.12 from the raised program.

use std::collections::HashSet;
use std::fmt::Write;
use std::path::Path;
use std::process::ExitCode;

use yan_db::{Database, Loaded};
use yan_hir::*;

mod std_map;
use std_map::Method;

const SHIM: &str = include_str!("export/yan_rt.py");

type R<T> = Result<T, String>;

fn no<T>(what: &str) -> R<T> {
    Err(format!("`{what}` has no Python mapping yet"))
}

pub fn run(file: &Path) -> ExitCode {
    match render(file) {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(code) => code,
    }
}

/// The Python program for `file`; failures are reported before returning.
fn render(file: &Path) -> Result<String, ExitCode> {
    let mut db = Database::new(false, false);
    let src = std::fs::read_to_string(file)
        .map_err(|e| crate::tool_failure(&format!("{}: {e}", file.display())))?;
    // `ex` lines become `ex_of_f_k` functions, which export as pytest tests.
    let (text, _) = crate::examples::augment(&file.display().to_string(), &src, false);
    let f = db.file(file, &text);
    let (m, trivia) = match yan_db::load(&db, f) {
        Ok(Loaded::Ready(m, t, _)) => (m, t),
        Ok(Loaded::Stopped(d)) => {
            crate::print_diags(d);
            return Err(ExitCode::FAILURE);
        }
        Err(msg) => return Err(crate::tool_failure(msg)),
    };
    let raised = raise::raise(m, trivia, &yan_elab::StdInfo)
        .map_err(|raise::Unraised(what)| crate::tool_failure(&format!("raising {what}")))?
        .module;
    module(&raised).map_err(|msg| {
        eprintln!("yan: export: {msg}");
        ExitCode::FAILURE
    })
}

fn module(m: &Module) -> R<String> {
    let mut e = Emitter {
        out: String::new(),
        depth: 0,
        fails: failure_types(m),
        tmp: 0,
        posts: Vec::new(),
    };
    e.out.push_str("from __future__ import annotations\n\n");
    e.out.push_str(SHIM);
    for it in &m.items {
        e.item(it)?;
    }
    if m.items.iter().any(|i| i.kind.name() == "main") {
        e.out
            .push_str("\n\nif __name__ == \"__main__\":\n    run_main(main)\n");
    }
    Ok(e.out)
}

/// Types raised as failures: every `fail` clause of a signature.
fn failure_types(m: &Module) -> HashSet<String> {
    let mut s = HashSet::new();
    for it in &m.items {
        if let ItemKind::Func(f) = &it.kind
            && let Some(r) = &f.ret
        {
            s.extend(r.fail.iter().filter_map(|p| p.last().cloned()));
        }
    }
    s
}

struct Emitter {
    out: String,
    depth: usize,
    fails: HashSet<String>,
    tmp: usize,
    /// The enclosing function's postconditions, checked on `out` at each return.
    posts: Vec<String>,
}

const KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield", "match", "case", "type",
];

fn ident(n: &str) -> String {
    if KEYWORDS.contains(&n) {
        format!("{n}_")
    } else {
        n.to_owned()
    }
}

fn py_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '"' => o.push_str("\\\""),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\x{:02x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn tparams(ps: &[TParam]) -> R<String> {
    if ps.is_empty() {
        return Ok(String::new());
    }
    let names = ps
        .iter()
        .map(|p| match p {
            TParam::Type { name, .. } => Ok(name.clone()),
            TParam::Value { .. } => no("value generic parameter"),
        })
        .collect::<R<Vec<_>>>()?;
    Ok(format!("[{}]", names.join(", ")))
}

fn ty(t: &Type) -> R<String> {
    let (path, args) = match t {
        Type::None => return Ok("None".into()),
        Type::Callable { params, ret, .. } => {
            let ps = params.iter().map(ty).collect::<R<Vec<_>>>()?;
            return Ok(format!("Callable[[{}], {}]", ps.join(", "), ty(ret)?));
        }
        Type::Call { .. } => return no("type-function application"),
        Type::Named { path, args } => (path, args),
    };
    let args = args
        .iter()
        .map(|a| match a {
            TArg::Type(t) => ty(t),
            TArg::Int(_) | TArg::Text(_) => no("non-type generic argument"),
        })
        .collect::<R<Vec<_>>>()?;
    let name = path.join(".");
    let one = |n: &str| format!("{n}[{}]", args.join(", "));
    Ok(match name.as_str() {
        "i8" | "i16" | "i32" | "i64" | "i128" | "u8" | "u16" | "u32" | "u64" | "u128" | "isize"
        | "usize" | "int" | "Int" => "int".into(),
        "f32" | "f64" | "float" => "float".into(),
        "Str" | "str" | "Char" => "str".into(),
        "Bool" | "bool" => "bool".into(),
        "Bytes" => "bytes".into(),
        "List" => one("list"),
        "Map" => one("dict"),
        "Set" => one("set"),
        "tuple" => one("tuple"),
        "Option" if args.len() == 1 => format!("{} | None", args[0]),
        _ if args.is_empty() => name,
        _ => one(&name),
    })
}

fn escape_braces(s: &str) -> String {
    s.replace('{', "{{").replace('}', "}}")
}

fn args(a: &[Arg]) -> R<String> {
    let parts = a
        .iter()
        .map(|a| {
            let v = match &a.kind {
                ArgKind::Expr(e) | ArgKind::Become(e) => expr(e)?,
                ArgKind::Ref(_) => return no("inout argument"),
            };
            Ok(match &a.name {
                Some(n) => format!("{}={v}", ident(n)),
                None => v,
            })
        })
        .collect::<R<Vec<_>>>()?;
    Ok(parts.join(", "))
}

fn lit(l: &Lit) -> R<String> {
    Ok(match l {
        Lit::Int(s) | Lit::Float(s) => s.clone(),
        Lit::Str { value, .. } => py_str(value),
        Lit::Char(c) => py_str(&c.to_string()),
        Lit::Bool(true) => "True".into(),
        Lit::Bool(false) => "False".into(),
        Lit::None => "None".into(),
    })
}

fn comp_clause(target: &[Name], iter: &Expr, cond: &Option<Box<Expr>>) -> R<String> {
    let t = target
        .iter()
        .map(|n| ident(n))
        .collect::<Vec<_>>()
        .join(", ");
    let mut s = format!("for {t} in {}", expr(iter)?);
    if let Some(c) = cond {
        let _ = write!(s, " if {}", expr(c)?);
    }
    Ok(s)
}

/// Rejects a reference to std that has no Python mapping.
fn std_ref(base: &Expr, member: Option<&str>) -> R<()> {
    match &base.kind {
        ExprKind::Name(n) => match std_map::reject_name(n, member) {
            Some(what) => no(&what),
            None => Ok(()),
        },
        _ => Ok(()),
    }
}

/// Methods and functions of std with a Python spelling that differs from the Yan one.
fn call(f: &Expr, a: &[Arg]) -> R<String> {
    if let ExprKind::Attr(recv, name, _) = &f.kind {
        std_ref(recv, Some(name))?;
        let r = expr(recv)?;
        let rest = args(a)?;
        let with_recv = |func: &str| {
            let sep = if rest.is_empty() { "" } else { ", " };
            format!("{func}({r}{sep}{rest})")
        };
        return Ok(match std_map::method(name) {
            Some(Method::Rename(py)) => format!("{r}.{py}({rest})"),
            Some(Method::Shim(func)) => with_recv(func),
            Some(Method::No(why)) => return no(&format!("{name} ({why})")),
            None => format!("{r}.{}({rest})", ident(name)),
        });
    }
    if let ExprKind::Name(n) = &f.kind {
        match (n.as_str(), a) {
            ("Some", [one]) => {
                if let ArgKind::Expr(e) = &one.kind {
                    return expr(e);
                }
            }
            ("Ok", [one]) => {
                if let ArgKind::Expr(e) = &one.kind {
                    return expr(e);
                }
            }
            _ => {}
        }
    }
    Ok(format!("{}({})", expr(f)?, args(a)?))
}

fn expr(x: &Expr) -> R<String> {
    Ok(match &x.kind {
        ExprKind::Name(n) => {
            std_ref(x, None)?;
            ident(n)
        }
        ExprKind::Lit(l) => lit(l)?,
        ExprKind::FStr { template, pieces } => {
            if *template {
                return no("template string");
            }
            let mut s = String::from("f\"");
            for p in pieces {
                match p {
                    FPiece::Lit(t) => {
                        let q = py_str(t);
                        s.push_str(&escape_braces(&q[1..q.len() - 1]));
                    }
                    FPiece::Expr(e, spec) => {
                        let _ = write!(s, "{{{}", expr(e)?);
                        if let Some(sp) = spec {
                            let _ = write!(s, ":{sp}");
                        }
                        s.push('}');
                    }
                }
            }
            s.push('"');
            s
        }
        ExprKind::Tuple(xs) => {
            let v = xs.iter().map(expr).collect::<R<Vec<_>>>()?;
            if v.len() == 1 {
                format!("({},)", v[0])
            } else {
                format!("({})", v.join(", "))
            }
        }
        ExprKind::List(xs) => format!(
            "[{}]",
            xs.iter().map(expr).collect::<R<Vec<_>>>()?.join(", ")
        ),
        ExprKind::Set(xs) if xs.is_empty() => "set()".into(),
        ExprKind::Set(xs) => format!(
            "{{{}}}",
            xs.iter().map(expr).collect::<R<Vec<_>>>()?.join(", ")
        ),
        ExprKind::Dict(kv) => {
            let v = kv
                .iter()
                .map(|(k, v)| Ok(format!("{}: {}", expr(k)?, expr(v)?)))
                .collect::<R<Vec<_>>>()?;
            format!("{{{}}}", v.join(", "))
        }
        ExprKind::Comp {
            kind,
            elem,
            value,
            target,
            iter,
            cond,
            ..
        } => {
            let clause = comp_clause(target, iter, cond)?;
            match (kind, value) {
                (CompKind::List, _) => format!("[{} {clause}]", expr(elem)?),
                (CompKind::Set, _) => format!("{{{} {clause}}}", expr(elem)?),
                (CompKind::Dict, Some(v)) => format!("{{{}: {} {clause}}}", expr(elem)?, expr(v)?),
                (CompKind::Dict, None) => return no("dict comprehension without a value"),
            }
        }
        ExprKind::Attr(e, n, _) => {
            std_ref(e, Some(n))?;
            format!("{}.{}", expr(e)?, ident(n))
        }
        ExprKind::Call(f, a) => call(f, a)?,
        ExprKind::Index(a, i) => format!("{}[{}]", expr(a)?, expr(i)?),
        ExprKind::Slice(a, [lo, hi, step], inclusive) => {
            if *inclusive {
                return no("inclusive slice");
            }
            let p = |o: &Option<Box<Expr>>| -> R<String> {
                o.as_ref().map_or(Ok(String::new()), |e| expr(e))
            };
            let mut s = format!("{}[{}:{}", expr(a)?, p(lo)?, p(hi)?);
            if step.is_some() {
                let _ = write!(s, ":{}", p(step)?);
            }
            s.push(']');
            s
        }
        // Failures are exceptions, so propagation is implicit.
        ExprKind::Try(e) => expr(e)?,
        ExprKind::Unary(op, e) => format!("({op}{})", expr(e)?),
        ExprKind::Not(e) => format!("(not {})", expr(e)?),
        ExprKind::Binary(op, a, b) | ExprKind::Compare(op, a, b) => {
            format!("({} {op} {})", expr(a)?, expr(b)?)
        }
        ExprKind::Cond { then, cond, els } => {
            format!("({} if {} else {})", expr(then)?, expr(cond)?, expr(els)?)
        }
        ExprKind::Fallback(e, fb) => {
            let inner = expr(e)?;
            let handler = match &**fb {
                Fallback::Value(v) => format!("lambda _e: {}", expr(v)?),
                Fallback::Bind(n, v) => format!("lambda {}: {}", ident(n), expr(v)?),
                Fallback::Fail(v) => format!("lambda _e: raise_({})", expr(v)?),
                _ => return no("control-flow fallback inside an expression"),
            };
            format!("else_(lambda: {inner}, {handler})")
        }
        ExprKind::Lambda(ps, body) => {
            let ps = ps.iter().map(|p| ident(p)).collect::<Vec<_>>().join(", ");
            format!("(lambda {ps}: {})", expr(body)?)
        }
        ExprKind::Cast(e, t) => match ty(t)?.as_str() {
            c @ ("int" | "float" | "str") => format!("{c}({})", expr(e)?),
            _ => expr(e)?,
        },
        ExprKind::Hole | ExprKind::Wild => return no("unfilled hole"),
        ExprKind::Reify(_) => return no("try expression"),
        ExprKind::Stage(_) => return no("stage 0 expression"),
    })
}

fn pat(p: &Pat) -> R<String> {
    Ok(match p {
        Pat::Wild => "_".into(),
        Pat::Lit(e) => expr(e)?,
        Pat::Tuple(ps) => format!(
            "({})",
            ps.iter().map(pat).collect::<R<Vec<_>>>()?.join(", ")
        ),
        Pat::As(p, n) => format!("{} as {}", pat(p)?, ident(n)),
        Pat::Path { path, args: None } => {
            let n = path.join(".");
            match n.as_str() {
                "None" | "True" | "False" => n,
                _ if path.len() > 1 => n,
                _ => ident(&n),
            }
        }
        Pat::Path {
            path,
            args: Some(a),
        } => {
            let subs = a
                .iter()
                .map(|(n, p)| {
                    Ok(match n {
                        Some(n) => format!("{}={}", ident(n), pat(p)?),
                        None => pat(p)?,
                    })
                })
                .collect::<R<Vec<_>>>()?;
            format!("{}({})", path.join("."), subs.join(", "))
        }
    })
}

impl Emitter {
    fn line(&mut self, s: &str) {
        for _ in 0..self.depth {
            self.out.push_str("    ");
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn block(&mut self, stmts: &[Stmt]) -> R<()> {
        self.depth += 1;
        let before = self.out.len();
        for s in stmts {
            self.stmt(s)?;
        }
        if self.out.len() == before {
            self.line("pass");
        }
        self.depth -= 1;
        Ok(())
    }

    fn item(&mut self, it: &Item) -> R<()> {
        self.out.push_str("\n\n");
        match &it.kind {
            ItemKind::Func(f) => self.func(f),
            // Lemmas are proof-only (6.4) and never compiled.
            ItemKind::Lemma(_) | ItemKind::Verbatim(_) => {
                self.out.truncate(self.out.len() - 2);
                Ok(())
            }
            ItemKind::Record(r) => self.record(r),
            ItemKind::Type(t) => self.type_def(t),
            ItemKind::Const { name, ty: t, value } => {
                let line = format!("{}: {} = {}", ident(name), ty(t)?, expr(value)?);
                self.line(&line);
                Ok(())
            }
            ItemKind::Trait(_) => no("trait"),
            ItemKind::Impl(_) => no("impl block"),
            ItemKind::Extern(_) => no("extern block"),
        }
    }

    fn fields(&mut self, fields: &[Field]) -> R<()> {
        self.depth += 1;
        if fields.is_empty() {
            self.line("pass");
        }
        for f in fields {
            let mut l = format!("{}: {}", ident(&f.name), ty(&f.ty)?);
            if let Some(d) = &f.default {
                let v = expr(d)?;
                if matches!(
                    d.kind,
                    ExprKind::List(_) | ExprKind::Dict(_) | ExprKind::Set(_)
                ) {
                    let _ = write!(l, " = field(default_factory=lambda: {v})");
                } else {
                    let _ = write!(l, " = {v}");
                }
            }
            self.line(&l);
        }
        self.depth -= 1;
        Ok(())
    }

    fn class(&mut self, name: &str, tps: &str, base: Option<&str>, fields: &[Field]) -> R<()> {
        let base = base.map_or(String::new(), |b| format!("({b})"));
        self.line("@dataclass");
        self.line(&format!("class {name}{tps}{base}:"));
        self.fields(fields)
    }

    fn record(&mut self, r: &Record) -> R<()> {
        let base = self.fails.contains(&r.name).then_some("YanFail");
        self.class(&r.name, &tparams(&r.tparams)?, base, &r.fields)
    }

    fn type_def(&mut self, t: &TypeDef) -> R<()> {
        let tps = tparams(&t.tparams)?;
        match &t.rhs {
            TypeRhs::Sum(vs) => {
                let base = ident(&t.name);
                self.line(&format!("class {base}{tps}:"));
                self.line("    pass");
                for v in vs {
                    self.out.push('\n');
                    let sup = format!("{base}{}", tps_args(&t.tparams));
                    self.class(&v.name, &tps, Some(&sup), &v.fields)?;
                    self.out.push('\n');
                }
                Ok(())
            }
            TypeRhs::Alias(ts) => {
                let parts = ts.iter().map(ty).collect::<R<Vec<_>>>()?;
                self.line(&format!(
                    "type {}{tps} = {}",
                    ident(&t.name),
                    parts.join(" | ")
                ));
                Ok(())
            }
            TypeRhs::Newtype(_) => no("newtype"),
            TypeRhs::Caps(_) => no("capability set"),
        }
    }

    fn func(&mut self, f: &FuncDef) -> R<()> {
        let Some(body) = &f.body else {
            return no("function without a body");
        };
        let mut ps = Vec::new();
        for p in &f.params {
            ps.push(match p {
                Param::Value {
                    name,
                    ty: t,
                    default,
                    ..
                } => {
                    let mut s = format!("{}: {}", ident(name), ty(t)?);
                    if let Some(d) = default {
                        let _ = write!(s, " = {}", expr(d)?);
                    }
                    s
                }
                Param::Slash => "/".into(),
                Param::Pack { name, ty: t } => format!("*{}: {}", ident(name), ty(t)?),
            });
        }
        let ret = match f.ret.as_ref().and_then(|r| r.ty.as_ref()) {
            Some(t) => ty(t)?,
            None => "None".into(),
        };
        let name = match f.name.strip_prefix("ex_of_") {
            Some(rest) => format!("test_ex_of_{rest}"),
            None => ident(&f.name),
        };
        self.line(&format!(
            "def {name}{}({}) -> {ret}:",
            tparams(&f.tparams)?,
            ps.join(", ")
        ));
        self.depth += 1;
        if let Some(d) = &body.doc {
            self.line(&py_str(d));
        }
        for c in &body.contracts {
            if let Contract::Pre(e) = c {
                let l = format!("assert {}", expr(e)?);
                self.line(&l);
            }
        }
        self.posts = body
            .contracts
            .iter()
            .filter_map(|c| match c {
                Contract::Post(e) => Some(expr(e)),
                _ => None,
            })
            .collect::<R<_>>()?;
        self.depth -= 1;
        self.block(&body.stmts)?;
        self.posts.clear();
        Ok(())
    }

    fn target(&self, t: &Expr) -> R<String> {
        match &t.kind {
            ExprKind::Tuple(xs) => Ok(xs.iter().map(expr).collect::<R<Vec<_>>>()?.join(", ")),
            _ => expr(t),
        }
    }

    fn fresh(&mut self) -> String {
        self.tmp += 1;
        format!("_t{}", self.tmp)
    }

    /// `target = e else <control flow>`: the failure or missing value runs the handler's statements.
    fn assign_fallback(&mut self, target: &str, ann: &str, e: &Expr, fb: &Fallback) -> R<()> {
        let tmp = self.fresh();
        let inner = expr(e)?;
        self.line(&format!("{tmp} = attempt(lambda: {inner})"));
        self.line(&format!("if isinstance({tmp}, _Missing):"));
        self.depth += 1;
        match fb {
            Fallback::Return(v) => {
                let v = v.as_ref().map_or(Ok(String::new()), |v| {
                    Ok::<_, String>(format!(" {}", expr(v)?))
                })?;
                self.line(&format!("return{v}"));
            }
            Fallback::Break => self.line("break"),
            Fallback::Continue => self.line("continue"),
            Fallback::Fail(v) => {
                let l = format!("raise {}", expr(v)?);
                self.line(&l);
            }
            Fallback::Value(v) => {
                let l = format!("{target} = {}", expr(v)?);
                self.line(&l);
            }
            Fallback::Bind(n, v) => {
                self.line(&format!("{} = {tmp}.err", ident(n)));
                let l = format!("{target} = {}", expr(v)?);
                self.line(&l);
            }
            Fallback::Block(n, body) => {
                if let Some(n) = n {
                    self.line(&format!("{} = {tmp}.err", ident(n)));
                }
                self.depth -= 1;
                self.block(body)?;
                self.depth += 1;
            }
        }
        self.depth -= 1;
        self.line("else:");
        self.depth += 1;
        self.line(&format!("{target}{ann} = {tmp}"));
        self.depth -= 1;
        Ok(())
    }

    fn stmt(&mut self, s: &Stmt) -> R<()> {
        match &s.kind {
            StmtKind::Expr(e) => {
                let l = expr(e)?;
                self.line(&l);
            }
            StmtKind::Assign {
                target,
                ty: t,
                value,
            } => {
                self.assign(&self.target(target)?, t.as_ref(), value)?;
            }
            StmtKind::Bind {
                target,
                ty: t,
                value,
                ..
            } => self.assign(&ident(target), t.as_ref(), value)?,
            StmtKind::AugAssign { target, op, value } => {
                let op = if op.ends_with('=') {
                    (*op).to_owned()
                } else {
                    format!("{op}=")
                };
                let l = format!("{} {op} {}", expr(target)?, expr(value)?);
                self.line(&l);
            }
            StmtKind::Return(v) if !self.posts.is_empty() => {
                let v = v.as_ref().map_or(Ok("None".to_owned()), expr)?;
                self.line(&format!("out = {v}"));
                for p in self.posts.clone() {
                    self.line(&format!("assert {p}"));
                }
                self.line("return out");
            }
            StmtKind::Return(v) => {
                let l = match v {
                    Some(v) => format!("return {}", expr(v)?),
                    None => "return".into(),
                };
                self.line(&l);
            }
            StmtKind::Become(e) => {
                let l = format!("return {}", expr(e)?);
                self.line(&l);
            }
            StmtKind::Fail(e) => {
                let l = format!("raise {}", expr(e)?);
                self.line(&l);
            }
            StmtKind::Break => self.line("break"),
            StmtKind::Continue => self.line("continue"),
            StmtKind::Pass => self.line("pass"),
            StmtKind::Assert(e) => {
                let l = format!("assert {}", expr(e)?);
                self.line(&l);
            }
            StmtKind::Unreachable => self.line("raise AssertionError(\"unreachable\")"),
            // Ghost code is erased before code generation (6.4).
            StmtKind::Ghost { .. } | StmtKind::Reveal(_) => {}
            StmtKind::If { arms, els } => {
                for (i, (c, b)) in arms.iter().enumerate() {
                    let kw = if i == 0 { "if" } else { "elif" };
                    let l = format!("{kw} {}:", expr(c)?);
                    self.line(&l);
                    self.block(&b.stmts)?;
                }
                if let Some(b) = els {
                    self.line("else:");
                    self.block(&b.stmts)?;
                }
            }
            StmtKind::While {
                label: None,
                cond,
                body,
                ..
            } => {
                let l = format!("while {}:", expr(cond)?);
                self.line(&l);
                self.block(body)?;
            }
            StmtKind::For {
                label: None,
                target,
                iter,
                body,
                ..
            } => {
                let t = target
                    .iter()
                    .map(|n| ident(n))
                    .collect::<Vec<_>>()
                    .join(", ");
                let l = format!("for {t} in {}:", expr(iter)?);
                self.line(&l);
                self.block(body)?;
            }
            StmtKind::While { .. } | StmtKind::For { .. } => return no("labeled loop"),
            StmtKind::BreakTo(_) | StmtKind::ContinueTo(_) => return no("labeled break"),
            StmtKind::Match { scrutinee, cases } => {
                let l = format!("match {}:", expr(scrutinee)?);
                self.line(&l);
                self.depth += 1;
                for c in cases {
                    let mut l = format!("case {}", pat(&c.pat)?);
                    if let Some(g) = &c.guard {
                        let _ = write!(l, " if {}", expr(g)?);
                    }
                    l.push(':');
                    self.line(&l);
                    self.block(&c.body.stmts)?;
                }
                self.depth -= 1;
            }
            StmtKind::With {
                kind: WithKind::Bind(n, e),
                body,
            } => {
                let l = format!("{} = {}", ident(n), expr(e)?);
                self.line(&l);
                for s in &body.stmts {
                    self.stmt(s)?;
                }
            }
            StmtKind::With { .. } => return no("with deadline or timeout"),
            StmtKind::Par { .. } => return no("par block"),
            StmtKind::Unsafe(_) | StmtKind::Asm { .. } => return no("unsafe block"),
            StmtKind::StageIf { .. } | StmtKind::StageFor { .. } => return no("stage 0 statement"),
        }
        Ok(())
    }

    fn assign(&mut self, target: &str, t: Option<&Type>, value: &Expr) -> R<()> {
        let ann = match t {
            Some(t) if !target.contains([',', '.', '[']) => format!(": {}", ty(t)?),
            _ => String::new(),
        };
        if let ExprKind::Fallback(e, fb) = &value.kind
            && !matches!(
                **fb,
                Fallback::Value(_) | Fallback::Fail(_) | Fallback::Bind(..)
            )
        {
            return self.assign_fallback(target, &ann, e, fb);
        }
        let l = format!("{target}{ann} = {}", expr(value)?);
        self.line(&l);
        Ok(())
    }
}

fn tps_args(ps: &[TParam]) -> String {
    if ps.is_empty() {
        return String::new();
    }
    let names: Vec<&str> = ps
        .iter()
        .filter_map(|p| match p {
            TParam::Type { name, .. } => Some(name.as_str()),
            TParam::Value { .. } => None,
        })
        .collect();
    format!("[{}]", names.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;

    fn golden(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../conformance/golden/{name}.yag"))
    }

    fn expected(path: &Path) -> String {
        let src = std::fs::read_to_string(path).unwrap();
        src.lines()
            .skip_while(|l| *l != "# stdout:")
            .skip(1)
            .map_while(|l| l.strip_prefix("# "))
            .map(|l| format!("{l}\n"))
            .collect()
    }

    /// The golden program exported, run under python3, prints what the header says and its
    /// `ex` tests pass.
    fn check(name: &str) {
        check_path(name, &golden(name), true);
    }

    fn check_path(name: &str, path: &Path, run_tests: bool) {
        let py = render(path).unwrap_or_else(|_| panic!("export of {name} failed"));
        let dir = std::env::temp_dir().join(format!("yan-export-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("prog.py");
        std::fs::write(&file, py).unwrap();
        let out = Command::new("python3").arg(&file).output().unwrap();
        assert!(
            out.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            expected(path),
            "{name}"
        );
        if !run_tests {
            std::fs::remove_dir_all(dir).unwrap();
            return;
        }
        let harness = "import importlib.util, sys\n\
            sp = importlib.util.spec_from_file_location('m', sys.argv[1])\n\
            m = importlib.util.module_from_spec(sp); sys.modules['m'] = m; sp.loader.exec_module(m)\n\
            [getattr(m, n)() for n in dir(m) if n.startswith('test_')]\n";
        let t = Command::new("python3")
            .args(["-c", harness])
            .arg(&file)
            .output()
            .unwrap();
        assert!(t.status.success(), "{}", String::from_utf8_lossy(&t.stderr));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn g1_hello() {
        check("g1_hello");
    }

    #[test]
    fn g2_vwap() {
        check("g2_vwap");
    }

    #[test]
    fn g4_word_count() {
        check("g4_word_count");
    }

    /// Sampled `conformance/yag/run` cases covering the std mapping: text, list, math, iter.
    #[test]
    fn sampled_run_cases() {
        let cases = [
            "collections/dict_insertion_order",
            "collections/list_show",
            "strings/concat",
            "lenient/n36_char_as_str",
            "std/list_sort",
            "std/list_sort_key",
            "std/list_bisect",
            "std/bisect_key",
            "std/list_insert_clamp",
            "std/text_count_char_predicates",
            "std/text_and_list_additions",
            "std/math_constants",
            "std/math_domain",
            "std/math_exact_ops",
            "std/min_max_key",
            "std/str_iter",
            "std/iter_for_loop",
            "std/iter_adapters",
            "std/iter_consumers",
            "std/iter_single_pass",
        ];
        for c in cases {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../conformance/yag/run/{c}.yag"));
            check_path(c, &path, false);
        }
    }
}
