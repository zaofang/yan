//! `yan import --python` (17.8): a typed Python subset to Yan Agent text.

use std::collections::HashMap;
use std::fmt::Write;
use std::path::Path;
use std::process::ExitCode;

use rustpython_parser::ast::Ranged;
use rustpython_parser::{Parse, ast};

type R<T> = Result<T, String>;

pub fn run(file: &Path) -> ExitCode {
    let src = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => return crate::tool_failure(&format!("{}: {e}", file.display())),
    };
    match convert(&src) {
        Ok((text, notes)) => {
            for note in notes {
                eprintln!("{}:{note}", file.display());
            }
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(msg) => crate::tool_failure(&format!("{}: {msg}", file.display())),
    }
}

pub fn convert(src: &str) -> R<(String, Vec<String>)> {
    let suite = ast::Suite::parse(src, "<python>").map_err(|e| e.to_string())?;
    let mut cx = Cx::new(src);
    let (tests, defs): (Vec<_>, Vec<_>) = suite.iter().partition(|s| is_test(s));
    for s in &defs {
        if let ast::Stmt::FunctionDef(f) = s {
            cx.sigs
                .insert(f.name.to_string(), shared_params(src, &f.args));
        }
    }
    let mut examples: HashMap<String, Vec<String>> = HashMap::new();
    for t in tests {
        let ast::Stmt::FunctionDef(f) = t else {
            continue;
        };
        for s in &f.body {
            let (target, line) = cx.example(s)?;
            examples.entry(target).or_default().push(line);
        }
    }
    let mut out = String::new();
    for (i, s) in defs.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        match s {
            ast::Stmt::FunctionDef(f) => cx.function(&mut out, f, &mut examples)?,
            other => {
                return Err(cx.unsupported(other.range().start().to_usize(), "top-level statement"));
            }
        }
    }
    if let Some(name) = examples.keys().next() {
        return Err(format!(
            "test refers to `{name}`, which is not defined in this file"
        ));
    }
    Ok((out, cx.notes))
}

fn is_test(s: &ast::Stmt) -> bool {
    matches!(s, ast::Stmt::FunctionDef(f) if f.name.starts_with("test_"))
}

/// Parameter names with a flag for sharing a type with another parameter; those go by keyword (E210).
fn shared_params(src: &str, a: &ast::Arguments) -> Vec<(String, bool)> {
    let all: Vec<_> = a.args.iter().map(|p| &p.def).collect();
    let ty = |p: &ast::Arg| p.annotation.as_ref().map(|t| &src[t.range()]);
    all.iter()
        .map(|p| {
            (
                p.arg.to_string(),
                all.iter().filter(|q| ty(q) == ty(p)).count() > 1,
            )
        })
        .collect()
}

struct Cx<'a> {
    src: &'a str,
    sigs: HashMap<String, Vec<(String, bool)>>,
    tmp: usize,
    notes: Vec<String>,
    /// One entry per enclosing loop: the `else` flag set before each `break` (N44).
    breaks: Vec<Option<String>>,
    flags: usize,
    /// Bare annotations waiting for the first assignment of their name (N48).
    pending: HashMap<String, String>,
    ret: String,
}

impl<'a> Cx<'a> {
    fn new(src: &'a str) -> Self {
        Cx {
            src,
            sigs: HashMap::new(),
            tmp: 0,
            notes: Vec::new(),
            breaks: Vec::new(),
            flags: 0,
            pending: HashMap::new(),
            ret: String::new(),
        }
    }

    fn line_of(&self, offset: usize) -> usize {
        self.src[..offset].matches('\n').count() + 1
    }

    fn unsupported(&self, offset: usize, what: &str) -> String {
        format!(
            "line {}: {what} is outside the typed subset",
            self.line_of(offset)
        )
    }

    fn note(&mut self, offset: usize, code: &str, what: &str) {
        let line = self.line_of(offset);
        self.notes.push(format!("line {line}: {code} {what}"));
    }

