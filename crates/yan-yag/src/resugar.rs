//! HIR to the Yan Agent subset (16.4): Yan's `let` and `var` become assignments, whose mutability
//! Yan Agent infers (R421); a form Yan Agent cannot spell is reported as the first construct outside the
//! subset. A name elaboration generated, which Yan Agent cannot spell (E110), gets a free one.

use std::collections::{HashMap, HashSet};

use crate::ast::*;

/// `m` as Yan Agent the printer can print, or the first construct outside the Yan Agent subset.
pub fn resugar(mut m: Module) -> Result<Module, &'static str> {
    let mut taken = HashSet::new();
    let mut made = Vec::new();
    let mut collect = |x: &mut Name| {
        if generated(x) {
            if !made.contains(x) {
                made.push(x.clone());
            }
        } else {
            taken.insert(x.clone());
        }
    };
    let mut w = Walk(&mut collect);
    m.items.iter_mut().try_for_each(|it| w.item(it))?;
    let mut fresh = HashMap::new();
    for g in made {
        let base = &g[1..];
        let mut x = base.to_owned();
        let mut k = 0;
        while taken.contains(&x) {
            k += 1;
            x = format!("{base}_{k}");
        }
        taken.insert(x.clone());
        fresh.insert(g, x);
    }
    if !fresh.is_empty() {
        let mut rename = |x: &mut Name| {
            if let Some(y) = fresh.get(x) {
                x.clone_from(y);
            }
        };
        let mut w = Walk(&mut rename);
        m.items.iter_mut().try_for_each(|it| w.item(it))?;
    }
    Ok(m)
}

/// `_`, letters, digits: a name elaboration generates.
fn generated(n: &str) -> bool {
    let Some(rest) = n.strip_prefix('_') else {
        return false;
    };
    let digits = rest.trim_start_matches(|c: char| c.is_ascii_lowercase());
    digits.len() < rest.len() && !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

type R = Result<(), &'static str>;

/// The walk, calling `.0` on every local name.
struct Walk<'a>(&'a mut dyn FnMut(&mut Name));

