//! Checked Yong to MIR.

use std::collections::HashMap;

use yong_core::ast as y;
use yong_std::registry::{self, id};

use crate::*;

type R<T> = Result<T, String>;

fn unsupported<T>(what: impl std::fmt::Display) -> R<T> {
    Err(format!("{what} in native code"))
}

/// The MIR of `m`, or what it uses that native code cannot run yet.
pub fn lower(m: &y::Module) -> R<Program> {
    let defs: Vec<&y::FnDef> = m
        .items
        .iter()
        .filter_map(|i| match &i.kind {
            y::ItemKind::Fn(f) if f.body.is_some() => Some(f),
            _ => None,
        })
        .collect();
    if let Some(i) = m.items.iter().find(|i| {
        !matches!(
            i.kind,
            y::ItemKind::Fn(_) | y::ItemKind::Record(_) | y::ItemKind::Sum(_)
        )
    }) {
        return unsupported(format!("item at line {}", i.span.line));
    }
    let ids: HashMap<String, FuncId> = defs
        .iter()
        .enumerate()
        .map(|(i, f)| (f.path.join("."), i as FuncId))
        .collect();
    let mut prog = Program {
        funcs: Vec::new(),
        main: *ids.get("main").ok_or("no `fn main` in the module")?,
        strings: Vec::new(),
    };
    let sigs: Vec<Vec<Ty>> = defs
        .iter()
        .map(|f| params(f).map(|ps| ps.into_iter().map(|(_, t)| t).collect()))
        .collect::<R<_>>()?;
    for f in &defs {
        let func = Fn::new(&ids, &sigs, &mut prog.strings).func(f)?;
        prog.funcs.push(func);
    }
    Ok(prog)
}

fn params(f: &y::FnDef) -> R<Vec<(&str, Ty)>> {
    f.params
        .iter()
        .filter_map(|p| match p {
            y::Param::Value {
                conv: y::Conv::Inout,
                ..
            } => Some(unsupported("an `inout` parameter")),
            y::Param::Value { comptime: true, .. } => Some(unsupported("a comptime parameter")),
            y::Param::Value { name, ty, .. } => Some(ty_of(ty).map(|t| (name.as_str(), t))),
            y::Param::Cap { .. } => None,
        })
        .collect()
}

fn scalar_ty(name: &str) -> Option<Ty> {
    let int = |bits, signed| Some(Ty::Int(IntTy { bits, signed }));
    match name {
        "i8" => int(8, true),
        "i16" => int(16, true),
        "i32" => int(32, true),
        "i64" => int(64, true),
        "u8" => int(8, false),
        "u16" => int(16, false),
        "u32" => int(32, false),
        "u64" | "Addr" => int(64, false),
        "f64" => Some(Ty::F64),
        "bool" => Some(Ty::Bool),
        "Unit" => Some(Ty::Unit),
        _ => None,
    }
}

fn ty_of(t: &y::Type) -> R<Ty> {
    match t {
        y::Type::Named(n) if n.args.is_empty() && n.path.len() == 1 => {
            match scalar_ty(&n.path[0]) {
                Some(t) => Ok(t),
                None => unsupported(format!("type `{}`", n.path[0])),
            }
        }
        _ => unsupported("a compound type"),
    }
}

/// `<op>_<ty>` as an op and its operand type.
fn scalar(path: &str) -> Option<(ScalarOp, Option<Ty>)> {
    use ScalarOp::*;
    if path == "exclusive_check" {
        return Some((Exclusive, None));
    }
    let (op, ty) = path.rsplit_once('_')?;
    let op = match op {
        "add_checked" => AddChecked,
        "sub_checked" => SubChecked,
        "mul_checked" => MulChecked,
        "floordiv" => FloorDiv,
        "mod" => Mod,
        "div" => Div,
        "and" => And,
        "or" => Or,
        "xor" => Xor,
        "not" => Not,
        "add" => Add,
        "sub" => Sub,
        "mul" => Mul,
        "eq" => Eq,
        "ne" => Ne,
        "lt" => Lt,
        "le" => Le,
        "gt" => Gt,
        "ge" => Ge,
        _ => return None,
    };
    Some((op, Some(scalar_ty(ty)?)))
}

