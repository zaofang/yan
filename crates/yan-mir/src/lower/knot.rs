//! Recursive types (5.1): a walk over a value that meets its own type again behind a block calls
//! an out-of-line function for that type, made once per walk and type, instead of inlining it.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Walk {
    /// `show`, nested in a container or not.
    Show(bool),
    Copy,
    Drop,
    /// `==` of two values.
    Eq,
    /// The order of two values: -1, 0 or 1.
    Cmp,
}

impl Walk {
    fn ret(self, s: &Shape) -> Shape {
        match self {
            Walk::Show(_) => Shape::Str,
            Walk::Copy => s.clone(),
            Walk::Drop => Shape::Scalar(Ty::Unit),
            Walk::Eq => Shape::Scalar(Ty::Bool),
            Walk::Cmp => Shape::Scalar(I64),
        }
    }

    /// How many values of the type it takes.
    fn arity(self) -> usize {
        match self {
            Walk::Eq | Walk::Cmp => 2,
            _ => 1,
        }
    }
}

impl<'m> Fn<'_, 'm> {
    /// The function walking `w` over the type `k` names, made once.
    fn walk_id(&mut self, w: Walk, k: &Knot) -> FuncId {
        let mut m = self.mono.borrow_mut();
        if let Some(id) = m.walks.get(&(w, k.0.clone())) {
            return *id;
        }
        let s = k.get();
        let id = m.sigs.len() as FuncId;
        m.sigs.push(Sig {
            params: vec![(y::Conv::Let, s.clone()); w.arity()],
            ok: w.ret(s),
            ret: w.ret(s),
            fails: false,
        });
        m.todo.push(Todo::Walk(w, s.clone()));
        m.walks.insert((w, k.0.clone()), id);
        id
    }

    /// The result slots of walking `w` over `v`, of the type `k` names.
    pub(super) fn walk_call(&mut self, w: Walk, k: &Knot, v: &[Operand]) -> Vec<Operand> {
        let func = self.walk_id(w, k);
        let dst = self.var(w.ret(k.get()));
        self.emit(Inst::Call {
            dst: dst.locals.clone(),
            func,
            args: v.to_vec(),
        });
        Self::ops(&dst)
    }

    /// The slots of the value of the type `k` names in the box at `p`, which keeps them.
    pub(super) fn unbox(&mut self, k: &Knot, p: &Operand) -> Vec<Operand> {
        self.read(&At::Mem(p.clone(), 0), k.get())
    }

    /// A box holding the value `v`, which it takes.
    pub(super) fn boxed(&mut self, v: Vec<Operand>) -> Operand {
        let p = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(v.len() as i128)]);
        self.write(&At::Mem(p.clone(), 0), v);
        p
    }

    /// The body of walk `w` over a value of shape `s`, its only parameter.
    pub(super) fn walk(mut self, w: Walk, s: &Shape) -> R<Func> {
        let v: Vec<Operand> = (0..w.arity())
            .flat_map(|_| Self::ops(&self.var(s.clone())))
            .collect();
        let params = v.len() as u32;
        self.cur = self.block_new();
        let (a, b) = v.split_at(s.width());
        let out = match w {
            Walk::Eq => vec![self.eq_value(s, a, b)?],
            Walk::Cmp => vec![self.cmp_value(s, a, b)?],
            Walk::Show(nested) => vec![self.show(&v, s, nested)?],
            Walk::Copy => self.clone_value(v, s)?,
            Walk::Drop => {
                self.glue(s, &v);
                vec![Operand::Unit]
            }
        };
        self.finish(Term::Return(out));
        Ok(Func {
            name: match w {
                Walk::Show(_) => "knot.show",
                Walk::Copy => "knot.copy",
                Walk::Drop => "knot.drop",
                Walk::Eq => "knot.eq",
                Walk::Cmp => "knot.cmp",
            }
            .into(),
            params,
            ret: w.ret(s).tys(),
            locals: self.locals,
            pinned: self.pinned,
            blocks: self.blocks,
        })
    }
}