    fn function(
        &mut self,
        out: &mut String,
        f: &ast::StmtFunctionDef,
        examples: &mut HashMap<String, Vec<String>>,
    ) -> R<()> {
        let at = f.range.start().to_usize();
        if !f.decorator_list.is_empty() {
            return Err(self.unsupported(at, "a decorator"));
        }
        let a = &f.args;
        if a.vararg.is_some()
            || a.kwarg.is_some()
            || !a.kwonlyargs.is_empty()
            || !a.posonlyargs.is_empty()
        {
            return Err(self.unsupported(at, "this parameter form"));
        }
        let mut ps = Vec::new();
        for p in &a.args {
            let Some(t) = &p.def.annotation else {
                return Err(self.unsupported(at, "an unannotated parameter"));
            };
            if p.default.is_some() {
                return Err(self.unsupported(at, "a default value"));
            }
            ps.push(format!("{}: {}", p.def.arg, self.ty(t)?));
        }
        let Some(ret) = &f.returns else {
            return Err(self.unsupported(at, "a missing return annotation"));
        };
        self.ret = self.ty(ret)?;
        self.pending.clear();
        writeln!(out, "def {}({}) -> {}:", f.name, ps.join(", "), self.ret).unwrap();
        let mut body = f.body.as_slice();
        if let Some(ast::Stmt::Expr(e)) = body.first()
            && let ast::Expr::Constant(c) = &*e.value
            && let ast::Constant::Str(doc) = &c.value
        {
            writeln!(out, "    \"\"\"{doc}\"\"\"").unwrap();
            body = &body[1..];
        }
        for ex in examples.remove(f.name.as_str()).unwrap_or_default() {
            writeln!(out, "    ex {ex}").unwrap();
        }
        self.block(out, body, 1)
    }

    fn example(&mut self, s: &ast::Stmt) -> R<(String, String)> {
        let at = s.range().start().to_usize();
        let ast::Stmt::Assert(a) = s else {
            return Err(self.unsupported(at, "a statement in a test other than assert"));
        };
        let ast::Expr::Compare(c) = &*a.test else {
            return Err(self.unsupported(at, "this assert form"));
        };
        let ([op], [rhs]) = (c.ops.as_slice(), c.comparators.as_slice()) else {
            return Err(self.unsupported(at, "a chained comparison"));
        };
        let ast::Expr::Call(call) = &*c.left else {
            return Err(self.unsupported(at, "an assert whose left side is not a call"));
        };
        let ast::Expr::Name(n) = &*call.func else {
            return Err(self.unsupported(at, "an assert on a non-function call"));
        };
        let op = match op {
            ast::CmpOp::Eq | ast::CmpOp::Is => "==",
            ast::CmpOp::NotEq | ast::CmpOp::IsNot => "!=",
            _ => return Err(self.unsupported(at, "this assert comparison")),
        };
        let line = format!("{} {op} {}", self.expr(&c.left)?, self.expr(rhs)?);
        Ok((n.id.to_string(), line))
    }

    fn block(&mut self, out: &mut String, body: &[ast::Stmt], depth: usize) -> R<()> {
        if body.is_empty() {
            return Err("empty block".into());
        }
        for s in body {
            self.stmt(out, s, depth)?;
        }
        Ok(())
    }

    fn line(out: &mut String, depth: usize, text: &str) {
        writeln!(out, "{}{text}", "    ".repeat(depth)).unwrap();
    }

