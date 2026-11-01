//! HIR to the Jian subset (16.4): Wen's `let` and `var` become assignments, whose mutability
//! Jian infers (R421); a form Jian cannot spell is reported as the first construct outside the
//! subset.

use crate::ast::*;

/// `m` as Jian the printer can print, or the first construct outside the Jian subset.
pub fn resugar(mut m: Module) -> Result<Module, &'static str> {
    for it in &mut m.items {
        item(it)?;
    }
    Ok(m)
}

type R = Result<(), &'static str>;

fn item(it: &mut Item) -> R {
    match &mut it.kind {
        ItemKind::Func(f) | ItemKind::Lemma(f) => func(f),
        ItemKind::Class(c) => c.fields.iter_mut().try_for_each(field),
        ItemKind::Type(t) => {
            if let TypeRhs::Sum(vs) = &mut t.rhs {
                for v in vs {
                    v.fields.iter_mut().try_for_each(field)?;
                }
            }
            t.where_.as_mut().map_or(Ok(()), expr)
        }
        ItemKind::Trait(t) => t.sigs.iter_mut().try_for_each(|(_, f)| func(f)),
        ItemKind::Impl(i) => i.fns.iter_mut().try_for_each(func),
        ItemKind::Const { value, .. } => expr(value),
    }
}

fn field(f: &mut Field) -> R {
    f.default.as_mut().map_or(Ok(()), expr)
}

fn func(f: &mut FuncDef) -> R {
    for p in &mut f.params {
        if let Param::Value {
            default: Some(e), ..
        } = p
        {
            expr(e)?;
        }
    }
    let Some(b) = &mut f.body else {
        return Ok(());
    };
    for c in &mut b.contracts {
        match c {
            Contract::Pre(e) | Contract::Post(e) | Contract::Decreases(e) => expr(e)?,
            Contract::Ex { expr: e, with } => {
                expr(e)?;
                with.iter_mut().try_for_each(|(_, e)| expr(e))?;
            }
        }
    }
    stmts(&mut b.stmts)
}

fn stmts(ss: &mut [Stmt]) -> R {
    ss.iter_mut().try_for_each(stmt)
}

fn block(b: &mut Block) -> R {
    stmts(&mut b.stmts)
}

fn loop_contracts(cs: &mut [LoopContract]) -> R {
    cs.iter_mut().try_for_each(|c| match c {
        LoopContract::Invariant(e) | LoopContract::Decreases(e) => expr(e),
    })
}

fn stmt(s: &mut Stmt) -> R {
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
        | StmtKind::Return(Some(e)) => expr(e),
        StmtKind::Assign { target, value, .. } | StmtKind::AugAssign { target, value, .. } => {
            expr(target)?;
            expr(value)
        }
        StmtKind::Ghost { value, .. } => expr(value),
        StmtKind::Bind { .. } => unreachable!("rewritten above"),
        StmtKind::LetElse { .. } => Err("`let ... else`"),
        StmtKind::BreakTo(_) | StmtKind::ContinueTo(_) => Err("a labelled `break` or `continue`"),
        StmtKind::Return(None)
        | StmtKind::Break
        | StmtKind::Continue
        | StmtKind::Pass
        | StmtKind::Unreachable
        | StmtKind::Reveal(_) => Ok(()),
        StmtKind::If { arms, els } => {
            for (c, b) in arms {
                expr(c)?;
                block(b)?;
            }
            els.as_mut().map_or(Ok(()), block)
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
            expr(cond)?;
            loop_contracts(contracts)?;
            stmts(body)
        }
        StmtKind::For {
            iter,
            contracts,
            body,
            ..
        } => {
            expr(iter)?;
            loop_contracts(contracts)?;
            stmts(body)
        }
        StmtKind::Match { scrutinee, cases } => {
            expr(scrutinee)?;
            for c in cases {
                pat(&mut c.pat)?;
                c.guard.as_mut().map_or(Ok(()), expr)?;
                block(&mut c.body)?;
            }
            Ok(())
        }
        StmtKind::With { kind, body } => {
            match kind {
                WithKind::Bind(_, e) | WithKind::Deadline(e) | WithKind::Timeout(e) => expr(e)?,
            }
            block(body)
        }
        StmtKind::Par { limit, body, .. } => {
            limit.as_mut().map_or(Ok(()), expr)?;
            match body {
                ParBody::Block(ss) => stmts(ss),
                ParBody::For {
                    iter,
                    contracts,
                    body,
                    ..
                } => {
                    expr(iter)?;
                    loop_contracts(contracts)?;
                    stmts(body)
                }
            }
        }
    }
}

