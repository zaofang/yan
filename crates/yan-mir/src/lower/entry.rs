//! The program's entry when `main` can fail: call it, and on failure show the error on stderr and
//! exit with status 1, as the interpreter does.

use super::*;

impl Mono<'_> {
    pub(super) fn entry(&mut self, main: FuncId) -> FuncId {
        let id = self.sigs.len() as FuncId;
        let unit = Shape::Scalar(Ty::Unit);
        self.sigs.push(Sig {
            params: Vec::new(),
            ok: unit.clone(),
            ret: unit,
            fails: false,
        });
        self.todo.push(Todo::Entry(main));
        id
    }
}

impl Fn<'_, '_> {
    pub(super) fn entry(mut self, main: FuncId) -> R<Func> {
        self.cur = self.block_new();
        let r = self.var(self.sig(main).ret);
        let v = Self::ops(&r);
        self.emit(Inst::Call {
            dst: r.locals.clone(),
            func: main,
            args: Vec::new(),
        });
        let (_, err_at, efs) = r.shape.variant("Err").expect("`main` fails");
        let e = efs[0].1.clone();
        let is_ok = self.scalar_op(ScalarOp::Eq, I64, vec![v[0].clone(), Operand::Int(0)]);
        let (ok, err) = self.branch(is_ok);
        self.cur = err;
        let ev = v[err_at..err_at + e.width()].to_vec();
        let shown = self.show(&ev, &e, false)?;
        let code = self.exit_code(&e, ev)?;
        self.emit(Inst::Rt {
            dst: None,
            f: RtFn::FailMain,
            args: vec![shown, code],
        });
        self.finish(Term::Panic("unreachable"));
        self.cur = ok;
        self.ret(vec![Operand::Unit]);
        self.finish(Term::Panic("unreachable"));
        Ok(Func {
            name: "yan.main".to_owned(),
            params: 0,
            ret: vec![Ty::Unit],
            locals: self.locals,
            blocks: self.blocks,
        })
    }
}

impl Fn<'_, '_> {
    /// The status a failing `main` exits with: the `code` of the error's `Exit` impl, else 1 (R430).
    fn exit_code(&mut self, e: &Shape, ev: Vec<Operand>) -> R<Operand> {
        let Shape::Rec(name, _) = e else {
            return Ok(Operand::Int(1));
        };
        let Some(def) = self
            .mono
            .borrow()
            .by_name
            .get(&format!("{name}.code"))
            .copied()
        else {
            return Ok(Operand::Int(1));
        };
        let func = self.mono.borrow_mut().instance(self.types, def)?;
        let r = self.var(self.sig(func).ret.clone());
        self.emit(Inst::Call {
            dst: r.locals.clone(),
            func,
            args: ev,
        });
        Ok(Operand::Local(r.locals[0]))
    }
}