    fn stmt(&mut self, out: &mut String, s: &ast::Stmt, d: usize) -> R<()> {
        let at = s.range().start().to_usize();
        match s {
            ast::Stmt::Return(r) => match &r.value {
                Some(v) => match &**v {
                    ast::Expr::ListComp(c) if c.generators.len() > 1 => {
                        let name = self.fresh();
                        let ret = self.ret.clone();
                        self.note(at, "N45", "nested comprehension rewritten as loops");
                        Self::line(out, d, &format!("{name}: {ret} = []"));
                        self.comp_loops(out, c, &name, d)?;
                        Self::line(out, d, &format!("return {name}"));
                    }
                    v => Self::line(out, d, &format!("return {}", self.expr(v)?)),
                },
                None => Self::line(out, d, "return"),
            },
            ast::Stmt::Pass(_) => Self::line(out, d, "pass"),
            ast::Stmt::Break(_) => {
                if let Some(Some(flag)) = self.breaks.last() {
                    Self::line(out, d, &format!("{flag} = True"));
                }
                Self::line(out, d, "break");
            }
            ast::Stmt::Continue(_) => Self::line(out, d, "continue"),
            ast::Stmt::Expr(e) => Self::line(out, d, &self.expr(&e.value)?),
            ast::Stmt::AugAssign(a) => {
                let op = binop(&a.op).ok_or_else(|| self.unsupported(at, "this operator"))?;
                Self::line(
                    out,
                    d,
                    &format!("{} {op}= {}", self.expr(&a.target)?, self.expr(&a.value)?),
                );
            }
            ast::Stmt::AnnAssign(a) => {
                let Some(v) = &a.value else {
                    let ast::Expr::Name(n) = &*a.target else {
                        return Err(self.unsupported(at, "a bare annotation on a non-name"));
                    };
                    let ty = self.ty(&a.annotation)?;
                    self.pending.insert(n.id.to_string(), ty);
                    self.note(at, "N48", "bare annotation moved to the first assignment");
                    return Ok(());
                };
                if let ast::Expr::ListComp(c) = &**v
                    && c.generators.len() > 1
                {
                    let target = self.expr(&a.target)?;
                    let ty = self.ty(&a.annotation)?;
                    self.note(at, "N45", "nested comprehension rewritten as loops");
                    Self::line(out, d, &format!("{target}: {ty} = []"));
                    return self.comp_loops(out, c, &target, d);
                }
                Self::line(
                    out,
                    d,
                    &format!(
                        "{}: {} = {}",
                        self.expr(&a.target)?,
                        self.ty(&a.annotation)?,
                        self.expr(v)?
                    ),
                );
            }
            ast::Stmt::Assign(a) => self.assign(out, a, d, at)?,
            ast::Stmt::If(i) => self.if_chain(out, i, d, "if")?,
            ast::Stmt::While(w) => {
                let flag = self.loop_else(out, d, at, &w.orelse);
                Self::line(out, d, &format!("while {}:", self.expr(&w.test)?));
                self.breaks.push(flag.clone());
                self.block(out, &w.body, d + 1)?;
                self.breaks.pop();
                self.else_body(out, d, flag, &w.orelse)?;
            }
            ast::Stmt::For(f) => {
                let flag = self.loop_else(out, d, at, &f.orelse);
                Self::line(
                    out,
                    d,
                    &format!(
                        "for {} in {}:",
                        self.target(&f.target)?,
                        self.expr(&f.iter)?
                    ),
                );
                self.breaks.push(flag.clone());
                self.block(out, &f.body, d + 1)?;
                self.breaks.pop();
                self.else_body(out, d, flag, &f.orelse)?;
            }
            _ => return Err(self.unsupported(at, "this statement")),
        }
        Ok(())
    }

    /// N44: a loop `else` runs when no `break` did; a flag records the breaks.
    fn loop_else(
        &mut self,
        out: &mut String,
        d: usize,
        at: usize,
        orelse: &[ast::Stmt],
    ) -> Option<String> {
        if orelse.is_empty() {
            return None;
        }
        let flag = if self.flags == 0 {
            "broke".to_string()
        } else {
            format!("broke{}", self.flags)
        };
        self.flags += 1;
        self.note(at, "N44", "loop else rewritten with a break flag");
        Self::line(out, d, &format!("{flag} = False"));
        Some(flag)
    }

    fn else_body(
        &mut self,
        out: &mut String,
        d: usize,
        flag: Option<String>,
        orelse: &[ast::Stmt],
    ) -> R<()> {
        if let Some(flag) = flag {
            Self::line(out, d, &format!("if not {flag}:"));
            self.block(out, orelse, d + 1)?;
        }
        Ok(())
    }

    /// N45: `name.append(elt)` under one `for` (and `if`) per clause.
    fn comp_loops(
        &mut self,
        out: &mut String,
        c: &ast::ExprListComp,
        name: &str,
        mut d: usize,
    ) -> R<()> {
        for g in &c.generators {
            if g.is_async {
                return Err(self.unsupported(c.range.start().to_usize(), "an async comprehension"));
            }
            Self::line(
                out,
                d,
                &format!(
                    "for {} in {}:",
                    self.target(&g.target)?,
                    self.expr(&g.iter)?
                ),
            );
            d += 1;
            for cond in &g.ifs {
                Self::line(out, d, &format!("if {}:", self.expr(cond)?));
                d += 1;
            }
        }
        Self::line(out, d, &format!("{name}.append({})", self.expr(&c.elt)?));
        Ok(())
    }

