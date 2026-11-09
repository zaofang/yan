//! `std.actor` in native code (RFC 0056 D8): spawn, handler calls, mailboxes, supervisor scopes
//! and restarts over `yan_rt::actor`, which hands back every closure it needs run.

use super::*;

const PTR: Shape = Shape::Scalar(Ty::Ptr);

/// The yan-rt status asking the caller to run the closure in the work slots and retry.
const WORK: i128 = 3;

/// A `Duration` is its nanoseconds; `show` and arithmetic on it are not native yet.
pub(super) fn duration() -> Shape {
    Shape::Rec(
        "Duration".into(),
        vec![("nanos".into(), Shape::Scalar(I64))],
    )
}

const DURATION_OF: &[(registry::Id, i128)] = &[
    (id::STD_TIME_DURATION_OF_NANOS, 1),
    (id::STD_TIME_DURATION_OF_MICROS, 1_000),
    (id::STD_TIME_DURATION_OF_MILLIS, 1_000_000),
    (id::STD_TIME_DURATION_OF_SECONDS, 1_000_000_000),
    (id::STD_TIME_DURATION_OF_MINUTES, 60_000_000_000),
    (id::STD_TIME_DURATION_OF_HOURS, 3_600_000_000_000),
];

impl<'a, 'm> Fn<'a, 'm> {
    /// Lower `c` if it is a `std.actor` or `Duration` constructor call; whether it was.
    pub(super) fn actor_call(
        &mut self,
        id: registry::Id,
        c: &y::Call,
        dst: Option<&Var>,
    ) -> R<bool> {
        let args = Self::value_args(c)?;
        if let Some((_, k)) = DURATION_OF.iter().find(|(i, _)| *i == id) {
            let [(_, n)] = args.as_slice() else {
                return unsupported(format!("call of `{}`", c.path.join(".")));
            };
            let n = self.scalar1(n)?;
            let ns = self.scalar_op(ScalarOp::MulChecked, I64, vec![n, Operand::Int(*k)]);
            if let Some(d) = dst {
                self.copy_to(&d.locals, vec![ns]);
            }
            return Ok(true);
        }
        let strategy = match id {
            id::STD_ACTOR_ONE_FOR_ONE => Some("OneForOne"),
            id::STD_ACTOR_ONE_FOR_ALL => Some("OneForAll"),
            id::STD_ACTOR_REST_FOR_ONE => Some("RestForOne"),
            _ => None,
        };
        if let Some(name) = strategy {
            let Some(d) = dst else {
                return Ok(true);
            };
            let Some(fs) = d.shape.fields() else {
                return unsupported("a `Supervisor` that is not a record");
            };
            let mut v = Vec::new();
            for (f, _, s) in fs {
                let field = match f.as_str() {
                    "strategy" => Self::variant_value(s, name, Vec::new()),
                    "intensity" => self.atom(args[0].1, s)?,
                    "period" => self.atom(args[1].1, s)?,
                    "shutdown" => self.atom(args[2].1, s)?,
                    _ => return unsupported(format!("`Supervisor` field `{f}`")),
                };
                v.extend(field);
            }
            self.copy_to(&d.locals, v);
            return Ok(true);
        }
        match (id, args.as_slice()) {
            (id::STD_ACTOR_SPAWN, [(_, state), (_, cap)]) => {
                let Some(s) = self.targ(c)? else {
                    return unsupported("`spawn` without its state type");
                };
                let v = self.atom(state, &s)?;
                let cap = self.scalar1(cap)?;
                let none = vec![
                    Operand::Int(2),
                    Operand::Int(0),
                    Operand::Int(0),
                    Operand::Int(0),
                ];
                let a = self.spawn(&s, v, cap, none);
                if let Some(d) = dst {
                    self.copy_to(&d.locals, vec![a]);
                }
            }
            (id::STD_ACTOR_START, [(_, start), (_, restart), (_, cap)]) => {
                let Some(s) = self.targ(c)? else {
                    return unsupported("`start` without its state type");
                };
                let f = self.atom(start, &Shape::Fn(Vec::new(), Box::new(s.clone())))?;
                let (restart, cap) = (self.scalar1(restart)?, self.scalar1(cap)?);
                let v = self.var(s.clone());
                self.emit(Inst::CallIndirect {
                    dst: v.locals.clone(),
                    code: f[0].clone(),
                    args: vec![f[1].clone()],
                    params: vec![Ty::Ptr],
                    ret: s.tys(),
                });
                let thunk = Operand::Func(self.restart_thunk(&s));
                let how = vec![restart, thunk, f[0].clone(), f[1].clone()];
                let a = self.spawn(&s, Self::ops(&v), cap, how);
                if let Some(d) = dst {
                    self.copy_to(&d.locals, vec![a]);
                }
            }
            (id::STD_ACTOR_TAKE, [(_, h)]) => {
                let Some(d) = dst else {
                    return unsupported("`take` without its result");
                };
                self.take(h, d)?;
            }
            (id::STD_ACTOR_PUT, [(_, h), (_, state)]) => {
                let h = self.scalar1(h)?;
                self.put(h, state)?;
            }
            (id::STD_ACTOR_SETTLE, [(_, h), (_, state), (_, outcome)]) => {
                let Some(d) = dst else {
                    return unsupported("`settle` without its result");
                };
                let h = self.scalar1(h)?;
                self.put(h.clone(), state)?;
                let (v, s) = self.atom_any(outcome)?;
                let failed = self.scalar_op(ScalarOp::Eq, I64, vec![v[0].clone(), Operand::Int(1)]);
                let (crash, on) = self.branch(failed);
                self.cur = crash;
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::ActorCrash,
                    args: vec![h],
                });
                self.restarts();
                self.goto(on);
                self.widen(&v, &s, d)?;
            }
            (id::STD_ACTOR_REPLY, [(_, outcome)]) => {
                let (v, s) = self.atom_any(outcome)?;
                match dst {
                    Some(d) if d.shape == s => self.copy_to(&d.locals, v),
                    Some(_) => return unsupported("`reply` across result types"),
                    None => {}
                }
            }
            (id::STD_ACTOR_SEND, [(_, h), (_, m)]) => {
                let h = self.scalar1(h)?;
                let unit = Shape::Fn(Vec::new(), Box::new(Shape::Scalar(Ty::Unit)));
                let m = self.atom(m, &unit)?;
                let mut args = vec![h];
                args.extend(m);
                let st = self.retried(RtFn::ActorSend, args);
                let (ok, bad) = self.branch_eq(st, 0);
                self.cur = bad;
                self.finish(Term::Panic("unreachable"));
                self.cur = ok;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn branch_eq(&mut self, v: Operand, k: i128) -> (BlockId, BlockId) {
        let c = self.scalar_op(ScalarOp::Eq, I64, vec![v, Operand::Int(k)]);
        self.branch(c)
    }

    /// A new actor with state `v` of shape `s`: `how` is the restart type, thunk and start
    /// closure.
    fn spawn(&mut self, s: &Shape, v: Vec<Operand>, cap: Operand, how: Vec<Operand>) -> Operand {
        let w = v.len();
        let buf = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(w.max(1) as i128)]);
        self.write(&At::Mem(buf.clone(), 0), v);
        let mut args = vec![buf.clone(), Operand::Int(s.width() as i128), cap];
        args.extend(how);
        let a = self.rt(RtFn::ActorSpawn, Ty::Ptr, args);
        self.free(RtFn::EnvFree, buf);
        a
    }

    fn put(&mut self, h: Operand, state: &y::Atom) -> R<()> {
        let (v, _) = self.atom_any(state)?;
        let buf = self.rt(
            RtFn::Alloc,
            Ty::Ptr,
            vec![Operand::Int(v.len().max(1) as i128)],
        );
        self.write(&At::Mem(buf.clone(), 0), v);
        self.emit(Inst::Rt {
            dst: None,
            f: RtFn::ActorPut,
            args: vec![h, buf.clone()],
        });
        self.free(RtFn::EnvFree, buf);
        Ok(())
    }

    /// A blocking actor operation: it suspends its task when it parks (status 1) and runs the
    /// closure yan-rt asks for (status [`WORK`]), retrying after either. Its final status.
    fn retried(&mut self, f: RtFn, args: Vec<Operand>) -> Operand {
        let op = self.block_new();
        self.goto(op);
        let st = self.blocking(f, args);
        let (work, on) = self.branch_eq(st.clone(), WORK);
        self.cur = work;
        self.run_message();
        self.goto(op);
        self.cur = on;
        st
    }

    /// Run the message in the work slots, which owns its captures block.
    fn run_message(&mut self) {
        let code = self.rt(RtFn::ActorArg, Ty::Ptr, vec![Operand::Int(0)]);
        let env = self.rt(RtFn::ActorArg, Ty::Ptr, vec![Operand::Int(1)]);
        let u = self.local(Ty::Unit);
        self.emit(Inst::CallIndirect {
            dst: vec![u],
            code,
            args: vec![env.clone()],
            params: vec![Ty::Ptr],
            ret: vec![Ty::Unit],
        });
        self.free(RtFn::EnvFree, env);
        self.emit(Inst::Rt {
            dst: None,
            f: RtFn::ActorRan,
            args: Vec::new(),
        });
    }

    /// Restart every child a crash queued, each from its start closure through its thunk.
    fn restarts(&mut self) {
        let head = self.block_new();
        self.goto(head);
        let k = self.rt(RtFn::ActorRestartNext, I64, Vec::new());
        let (done, more) = self.branch_eq(k, 0);
        self.cur = more;
        let a: Vec<Operand> = (0..4)
            .map(|i| self.rt(RtFn::ActorArg, Ty::Ptr, vec![Operand::Int(i)]))
            .collect();
        let u = self.local(Ty::Unit);
        self.emit(Inst::CallIndirect {
            dst: vec![u],
            code: a[0].clone(),
            args: a[1..].to_vec(),
            params: vec![Ty::Ptr; 3],
            ret: vec![Ty::Unit],
        });
        self.goto(head);
        self.cur = done;
    }

    /// `take`: `Ok` of the actor's state, or `Err` of `Stopped` or `Deadlock` (RFC 0056 D6).
    fn take(&mut self, h: &y::Atom, d: &Var) -> R<()> {
        let (Some((_, _, ok)), Some((_, _, err))) = (d.shape.variant("Ok"), d.shape.variant("Err"))
        else {
            return unsupported("`take` without a `Result`");
        };
        let (s, e) = (ok[0].1.clone(), err[0].1.clone());
        let stopped = Self::variant_value(&d.shape, "Err", Self::failure(&e, "Stopped")?);
        let deadlock = Self::variant_value(&d.shape, "Err", Self::failure(&e, "Deadlock")?);
        let h = self.scalar1(h)?;
        let out = self.rt(
            RtFn::Alloc,
            Ty::Ptr,
            vec![Operand::Int(s.width().max(1) as i128)],
        );
        let st = self.retried(RtFn::ActorTake, vec![h, out.clone()]);
        let join = self.block_new();
        let (taken, failed) = self.branch_eq(st.clone(), 0);
        self.cur = taken;
        let v = self.read(&At::Mem(out.clone(), 0), &s);
        let v = Self::variant_value(&d.shape, "Ok", v);
        self.copy_to(&d.locals, v);
        self.goto(join);
        self.cur = failed;
        let (stop, dead) = self.branch_eq(st, 4);
        self.cur = stop;
        self.copy_to(&d.locals, stopped);
        self.goto(join);
        self.cur = dead;
        self.copy_to(&d.locals, deadlock);
        self.goto(join);
        self.free(RtFn::EnvFree, out);
        Ok(())
    }

    /// The fieldless failure `name` as a value of the fail type `e`.
    fn failure(e: &Shape, name: &str) -> R<Vec<Operand>> {
        match e {
            Shape::Rec(n, fs) if n == name && fs.is_empty() => Ok(Vec::new()),
            Shape::Sum(_, _) if e.variant(name).is_some() => {
                Ok(Self::variant_value(e, name, Vec::new()))
            }
            _ => unsupported(format!("an actor call whose failure lacks `{name}`")),
        }
    }

    /// `Result[T, E]` `v` of shape `s` as the `Result[T, F]` `d` (`widen_err`).
    fn widen(&mut self, v: &[Operand], s: &Shape, d: &Var) -> R<()> {
        if *s == d.shape {
            self.copy_to(&d.locals, v.to_vec());
            return Ok(());
        }
        let (Some((_, ok_at, ok)), Some((_, err_at, err)), Some((_, _, to))) =
            (s.variant("Ok"), s.variant("Err"), d.shape.variant("Err"))
        else {
            return unsupported("a handler outcome that is not a `Result`");
        };
        let (e, f) = (err[0].1.clone(), to[0].1.clone());
        let w = ok[0].1.width();
        let join = self.block_new();
        let (is_ok, is_err) = self.branch_eq(v[0].clone(), 0);
        self.cur = is_ok;
        let okv = Self::variant_value(&d.shape, "Ok", v[ok_at..ok_at + w].to_vec());
        self.copy_to(&d.locals, okv);
        self.goto(join);
        self.cur = is_err;
        let ev = self.widen_err(v[err_at..err_at + e.width()].to_vec(), &e, &f)?;
        let errv = Self::variant_value(&d.shape, "Err", ev);
        self.copy_to(&d.locals, errv);
        self.goto(join);
        Ok(())
    }

    /// The thunk that restarts actors of state `s`, made once per shape.
    fn restart_thunk(&mut self, s: &Shape) -> FuncId {
        let mut m = self.mono.borrow_mut();
        if let Some(id) = m.restarts.get(s) {
            return *id;
        }
        let id = m.sigs.len() as FuncId;
        m.sigs.push(Sig {
            params: vec![(y::Conv::Let, PTR); 3],
            ok: Shape::Scalar(Ty::Unit),
            ret: Shape::Scalar(Ty::Unit),
            fails: false,
        });
        m.todo.push(Todo::Restart(s.clone()));
        m.restarts.insert(s.clone(), id);
        id
    }

    /// `thunk(code, env, actor)`: call the start closure `code`, `env` and give the actor the
    /// state it returns (R958).
    pub(super) fn restart(mut self, s: &Shape) -> Func {
        let p: Vec<Local> = (0..3).map(|_| self.local(Ty::Ptr)).collect();
        self.cur = self.block_new();
        let v = self.var(s.clone());
        self.emit(Inst::CallIndirect {
            dst: v.locals.clone(),
            code: Operand::Local(p[0]),
            args: vec![Operand::Local(p[1])],
            params: vec![Ty::Ptr],
            ret: s.tys(),
        });
        let buf = self.rt(
            RtFn::Alloc,
            Ty::Ptr,
            vec![Operand::Int(s.width().max(1) as i128)],
        );
        self.write(&At::Mem(buf.clone(), 0), Self::ops(&v));
        self.emit(Inst::Rt {
            dst: None,
            f: RtFn::ActorRestarted,
            args: vec![Operand::Local(p[2]), buf.clone()],
        });
        self.free(RtFn::EnvFree, buf);
        self.finish(Term::Return(vec![Operand::Unit]));
        Func {
            name: "actor.restart".into(),
            params: 3,
            ret: vec![Ty::Unit],
            locals: self.locals,
            blocks: self.blocks,
        }
    }

    /// `with supervisor(spec): body` (9.6): the actors started in `body` are children of a new
    /// supervisor, stopped when `body` ends or the function returns (R981).
    pub(super) fn with_supervisor(&mut self, spec: &y::Atom, body: &'m y::Block) -> R<()> {
        let (v, s) = self.atom_any(spec)?;
        let Some(fs) = s.fields() else {
            return unsupported("a supervisor spec that is not a `Supervisor`");
        };
        let slot = |n: &str| {
            fs.iter()
                .find(|(f, ..)| f == n)
                .map(|(_, at, _)| v[*at].clone())
        };
        let (Some(strategy), Some(intensity), Some(period)) =
            (slot("strategy"), slot("intensity"), slot("period"))
        else {
            return unsupported("a supervisor spec that is not a `Supervisor`");
        };
        let depth = match self.sup_depth {
            Some(d) => d,
            None => {
                let d = self.local(I64);
                let entry = &mut self.blocks[0];
                entry.insts.insert(
                    0,
                    Inst::Copy {
                        dst: d,
                        src: Operand::Int(i128::from(i64::MAX)),
                    },
                );
                entry.lines.insert(0, 0);
                self.sup_depth = Some(d);
                d
            }
        };
        self.emit(Inst::Rt {
            dst: Some(depth),
            f: RtFn::SupBegin,
            args: vec![Operand::Local(depth), strategy, intensity, period],
        });
        self.block(body)?;
        self.emit(Inst::Rt {
            dst: None,
            f: RtFn::SupEnd,
            args: Vec::new(),
        });
        Ok(())
    }

    /// End the `with supervisor` scopes this function opened, before it leaves.
    pub(super) fn end_sups(&mut self) {
        if let Some(d) = self.sup_depth {
            self.push(Inst::Rt {
                dst: None,
                f: RtFn::SupEndTo,
                args: vec![Operand::Local(d)],
            });
        }
    }
}
