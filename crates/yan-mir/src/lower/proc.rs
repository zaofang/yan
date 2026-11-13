//! `std.proc` under a `fake_proc` handler (17.3, R691, R692): a `Command` is its program and
//! arguments, the handler is its dict of `Output`s, and `run` looks the program up. Running on
//! the host stays out of native code: the interpreter confines the child by the entry ceiling.

use super::*;

/// The shape of a type `std.proc` declares without a definition.
pub(super) fn std_type(name: &str) -> Option<Shape> {
    (name == "Command").then(|| {
        Shape::Rec(
            name.into(),
            vec![
                ("program".into(), Shape::Str),
                ("args".into(), Shape::List(Box::new(Shape::Str))),
            ],
        )
    })
}

impl<'a, 'm> Fn<'a, 'm> {
    /// The argv of the `Command` words `v`: a fresh `list[str]`.
    pub(super) fn argv(&mut self, v: &[Operand]) -> R<Operand> {
        let list = Shape::List(Box::new(Shape::Str));
        let l = self.clone_value(vec![v[1].clone()], &list)?.remove(0);
        let program = self.clone_value(vec![v[0].clone()], &Shape::Str)?;
        let at = self.rt(RtFn::ListInsert, Ty::Ptr, vec![l.clone(), Operand::Int(0)]);
        self.write(&At::Mem(at, 0), program);
        Ok(l)
    }

    pub(super) fn proc_call(&mut self, id: registry::Id, c: &y::Call, dst: Option<Var>) -> R<bool> {
        let args = Self::value_args(c)?;
        let set = |me: &mut Self, v: Vec<Operand>| {
            if let Some(d) = &dst {
                me.copy_to(&d.locals.clone(), v);
            }
        };
        let str_list = Shape::List(Box::new(Shape::Str));
        match (id, args.as_slice()) {
            (id::STD_PRELUDE_COMMAND, [(c1, p), (c2, xs)]) => {
                let mut v = self.owned(c1, p, &Shape::Str)?;
                v.extend(self.owned(c2, xs, &str_list)?);
                set(self, v);
            }
            (id::STD_PROC_PROGRAM, [(_, cmd)]) => {
                let v = self.atom_any(cmd)?.0;
                let r = self.clone_value(vec![v[0].clone()], &Shape::Str)?;
                set(self, r);
            }
            (id::STD_PROC_ARGV, [(_, cmd)]) => {
                let v = self.atom_any(cmd)?.0;
                let l = self.argv(&v[..2])?;
                // `argv` starts from the program and the arguments, which the command keeps.
                set(self, vec![l]);
            }
            (id::STD_PROC_ARG, [(_, cmd), (conv, s)]) => {
                let v = self.atom_any(cmd)?.0;
                let program = self.clone_value(vec![v[0].clone()], &Shape::Str)?;
                let l = self.clone_value(vec![v[1].clone()], &str_list)?.remove(0);
                let x = self.owned(conv, s, &Shape::Str)?;
                let at = self.rt(RtFn::ListPush, Ty::Ptr, vec![l.clone()]);
                self.write(&At::Mem(at, 0), x);
                set(self, vec![program[0].clone(), l]);
            }
            (id::STD_PROC_SUCCESS, [(_, out)]) => {
                let (v, s) = self.atom_any(out)?;
                return self.success_of(&v, &s, dst);
            }
            (id::STD_TEST_FAKE_PROC, [(conv, d)]) => {
                let Some(dst) = &dst else { return Ok(true) };
                let r = self.owned(conv, d, &dst.shape)?;
                self.copy_to(&dst.locals.clone(), r);
            }
            (id::STD_PROC_RUN, [(_, cmd)]) => {
                let Some(d) = dst else {
                    return unsupported("`proc.run` without its result");
                };
                let handler = match c.args.iter().find_map(|a| match a {
                    y::Arg::Cap(n, _) => self.names.get(n),
                    y::Arg::Value(..) => None,
                }) {
                    Some(v) => Self::ops(v)[0].clone(),
                    None => Operand::Int(0),
                };
                let program = self.atom_any(cmd)?.0[0].clone();
                self.run_fake(handler, program, &d)?;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// `Output.success`: the code is `Some(0)`.
    fn success_of(&mut self, v: &[Operand], s: &Shape, dst: Option<Var>) -> R<bool> {
        let Some(fs) = s.fields() else {
            return unsupported("`success` of a non-`Output`");
        };
        let code = fs[0].2.clone();
        let Some((tag, at, _)) = code.variant("Some") else {
            return unsupported("`Output.code` that is not an option");
        };
        let r = self.local(Ty::Bool);
        self.emit(Inst::Copy {
            dst: r,
            src: Operand::Bool(false),
        });
        let is_some = self.scalar_op(ScalarOp::Eq, I64, vec![v[0].clone(), Operand::Int(tag)]);
        let join = self.block_new();
        let (some, no) = self.branch(is_some);
        self.cur = some;
        let zero = self.scalar_op(ScalarOp::Eq, I64, vec![v[at].clone(), Operand::Int(0)]);
        self.emit(Inst::Copy { dst: r, src: zero });
        self.goto(join);
        self.cur = no;
        self.goto(join);
        self.cur = join;
        let r = Operand::Local(r);
        if let Some(d) = &dst {
            self.copy_to(&d.locals.clone(), vec![r]);
        }
        Ok(true)
    }

    /// `proc.run` under handler `h` (the dict of outputs; 0 is the host).
    fn run_fake(&mut self, h: Operand, program: Operand, d: &Var) -> R<()> {
        let shape = d.shape.clone();
        let (Some((_, _, ok)), Some((_, _, err))) = (shape.variant("Ok"), shape.variant("Err"))
        else {
            return unsupported("`proc.run` that is not a `Result`");
        };
        let (out, e) = (ok[0].1.clone(), err[0].1.clone());
        let join = self.block_new();
        let host = self.scalar_op(ScalarOp::Eq, I64, vec![h.clone(), Operand::Int(0)]);
        let (ambient, fake) = self.branch(host);
        self.cur = ambient;
        self.emit(Inst::Rt {
            dst: None,
            f: RtFn::ProcAmbient,
            args: Vec::new(),
        });
        self.goto(join);
        self.cur = fake;
        let buf = self.key_buf(vec![program.clone()]);
        let p = self.rt(RtFn::MapFind, Ty::Ptr, vec![h, buf.clone()]);
        self.free(RtFn::EnvFree, buf);
        let missing = self.scalar_op(ScalarOp::Eq, I64, vec![p.clone(), Operand::Int(0)]);
        let (none, some) = self.branch(missing);
        self.cur = some;
        let v = self.read(&At::Mem(p, 0), &out);
        let v = self.clone_value(v, &out)?;
        self.copy_to(&d.locals, Self::variant_value(&shape, "Ok", v));
        self.goto(join);
        self.cur = none;
        let Some(fs) = e.fields() else {
            return unsupported("`proc.run` whose fail type is not `ProcError`");
        };
        let reason = fs[1].2.clone();
        let mut ev = self.clone_value(vec![program], &Shape::Str)?;
        ev.extend(Self::variant_value(&reason, "NotFound", Vec::new()));
        ev.push(self.lit("not in fake_proc"));
        self.copy_to(&d.locals, Self::variant_value(&shape, "Err", ev));
        self.goto(join);
        self.cur = join;
        Ok(())
    }
}