    fn if_chain(&mut self, out: &mut String, i: &ast::StmtIf, d: usize, kw: &str) -> R<()> {
        Self::line(out, d, &format!("{kw} {}:", self.expr(&i.test)?));
        self.block(out, &i.body, d + 1)?;
        match i.orelse.as_slice() {
            [] => {}
            [ast::Stmt::If(next)] => self.if_chain(out, next, d, "elif")?,
            rest => {
                Self::line(out, d, "else:");
                self.block(out, rest, d + 1)?;
            }
        }
        Ok(())
    }

    fn assign(&mut self, out: &mut String, a: &ast::StmtAssign, d: usize, at: usize) -> R<()> {
        if let [first, rest @ ..] = a.targets.as_slice()
            && !rest.is_empty()
        {
            if a.targets.iter().any(|t| matches!(t, ast::Expr::Tuple(_))) {
                return Err(self.unsupported(at, "a chained assignment with unpacking"));
            }
            self.note(at, "N46", "chained assignment split");
            let first_text = self.target(first)?;
            self.simple_assign(out, d, first, &first_text, &self.expr(&a.value)?);
            for t in rest {
                let text = self.target(t)?;
                self.simple_assign(out, d, t, &text, &first_text);
            }
            return Ok(());
        }
        let [target] = a.targets.as_slice() else {
            return Err(self.unsupported(at, "an assignment without a target"));
        };
        if let ast::Expr::Tuple(ts) = target
            && ts.elts.iter().any(|t| matches!(t, ast::Expr::Starred(_)))
        {
            return self.starred(out, ts, &a.value, d, at);
        }
        let (ast::Expr::Tuple(ts), ast::Expr::Tuple(vs)) = (target, &*a.value) else {
            let value = self.expr(&a.value)?;
            let text = self.target(target)?;
            self.simple_assign(out, d, target, &text, &value);
            return Ok(());
        };
        if ts.elts.len() != vs.elts.len() {
            return Err(self.unsupported(at, "an unpacking of unequal length"));
        }
        let names = ts
            .elts
            .iter()
            .map(|t| self.target(t))
            .collect::<R<Vec<_>>>()?;
        let vals = vs
            .elts
            .iter()
            .map(|v| self.expr(v))
            .collect::<R<Vec<_>>>()?;
        // Simultaneous assignment: bind through temporaries unless no value reads a target.
        let reads_target = vals.iter().any(|v| names.iter().any(|n| mentions(v, n)));
        if !reads_target {
            for (n, v) in names.iter().zip(&vals) {
                Self::line(out, d, &format!("{n} = {v}"));
            }
            return Ok(());
        }
        let temps: Vec<String> = names.iter().map(|_| self.fresh()).collect();
        for (t, v) in temps.iter().zip(&vals) {
            Self::line(out, d, &format!("{t} = {v}"));
        }
        for (n, t) in names.iter().zip(&temps) {
            Self::line(out, d, &format!("{n} = {t}"));
        }
        Ok(())
    }

    /// `target = value`; the first assignment of a bare-annotated name carries the annotation (N48).
    fn simple_assign(
        &mut self,
        out: &mut String,
        d: usize,
        target: &ast::Expr,
        text: &str,
        value: &str,
    ) {
        if let ast::Expr::Name(n) = target
            && let Some(ty) = self.pending.remove(n.id.as_str())
        {
            Self::line(out, d, &format!("{text}: {ty} = {value}"));
        } else {
            Self::line(out, d, &format!("{text} = {value}"));
        }
    }

    /// N47: `a, *rest, z = xs` by indexing and slicing a list.
    fn starred(
        &mut self,
        out: &mut String,
        ts: &ast::ExprTuple,
        value: &ast::Expr,
        d: usize,
        at: usize,
    ) -> R<()> {
        let stars = ts
            .elts
            .iter()
            .filter(|t| matches!(t, ast::Expr::Starred(_)))
            .count();
        if stars != 1 {
            return Err(self.unsupported(at, "two starred targets"));
        }
        self.note(at, "N47", "starred unpacking rewritten with indexing");
        let src = match value {
            ast::Expr::Name(n) => n.id.to_string(),
            v => {
                let t = self.fresh();
                Self::line(out, d, &format!("{t} = {}", self.expr(v)?));
                t
            }
        };
        let k = ts
            .elts
            .iter()
            .position(|t| matches!(t, ast::Expr::Starred(_)))
            .unwrap();
        let m = ts.elts.len() - k - 1;
        for (i, t) in ts.elts.iter().enumerate() {
            let rhs = match t {
                _ if i < k => format!("{src}[{i}]"),
                ast::Expr::Starred(_) => match (k, m) {
                    (0, 0) => format!("{src}[:]"),
                    (k, 0) => format!("{src}[{k}:]"),
                    (0, m) => format!("{src}[:len({src}) - {m}]"),
                    (k, m) => format!("{src}[{k}:len({src}) - {m}]"),
                },
                _ => format!("{src}[len({src}) - {}]", ts.elts.len() - i),
            };
            let inner = match t {
                ast::Expr::Starred(s) => &*s.value,
                t => t,
            };
            let text = self.target(inner)?;
            self.simple_assign(out, d, inner, &text, &rhs);
        }
        Ok(())
    }