impl Walk<'_> {
    fn item(&mut self, it: &mut Item) -> R {
        match &mut it.kind {
            ItemKind::Func(f) | ItemKind::Lemma(f) => self.func(f),
            ItemKind::Record(r) => r.fields.iter_mut().try_for_each(|f| self.field(f)),
            ItemKind::Type(t) => {
                if let TypeRhs::Sum(vs) = &mut t.rhs {
                    for v in vs {
                        v.fields.iter_mut().try_for_each(|f| self.field(f))?;
                    }
                }
                self.opt(t.where_.as_mut())
            }
            ItemKind::Trait(t) => t.sigs.iter_mut().try_for_each(|(_, f)| self.func(f)),
            ItemKind::Impl(i) => i.fns.iter_mut().try_for_each(|f| self.func(f)),
            ItemKind::Const { value, .. } => self.expr(value),
            ItemKind::Extern(_) => Err("an extern block"),
        }
    }

    fn opt(&mut self, e: Option<&mut Expr>) -> R {
        e.map_or(Ok(()), |e| self.expr(e))
    }

    fn field(&mut self, f: &mut Field) -> R {
        self.opt(f.default.as_mut())
    }

    fn names(&mut self, ns: &mut [Name]) {
        ns.iter_mut().for_each(|x| (self.0)(x));
    }

    fn func(&mut self, f: &mut FuncDef) -> R {
        for p in &mut f.params {
            if let Param::Value {
                default: Some(e), ..
            } = p
            {
                self.expr(e)?;
            }
        }
        let Some(b) = &mut f.body else {
            return Ok(());
        };
        for c in &mut b.contracts {
            match c {
                Contract::Pre(e) | Contract::Post(e) | Contract::Decreases(e) => self.expr(e)?,
                Contract::Ex { expr: e, with } => {
                    self.expr(e)?;
                    with.iter_mut().try_for_each(|(_, e)| self.expr(e))?;
                }
            }
        }
        self.stmts(&mut b.stmts)
    }

    fn stmts(&mut self, ss: &mut [Stmt]) -> R {
        ss.iter_mut().try_for_each(|s| self.stmt(s))
    }

    fn block(&mut self, b: &mut Block) -> R {
        self.stmts(&mut b.stmts)
    }

    fn loop_contracts(&mut self, cs: &mut [LoopContract]) -> R {
        cs.iter_mut().try_for_each(|c| match c {
            LoopContract::Invariant(e) | LoopContract::Decreases(e) => self.expr(e),
        })
    }

    fn stmt(&mut self, s: &mut Stmt) -> R {
        if let StmtKind::Bind {
            target,
            target_span,
            ty,
            value,
            ..
        } = &mut s.kind
        {
            let target = Expr {
                kind: ExprKind::Name(std::mem::take(target)),
                span: *target_span,
                paren: false,
            };
            let (ty, value) = (ty.take(), value.clone());
            s.kind = StmtKind::Assign { target, ty, value };
        }
        match &mut s.kind {
            StmtKind::Expr(e)
            | StmtKind::Become(e)
            | StmtKind::Fail(e)
            | StmtKind::Assert(e)
            | StmtKind::Return(Some(e)) => self.expr(e),
            StmtKind::Assign { target, value, .. } | StmtKind::AugAssign { target, value, .. } => {
                self.expr(target)?;
                self.expr(value)
            }
            StmtKind::Ghost { value, .. } => self.expr(value),
            StmtKind::Bind { .. } => unreachable!("rewritten above"),
            StmtKind::Unsafe(_) => Err("`unsafe`"),
            StmtKind::Asm { .. } => Err("`asm`"),
            StmtKind::StageIf { .. } | StmtKind::StageFor { .. } => Err("stage-0 control"),
            StmtKind::BreakTo(_) | StmtKind::ContinueTo(_) => {
                Err("a labelled `break` or `continue`")
            }
            StmtKind::Return(None)
            | StmtKind::Break
            | StmtKind::Continue
            | StmtKind::Pass
            | StmtKind::Unreachable
            | StmtKind::Reveal(_) => Ok(()),
            StmtKind::If { arms, els } => {
                for (c, b) in arms {
                    self.expr(c)?;
                    self.block(b)?;
                }
                els.as_mut().map_or(Ok(()), |b| self.block(b))
            }
            StmtKind::While { label: Some(_), .. } | StmtKind::For { label: Some(_), .. } => {
                Err("a loop label")
            }
            StmtKind::While {
                cond,
                contracts,
                body,
                ..
            } => {
                self.expr(cond)?;
                self.loop_contracts(contracts)?;
                self.stmts(body)
            }
            StmtKind::For {
                target,
                iter,
                contracts,
                body,
                ..
            } => {
                self.names(target);
                self.expr(iter)?;
                self.loop_contracts(contracts)?;
                self.stmts(body)
            }
            StmtKind::Match { scrutinee, cases } => {
                self.expr(scrutinee)?;
                for c in cases {
                    self.pat(&mut c.pat)?;
                    self.opt(c.guard.as_mut())?;
                    self.block(&mut c.body)?;
                }
                Ok(())
            }
            StmtKind::With { kind, body } => {
                match kind {
                    WithKind::Bind(x, e) => {
                        (self.0)(x);
                        self.expr(e)?
                    }
                    WithKind::Deadline(e) | WithKind::Timeout(e) => self.expr(e)?,
                }
                self.block(body)
            }
            StmtKind::Par { limit, body, .. } => {
                self.opt(limit.as_mut())?;
                match body {
                    ParBody::Block(ss) => self.stmts(ss),
                    ParBody::For {
                        target,
                        iter,
                        contracts,
                        body,
                    } => {
                        self.names(target);
                        self.expr(iter)?;
                        self.loop_contracts(contracts)?;
                        self.stmts(body)
                    }
                }
            }
        }
    }

    fn pat(&mut self, p: &mut Pat) -> R {
        match p {
            Pat::Wild => Ok(()),
            Pat::Path { path, args: None } => {
                if let [x] = path.as_mut_slice() {
                    (self.0)(x);
                }
                Ok(())
            }
            Pat::Lit(e) => self.expr(e),
            Pat::Path { args: Some(a), .. } => a.iter_mut().try_for_each(|(_, p)| self.pat(p)),
            Pat::Tuple(ps) => ps.iter_mut().try_for_each(|p| self.pat(p)),
            Pat::As(p, x) => {
                (self.0)(x);
                self.pat(p)
            }
        }
    }

    fn arg(&mut self, a: &mut Arg) -> R {
        match &mut a.kind {
            ArgKind::Expr(e) | ArgKind::Ref(e) | ArgKind::Become(e) => self.expr(e),
        }
    }

    fn expr(&mut self, e: &mut Expr) -> R {
        match &mut e.kind {
            ExprKind::Name(x) => {
                (self.0)(x);
                Ok(())
            }
            ExprKind::Lit(_) | ExprKind::Hole | ExprKind::Wild => Ok(()),
            ExprKind::Stage(_) => Err("a stage-0 expression"),
            ExprKind::FStr { pieces, .. } => pieces.iter_mut().try_for_each(|p| match p {
                FPiece::Lit(_) => Ok(()),
                FPiece::Expr(e, _) => self.expr(e),
            }),
            ExprKind::Tuple(xs) | ExprKind::List(xs) | ExprKind::Set(xs) => {
                xs.iter_mut().try_for_each(|x| self.expr(x))
            }
            ExprKind::Dict(kvs) => kvs.iter_mut().try_for_each(|(k, v)| {
                self.expr(k)?;
                self.expr(v)
            }),
            ExprKind::Comp {
                elem,
                value,
                target,
                iter,
                cond,
                ..
            } => {
                self.names(target);
                self.expr(elem)?;
                self.opt(value.as_deref_mut())?;
                self.expr(iter)?;
                self.opt(cond.as_deref_mut())
            }
            ExprKind::Lambda(ps, x) => {
                self.names(ps);
                self.expr(x)
            }
            ExprKind::Attr(x, ..) | ExprKind::Try(x) | ExprKind::Unary(_, x) | ExprKind::Not(x) => {
                self.expr(x)
            }
            ExprKind::Call(f, args) => {
                self.expr(f)?;
                args.iter_mut().try_for_each(|a| self.arg(a))
            }
            ExprKind::Index(a, b) | ExprKind::Binary(_, a, b) | ExprKind::Compare(_, a, b) => {
                self.expr(a)?;
                self.expr(b)
            }
            ExprKind::Slice(x, parts, _) => {
                self.expr(x)?;
                parts.iter_mut().flatten().try_for_each(|p| self.expr(p))
            }
            ExprKind::Cond { then, cond, els } => {
                self.expr(then)?;
                self.expr(cond)?;
                self.expr(els)
            }
            ExprKind::Fallback(x, f) => {
                self.expr(x)?;
                match &mut **f {
                    Fallback::Value(v) | Fallback::Fail(v) => self.expr(v),
                    Fallback::Bind(x, v) => {
                        (self.0)(x);
                        self.expr(v)
                    }
                    Fallback::Return(v) => self.opt(v.as_mut()),
                    Fallback::Break | Fallback::Continue => Ok(()),
                    Fallback::Block(..) => Err("a fallback block"),
                }
            }
            ExprKind::Reify(_) => Err("`try` as a value"),
            ExprKind::Cast(..) => Err("`as`"),
        }
    }
}