fn parse_int(s: &str) -> R<i128> {
    let t = s.replace('_', "");
    let (neg, t) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.as_str()),
    };
    let (radix, digits) = match t.get(..2) {
        Some("0x") => (16, &t[2..]),
        Some("0o") => (8, &t[2..]),
        Some("0b") => (2, &t[2..]),
        _ => (10, t),
    };
    let v = i128::from_str_radix(digits, radix).map_err(|_| format!("integer literal `{s}`"))?;
    Ok(if neg { -v } else { v })
}

enum Scope {
    Loop {
        label: String,
        head: BlockId,
        exit: BlockId,
    },
    /// A block that declares join points. A jump from its statement `at` sets `cont` to `at`,
    /// and the end of a join point's body resumes after that statement.
    Jps {
        jps: Vec<(String, BlockId, Vec<Local>)>,
        cont: Local,
        at: Option<i128>,
    },
}

struct Fn<'a> {
    ids: &'a HashMap<String, FuncId>,
    sigs: &'a [Vec<Ty>],
    strings: &'a mut Vec<String>,
    locals: Vec<Ty>,
    names: HashMap<String, Local>,
    blocks: Vec<Block>,
    done: Vec<bool>,
    cur: BlockId,
    scopes: Vec<Scope>,
}

impl<'a> Fn<'a> {
    fn new(
        ids: &'a HashMap<String, FuncId>,
        sigs: &'a [Vec<Ty>],
        strings: &'a mut Vec<String>,
    ) -> Self {
        Fn {
            ids,
            sigs,
            strings,
            locals: Vec::new(),
            names: HashMap::new(),
            blocks: Vec::new(),
            done: Vec::new(),
            cur: 0,
            scopes: Vec::new(),
        }
    }

    fn func(mut self, f: &y::FnDef) -> R<Func> {
        let ps = params(f)?;
        for (name, ty) in &ps {
            let l = self.local(*ty);
            self.names.insert((*name).to_owned(), l);
        }
        let ret = ty_of(&f.ret)?;
        if f.fail.is_some() {
            return unsupported("a failing function");
        }
        self.cur = self.block_new();
        for c in &f.contracts {
            if let y::Contract::Requires(e) = c {
                let t = self.expr(e)?;
                self.check(t, "contract");
            }
        }
        self.block(f.body.as_ref().expect("filtered on body"))?;
        self.finish(Term::Panic("unreachable"));
        Ok(Func {
            name: f.path.join("."),
            params: ps.len() as u32,
            ret,
            locals: self.locals,
            blocks: self.blocks,
        })
    }

    fn local(&mut self, t: Ty) -> Local {
        self.locals.push(t);
        self.locals.len() as Local - 1
    }

    fn block_new(&mut self) -> BlockId {
        self.blocks.push(Block {
            insts: Vec::new(),
            term: Term::Panic("unreachable"),
        });
        self.done.push(false);
        self.blocks.len() as BlockId - 1
    }

    fn emit(&mut self, i: Inst) {
        self.blocks[self.cur as usize].insts.push(i);
    }

    /// End the current block with `t` unless it already ended.
    fn finish(&mut self, t: Term) {
        let c = self.cur as usize;
        if !self.done[c] {
            self.blocks[c].term = t;
            self.done[c] = true;
        }
    }

    /// End the current block with `t` and continue in a new, unreachable one.
    fn end(&mut self, t: Term) {
        self.finish(t);
        self.cur = self.block_new();
    }

    fn goto(&mut self, b: BlockId) {
        self.finish(Term::Jump(b));
        self.cur = b;
    }

    fn check(&mut self, cond: Operand, kind: &'static str) {
        let (ok, bad) = (self.block_new(), self.block_new());
        self.finish(Term::Branch {
            cond,
            then: ok,
            els: bad,
        });
        self.cur = bad;
        self.finish(Term::Panic(kind));
        self.cur = ok;
    }

