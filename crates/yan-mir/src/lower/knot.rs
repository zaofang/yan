//! Recursive types (5.1): a walk over a value that meets its own type again behind a block calls
//! an out-of-line function for that type, made once per walk and type, instead of inlining it.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Walk {
    /// `show`, nested in a container or not.
    Show(bool),
    Copy,
    Drop,
}

impl Walk {
    fn ret(self, s: &Shape) -> Shape {
        match self {
            Walk::Show(_) => Shape::Str,
            Walk::Copy => s.clone(),
            Walk::Drop => Shape::Scalar(Ty::Unit),
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
            params: vec![(y::Conv::Let, s.clone())],
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

    /// The body of walk `w` over a value of shape `s`, its only parameter.
    pub(super) fn walk(mut self, w: Walk, s: &Shape) -> R<Func> {
        let v = Self::ops(&self.var(s.clone()));
        let params = v.len() as u32;
        self.cur = self.block_new();
        let out = match w {
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