    fn fresh(&mut self) -> String {
        let t = if self.tmp == 0 {
            "t".to_string()
        } else {
            format!("t{}", self.tmp)
        };
        self.tmp += 1;
        t
    }

    fn target(&self, e: &ast::Expr) -> R<String> {
        match e {
            ast::Expr::Name(_) | ast::Expr::Subscript(_) | ast::Expr::Attribute(_) => self.expr(e),
            ast::Expr::Tuple(t) => Ok(t
                .elts
                .iter()
                .map(|x| self.target(x))
                .collect::<R<Vec<_>>>()?
                .join(", ")),
            _ => Err(self.unsupported(e.range().start().to_usize(), "this assignment target")),
        }
    }

    fn ty(&self, e: &ast::Expr) -> R<String> {
        match e {
            ast::Expr::Name(n) => Ok(n.id.to_string()),
            ast::Expr::Constant(c) if matches!(c.value, ast::Constant::None) => Ok("None".into()),
            ast::Expr::Subscript(s) => {
                let args = match &*s.slice {
                    ast::Expr::Tuple(t) => {
                        t.elts.iter().map(|x| self.ty(x)).collect::<R<Vec<_>>>()?
                    }
                    one => vec![self.ty(one)?],
                };
                Ok(format!("{}[{}]", self.ty(&s.value)?, args.join(", ")))
            }
            ast::Expr::BinOp(b) if matches!(b.op, ast::Operator::BitOr) => {
                Ok(format!("{} | {}", self.ty(&b.left)?, self.ty(&b.right)?))
            }
            _ => Err(self.unsupported(e.range().start().to_usize(), "this type expression")),
        }
    }

    fn expr(&self, e: &ast::Expr) -> R<String> {
        let at = e.range().start().to_usize();
        Ok(match e {
            ast::Expr::Name(n) => n.id.to_string(),
            ast::Expr::Constant(c) => match &c.value {
                ast::Constant::None => "None".into(),
                ast::Constant::Bool(b) => if *b { "True" } else { "False" }.into(),
                ast::Constant::Int(i) => i.to_string(),
                ast::Constant::Float(f) => format!("{f:?}"),
                ast::Constant::Str(s) => str_lit(s),
                _ => return Err(self.unsupported(at, "this literal")),
            },
            ast::Expr::BinOp(b) => {
                let op = binop(&b.op).ok_or_else(|| self.unsupported(at, "this operator"))?;
                format!("{} {op} {}", self.atom(&b.left)?, self.atom(&b.right)?)
            }
            ast::Expr::UnaryOp(u) => {
                let op = match u.op {
                    ast::UnaryOp::Not => "not ",
                    ast::UnaryOp::USub => "-",
                    ast::UnaryOp::UAdd => "+",
                    ast::UnaryOp::Invert => "~",
                };
                format!("{op}{}", self.atom(&u.operand)?)
            }
            ast::Expr::BoolOp(b) => {
                let op = if matches!(b.op, ast::BoolOp::And) {
                    " and "
                } else {
                    " or "
                };
                let vs = b
                    .values
                    .iter()
                    .map(|v| self.atom(v))
                    .collect::<R<Vec<_>>>()?;
                vs.join(op)
            }
            ast::Expr::Compare(c) => {
                let mut s = self.expr(&c.left)?;
                for (op, r) in c.ops.iter().zip(&c.comparators) {
                    write!(s, " {} {}", cmpop(op), self.expr(r)?).unwrap();
                }
                s
            }
            ast::Expr::IfExp(i) => {
                format!(
                    "{} if {} else {}",
                    self.atom(&i.body)?,
                    self.atom(&i.test)?,
                    self.atom(&i.orelse)?
                )
            }
            ast::Expr::List(l) => format!("[{}]", self.exprs(&l.elts)?),
            ast::Expr::Tuple(t) => format!("({})", self.exprs(&t.elts)?),
            ast::Expr::Set(s) => format!("{{{}}}", self.exprs(&s.elts)?),
            ast::Expr::Dict(d) => {
                let mut kv = Vec::new();
                for (k, v) in d.keys.iter().zip(&d.values) {
                    let Some(k) = k else {
                        return Err(self.unsupported(at, "dict unpacking"));
                    };
                    kv.push(format!("{}: {}", self.expr(k)?, self.expr(v)?));
                }
                format!("{{{}}}", kv.join(", "))
            }
            ast::Expr::Subscript(s) => {
                format!("{}[{}]", self.expr(&s.value)?, self.expr(&s.slice)?)
            }
            ast::Expr::Attribute(a) => format!("{}.{}", self.expr(&a.value)?, a.attr),
            ast::Expr::ListComp(c) => {
                let [g] = c.generators.as_slice() else {
                    return Err(self.unsupported(
                        at,
                        "a nested comprehension outside an annotated assignment or return",
                    ));
                };
                if g.is_async {
                    return Err(self.unsupported(at, "an async comprehension"));
                }
                let mut s = format!(
                    "[{} for {} in {}",
                    self.expr(&c.elt)?,
                    self.target(&g.target)?,
                    self.expr(&g.iter)?
                );
                for cond in &g.ifs {
                    write!(s, " if {}", self.expr(cond)?).unwrap();
                }
                s.push(']');
                s
            }
            ast::Expr::Call(c) => self.call(c, at)?,
            _ => return Err(self.unsupported(at, "this expression")),
        })
    }