    fn name(&self, n: &str) -> R<Local> {
        self.names
            .get(n)
            .copied()
            .ok_or_else(|| format!("unbound name `{n}`"))
    }

    fn atom(&mut self, a: &y::Atom) -> R<Operand> {
        Ok(match a {
            y::Atom::Name(n, _) => Operand::Local(self.name(n)?),
            y::Atom::Lit(l, _) => match l {
                y::Literal::Int(s) => Operand::Int(parse_int(s)?),
                y::Literal::Float(s) => Operand::F64(
                    s.replace('_', "")
                        .parse()
                        .map_err(|_| format!("float `{s}`"))?,
                ),
                y::Literal::Bool(b) => Operand::Bool(*b),
                y::Literal::Unit => Operand::Unit,
                y::Literal::Str(s) => {
                    self.strings.push(s.clone());
                    Operand::Str(self.strings.len() as u32 - 1)
                }
                y::Literal::Char(_) => return unsupported("a char"),
            },
        })
    }

    fn expr(&mut self, e: &y::Expr) -> R<Operand> {
        match e {
            y::Expr::Atom(a) => self.atom(a),
            y::Expr::Call(c) => {
                let t = self.local(Ty::Bool);
                self.call(c, Some(t))?;
                Ok(Operand::Local(t))
            }
        }
    }

    fn args(&mut self, c: &y::Call) -> R<Vec<Operand>> {
        c.args
            .iter()
            .filter_map(|a| match a {
                y::Arg::Value(y::Conv::Inout, ..) => Some(unsupported("an `inout` argument")),
                y::Arg::Value(_, a, _) => Some(self.atom(a)),
                y::Arg::Cap(..) => None,
            })
            .collect()
    }

    fn call(&mut self, c: &y::Call, dst: Option<Local>) -> R<()> {
        let name = c.path.join(".");
        let args = self.args(c)?;
        if let Some(&func) = self.ids.get(&name) {
            if self.sigs[func as usize].len() != args.len() {
                return unsupported(format!("call of `{name}` with these arguments"));
            }
            self.emit(Inst::Call { dst, func, args });
            return Ok(());
        }
        let Some(id) = registry::id(&name) else {
            return unsupported(format!("call of `{name}`"));
        };
        if let Some((op, ty)) = scalar(&name) {
            let ty = match ty {
                Some(t) => t,
                None => match args.first() {
                    Some(Operand::Local(l)) => self.locals[*l as usize],
                    _ => Ty::Int(IntTy {
                        bits: 64,
                        signed: true,
                    }),
                },
            };
            self.emit(Inst::Scalar {
                dst,
                id,
                op,
                ty,
                args,
            });
            return Ok(());
        }
        match (id, args.as_slice()) {
            (id::STD_IO_PRINT, [Operand::Str(_)]) => {
                self.emit(Inst::Std { dst, id, args });
                Ok(())
            }
            _ => unsupported(format!("call of `{name}`")),
        }
    }

    fn rhs(&mut self, r: &y::Rhs, dst: Option<Local>) -> R<()> {
        match r {
            y::Rhs::Atom(a) => {
                let src = self.atom(a)?;
                if let Some(dst) = dst {
                    self.emit(Inst::Copy { dst, src });
                }
                Ok(())
            }
            y::Rhs::Call(c) => self.call(c, dst),
            y::Rhs::Proj(p) | y::Rhs::Copy(p) | y::Rhs::Move(p) if p.projs.is_empty() => {
                let src = Operand::Local(self.name(&p.root)?);
                if let Some(dst) = dst {
                    self.emit(Inst::Copy { dst, src });
                }
                Ok(())
            }
            y::Rhs::Proj(_) | y::Rhs::Copy(_) | y::Rhs::Move(_) => unsupported("a projection"),
            y::Rhs::Propagate { .. } | y::Rhs::Try(_) => unsupported("a failure"),
            y::Rhs::Ctor(_) => unsupported("a constructor"),
            y::Rhs::Closure(_) => unsupported("a closure"),
        }
    }