fn pat(p: &mut Pat) -> R {
    match p {
        Pat::Wild | Pat::Path { args: None, .. } => Ok(()),
        Pat::Lit(e) => expr(e),
        Pat::Path { args: Some(a), .. } => a.iter_mut().try_for_each(|(_, p)| pat(p)),
        Pat::Tuple(ps) => ps.iter_mut().try_for_each(pat),
        Pat::As(p, _) => pat(p),
        Pat::Or(_) => Err("an or-pattern"),
        Pat::Range { .. } => Err("a range pattern"),
        Pat::Slice(..) => Err("a slice pattern"),
    }
}

fn arg(a: &mut Arg) -> R {
    match &mut a.kind {
        ArgKind::Expr(e) | ArgKind::Ref(e) | ArgKind::Become(e) => expr(e),
    }
}

fn expr(e: &mut Expr) -> R {
    match &mut e.kind {
        ExprKind::Name(_) | ExprKind::Lit(_) | ExprKind::Hole | ExprKind::Wild => Ok(()),
        ExprKind::FStr { pieces, .. } => pieces.iter_mut().try_for_each(|p| match p {
            FPiece::Lit(_) => Ok(()),
            FPiece::Expr(e, _) => expr(e),
        }),
        ExprKind::Tuple(xs) | ExprKind::List(xs) | ExprKind::Set(xs) => {
            xs.iter_mut().try_for_each(expr)
        }
        ExprKind::Dict(kvs) => kvs.iter_mut().try_for_each(|(k, v)| {
            expr(k)?;
            expr(v)
        }),
        ExprKind::Comp {
            elem,
            value,
            iter,
            cond,
            ..
        } => {
            expr(elem)?;
            value.as_deref_mut().map_or(Ok(()), expr)?;
            expr(iter)?;
            cond.as_deref_mut().map_or(Ok(()), expr)
        }
        ExprKind::Attr(x, ..)
        | ExprKind::Try(x)
        | ExprKind::Unary(_, x)
        | ExprKind::Not(x)
        | ExprKind::Lambda(_, x) => expr(x),
        ExprKind::Call(f, args) => {
            expr(f)?;
            args.iter_mut().try_for_each(arg)
        }
        ExprKind::Index(a, b) | ExprKind::Binary(_, a, b) | ExprKind::Compare(_, a, b) => {
            expr(a)?;
            expr(b)
        }
        ExprKind::Slice(x, parts, _) => {
            expr(x)?;
            parts.iter_mut().flatten().try_for_each(|p| expr(p))
        }
        ExprKind::Cond { then, cond, els } => {
            expr(then)?;
            expr(cond)?;
            expr(els)
        }
        ExprKind::Fallback(x, f) => {
            expr(x)?;
            match &mut **f {
                Fallback::Value(v) | Fallback::Bind(_, v) | Fallback::Fail(v) => expr(v),
                Fallback::Return(v) => v.as_mut().map_or(Ok(()), expr),
                Fallback::Break | Fallback::Continue => Ok(()),
                Fallback::Block(..) => Err("a fallback block"),
            }
        }
        ExprKind::Is { .. } => Err("`is`"),
        ExprKind::Reify(_) => Err("`try` as a value"),
        ExprKind::Cast(..) => Err("`as`"),
    }
}