    /// An operand: compound expressions are parenthesized.
    fn atom(&self, e: &ast::Expr) -> R<String> {
        let text = self.expr(e)?;
        Ok(match e {
            ast::Expr::BinOp(_)
            | ast::Expr::BoolOp(_)
            | ast::Expr::IfExp(_)
            | ast::Expr::Compare(_) => {
                format!("({text})")
            }
            ast::Expr::UnaryOp(u) if matches!(u.op, ast::UnaryOp::Not) => format!("({text})"),
            _ => text,
        })
    }

    fn exprs(&self, es: &[ast::Expr]) -> R<String> {
        Ok(es
            .iter()
            .map(|x| self.expr(x))
            .collect::<R<Vec<_>>>()?
            .join(", "))
    }

    fn call(&self, c: &ast::ExprCall, at: usize) -> R<String> {
        let mut args = Vec::new();
        let params = match &*c.func {
            ast::Expr::Name(n) => self.sigs.get(n.id.as_str()),
            _ => None,
        };
        for (i, a) in c.args.iter().enumerate() {
            let v = self.expr(a)?;
            match params.and_then(|p| p.get(i)) {
                Some((p, true)) => args.push(format!("{p}={v}")),
                _ => args.push(v),
            }
        }
        for k in &c.keywords {
            let Some(name) = &k.arg else {
                return Err(self.unsupported(at, "keyword unpacking"));
            };
            args.push(format!("{name}={}", self.expr(&k.value)?));
        }
        Ok(format!("{}({})", self.expr(&c.func)?, args.join(", ")))
    }
}

fn binop(op: &ast::Operator) -> Option<&'static str> {
    Some(match op {
        ast::Operator::Add => "+",
        ast::Operator::Sub => "-",
        ast::Operator::Mult => "*",
        ast::Operator::Div => "/",
        ast::Operator::FloorDiv => "//",
        ast::Operator::Mod => "%",
        ast::Operator::Pow => "**",
        _ => return None,
    })
}

fn cmpop(op: &ast::CmpOp) -> &'static str {
    match op {
        ast::CmpOp::Eq => "==",
        ast::CmpOp::NotEq => "!=",
        ast::CmpOp::Lt => "<",
        ast::CmpOp::LtE => "<=",
        ast::CmpOp::Gt => ">",
        ast::CmpOp::GtE => ">=",
        ast::CmpOp::Is => "is",
        ast::CmpOp::IsNot => "is not",
        ast::CmpOp::In => "in",
        ast::CmpOp::NotIn => "not in",
    }
}