    fn block(&mut self, b: &y::Block) -> R<()> {
        let decls: Vec<_> = b
            .stmts
            .iter()
            .filter_map(|s| match &s.kind {
                y::StmtKind::Jp { name, params, body } => Some((name, params, body)),
                _ => None,
            })
            .collect();
        if decls.is_empty() {
            return b.stmts.iter().try_for_each(|s| self.stmt(s));
        }
        let cont = self.local(Ty::Int(IntTy {
            bits: 64,
            signed: true,
        }));
        let mut jps = Vec::new();
        for (name, params, _) in &decls {
            let mut ls = Vec::new();
            for p in params.iter() {
                let y::Param::Value { name, ty, .. } = p else {
                    return unsupported("a capability join-point parameter");
                };
                let l = self.local(ty_of(ty)?);
                self.names.insert(name.clone(), l);
                ls.push(l);
            }
            jps.push(((*name).clone(), self.block_new(), ls));
        }
        self.scopes.push(Scope::Jps {
            jps: jps.clone(),
            cont,
            at: None,
        });
        let mut resume = Vec::new();
        for s in &b.stmts {
            if matches!(s.kind, y::StmtKind::Jp { .. }) {
                continue;
            }
            if let Some(Scope::Jps { at, .. }) = self.scopes.last_mut() {
                *at = Some(resume.len() as i128);
            }
            self.stmt(s)?;
            let next = self.block_new();
            self.goto(next);
            resume.push(next);
        }
        let after = self.cur;
        if let Some(Scope::Jps { at, .. }) = self.scopes.last_mut() {
            *at = None;
        }
        for ((_, _, body), (_, start, _)) in decls.iter().zip(&jps) {
            self.cur = *start;
            self.block(body)?;
            self.finish(Term::Switch {
                on: cont,
                targets: resume.clone(),
            });
        }
        self.scopes.pop();
        self.cur = after;
        Ok(())
    }

    fn jump(&mut self, name: &str, args: &[y::Atom]) -> R<()> {
        let found = self.scopes.iter().rev().find_map(|s| match s {
            Scope::Jps { jps, cont, at } => jps
                .iter()
                .find(|(n, ..)| n == name)
                .map(|(_, b, ps)| (*b, ps.clone(), *cont, *at)),
            Scope::Loop { .. } => None,
        });
        let Some((target, params, cont, at)) = found else {
            return unsupported(format!("jump to `{name}`"));
        };
        let vals = args.iter().map(|a| self.atom(a)).collect::<R<Vec<_>>>()?;
        // Through temporaries, so `jump k(b, a)` reads before it writes.
        let temps: Vec<Local> = vals
            .into_iter()
            .zip(&params)
            .map(|(src, p)| {
                let t = self.local(self.locals[*p as usize]);
                self.emit(Inst::Copy { dst: t, src });
                t
            })
            .collect();
        for (p, t) in params.iter().zip(temps) {
            self.emit(Inst::Copy {
                dst: *p,
                src: Operand::Local(t),
            });
        }
        if let Some(at) = at {
            self.emit(Inst::Copy {
                dst: cont,
                src: Operand::Int(at),
            });
        }
        self.end(Term::Jump(target));
        Ok(())
    }

    fn leave(&mut self, label: &str, brk: bool) -> R<()> {
        let to = self.scopes.iter().rev().find_map(|s| match s {
            Scope::Loop {
                label: l,
                head,
                exit,
            } if l == label => Some(if brk { *exit } else { *head }),
            _ => None,
        });
        match to {
            Some(b) => {
                self.end(Term::Jump(b));
                Ok(())
            }
            None => unsupported(format!("loop label `{label}`")),
        }
    }

    fn stmt(&mut self, s: &y::Stmt) -> R<()> {
        use y::StmtKind as K;
        match &s.kind {
            K::Let { pat, ty, rhs, .. } => {
                let dst = match pat {
                    y::Pat::Bind(n) => {
                        let l = self.local(ty_of(ty)?);
                        self.names.insert(n.clone(), l);
                        Some(l)
                    }
                    y::Pat::Wild => None,
                    _ => return unsupported("a destructuring `let`"),
                };
                self.rhs(rhs, dst)
            }
            K::Assign { place, rhs, .. } if place.projs.is_empty() => {
                let dst = self.name(&place.root)?;
                self.rhs(rhs, Some(dst))
            }
            K::Assign { .. } => unsupported("assignment through a projection"),
            K::If { cond, then, els } => {
                let cond = self.atom(cond)?;
                let (t, e, join) = (self.block_new(), self.block_new(), self.block_new());
                self.finish(Term::Branch {
                    cond,
                    then: t,
                    els: e,
                });
                self.cur = t;
                self.block(then)?;
                self.goto(join);
                self.cur = e;
                if let Some(els) = els {
                    self.block(els)?;
                }
                self.goto(join);
                Ok(())
            }
            K::Loop { label, body, .. } => {
                let (head, exit) = (self.block_new(), self.block_new());
                self.goto(head);
                self.scopes.push(Scope::Loop {
                    label: label.clone(),
                    head,
                    exit,
                });
                self.block(body)?;
                self.scopes.pop();
                self.goto(head);
                self.cur = exit;
                Ok(())
            }
            K::Break { value: Some(_), .. } => unsupported("`break` with a value"),
            K::Break { label, value: None } => self.leave(label, true),
            K::Continue { label } => self.leave(label, false),
            K::Jp { .. } => unreachable!("handled by `block`"),
            K::Jump { name, args } => self.jump(name, args),
            K::Ret(a) => {
                let v = self.atom(a)?;
                self.end(Term::Return(v));
                Ok(())
            }
            K::Become(c) => {
                let name = c.path.join(".");
                let Some(&func) = self.ids.get(&name) else {
                    return unsupported(format!("`become` of `{name}`"));
                };
                let args = self.args(c)?;
                self.end(Term::TailCall { func, args });
                Ok(())
            }
            K::Par { blocks, .. } => blocks.iter().try_for_each(|b| self.block(b)),
            K::Assert(e) => {
                let t = self.expr(e)?;
                self.check(t, "assert");
                Ok(())
            }
            K::Unreachable => {
                self.end(Term::Panic("unreachable"));
                Ok(())
            }
            K::Unsafe(b) => self.block(b),
            K::Drop(_) => Ok(()),
            K::Rhs(r) => self.rhs(r, None),
            K::Match { .. } => unsupported("`match`"),
            K::Fail(_) => unsupported("`fail`"),
            K::Yield(_) => unsupported("generators"),
            K::With { .. } => unsupported("`with`"),
            K::ComptimeFor { .. } | K::ComptimeIf { .. } => unsupported("a comptime statement"),
            K::Asm { .. } => unsupported("`asm`"),
            K::Reuse { .. } => unsupported("`reuse`"),
        }
    }
}

#[cfg(test)]
mod selftest {
    use super::*;

    #[test]
    fn lowers_join_points_and_reports_the_rest() {
        let src = "module t\nfn main() -> Unit {\n var n: i64 = 0\n\
                   jp k(let m: i64) { n = add_checked_i64(let n, let m) }\n\
                   loop 'l {\n n = add_checked_i64(let n, let 1)\n let big: bool = gt_i64(let n, let 2)\n\
                   if big { jump k(10) }\n }\n assert eq_i64(let n, let 13)\n ret ()\n}\n";
        let (m, d) = yong_core::parse("t.yong", src);
        assert!(d.is_empty());
        let p = lower(&m.unwrap()).expect("lowers");
        let f = &p.funcs[p.main as usize];
        assert!(
            f.blocks
                .iter()
                .any(|b| matches!(b.term, Term::Switch { .. }))
        );
        let (m, _) = yong_core::parse(
            "t.yong",
            "module t\nrecord P { x: i64 }\nfn main() -> Unit {\n let p: P = P { x: 1 }\n ret ()\n}\n",
        );
        assert!(lower(&m.unwrap()).unwrap_err().contains("type `P`"));
    }
}