fn str_lit(s: &str) -> String {
    format!("{s:?}")
}

fn mentions(expr: &str, name: &str) -> bool {
    expr.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .any(|w| w == name)
}

#[cfg(test)]
mod tests {
    use super::convert;

    #[test]
    fn oracle_programs_match_hand_written_bodies() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../bench/r567/oracle");
        for name in [
            "below_zero",
            "filter_by_substring",
            "greatest_common_divisor",
        ] {
            let py = std::fs::read_to_string(format!("{dir}/{name}.py")).unwrap();
            let (out, _) = convert(&py).unwrap();
            assert!(out.starts_with(&format!("def {name}(")), "{name}");
            assert!(out.contains("    ex "), "{name}");
        }
    }

    #[test]
    fn same_typed_parameters_go_by_keyword() {
        let (out, _) = convert("def f(a: int, b: int) -> int:\n    return a\n\ndef test_f() -> None:\n    assert f(1, 2) == 1\n").unwrap();
        assert!(out.contains("ex f(a=1, b=2) == 1"));
    }

    #[test]
    fn unsupported_constructs_name_their_line() {
        let err = convert("import os\n").unwrap_err();
        assert!(err.starts_with("line 1:"), "{err}");
        let err = convert("def f(x) -> int:\n    return 1\n").unwrap_err();
        assert!(err.contains("unannotated"), "{err}");
    }

    fn body(src: &str) -> (String, Vec<String>) {
        let (out, notes) = convert(&format!("def f(xs: list[int]) -> int:\n{src}")).unwrap();
        (out.split_once(":\n").unwrap().1.to_string(), notes)
    }

    #[test]
    fn loop_else_uses_a_break_flag() {
        let (out, notes) = body(
            "    for x in xs:\n        if x > 1:\n            break\n    else:\n        return 0\n    return 1\n",
        );
        assert_eq!(
            out,
            "    broke = False\n    for x in xs:\n        if x > 1:\n            broke = True\n            break\n    if not broke:\n        return 0\n    return 1\n"
        );
        assert!(notes[0].starts_with("line 2: N44"), "{notes:?}");
    }

    #[test]
    fn inner_loop_break_does_not_set_outer_flag() {
        let (out, _) = body(
            "    while True:\n        for x in xs:\n            break\n        break\n    else:\n        return 0\n    return 1\n",
        );
        assert_eq!(out.matches("broke = True").count(), 1, "{out}");
    }

    #[test]
    fn nested_comprehension_becomes_loops() {
        let (out, notes) = body(
            "    ys: list[int] = [x + y for x in xs for y in xs if y > x]\n    return len(ys)\n",
        );
        assert_eq!(
            out,
            "    ys: list[int] = []\n    for x in xs:\n        for y in xs:\n            if y > x:\n                ys.append(x + y)\n    return len(ys)\n"
        );
        assert!(notes[0].contains("N45"));
        let (out, _) =
            convert("def g(xs: list[int]) -> list[int]:\n    return [x for x in xs for _ in xs]\n")
                .unwrap();
        assert!(
            out.contains("t: list[int] = []") && out.contains("return t"),
            "{out}"
        );
    }

    #[test]
    fn chained_assignment_evaluates_once() {
        let (out, notes) = body("    a = b = len(xs)\n    return a + b\n");
        assert_eq!(out, "    a = len(xs)\n    b = a\n    return a + b\n");
        assert!(notes[0].contains("N46"));
    }

    #[test]
    fn starred_unpacking_indexes() {
        let (out, notes) = body("    a, *rest = xs\n    *init, last = xs\n    return a\n");
        assert_eq!(
            out,
            "    a = xs[0]\n    rest = xs[1:]\n    init = xs[:len(xs) - 1]\n    last = xs[len(xs) - 1]\n    return a\n"
        );
        assert!(notes.iter().all(|n| n.contains("N47")));
        let (out, _) = body("    a, *m, z = xs\n    return a\n");
        assert!(out.contains("m = xs[1:len(xs) - 1]"), "{out}");
    }

    #[test]
    fn bare_annotation_moves_to_first_assignment() {
        let (out, notes) = body("    n: int\n    n = 1\n    n = 2\n    return n\n");
        assert_eq!(out, "    n: int = 1\n    n = 2\n    return n\n");
        assert!(notes[0].contains("N48"));
    }
}
