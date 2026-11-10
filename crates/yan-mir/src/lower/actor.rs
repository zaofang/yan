//! `std.actor` in native code (RFC 0056 D8): spawn, handler calls, mailboxes, supervisor scopes
//! and restarts over `yan_rt::actor`, which hands back every closure it needs run.

use super::*;

const PTR: Shape = Shape::Scalar(Ty::Ptr);

/// The yan-rt status asking the caller to run the closure in the work slots and retry.
pub(super) const WORK: i128 = 3;

/// The yan-rt statuses of a failed actor operation, by the failure each means.
const FAILURES: &[(i128, &str)] = &[
    (4, "Stopped"),
    (5, "Deadlock"),
    (6, "Timeout"),
    (7, "Full"),
    (2, "Cancelled"),
    (8, "Unreachable"),
    (9, "SchemaMismatch"),
];

/// A thunk yan-rt hands back to run, made once per shape; each takes three pointers.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Thunk {
    /// `(start code, start env, actor)`: restart an actor of this state.
    Restart,
    /// `(code, env, args)`: run a message taking these arguments.
    Message,
}

/// `Stopped(reason)` (R977).
pub(super) fn stopped() -> Shape {
    let reason = Shape::Sum(
        "StopReason".into(),
        vec![
            ("Normal".into(), Vec::new()),
            ("Shutdown".into(), Vec::new()),
            ("Crashed".into(), vec![("error".into(), Shape::Str)]),
            ("NodeDown".into(), Vec::new()),
        ],
    );
    Shape::Rec("Stopped".into(), vec![("reason".into(), reason)])
}

/// `Undelivered[A, R]`: why a send failed and its arguments (R976).
pub(super) fn undelivered(args: Shape, reason: Shape) -> Shape {
    Shape::Rec(
        "Undelivered".into(),
        vec![("reason".into(), reason), ("args".into(), args)],
    )
}

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
            (id::STD_ACTOR_SPAWN | id::STD_ACTOR_SPAWN_AS, [(_, state), (_, cap), policy @ ..]) => {
                let s = self.state_targ(c)?;
                let v = self.atom(state, &s)?;
                let cap = self.scalar1(cap)?;
                let policy = match policy {
                    [(_, p)] => self.scalar1(p)?,
                    _ => Operand::Int(0),
                };
                let none = vec![
                    policy,
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
            (id::STD_ACTOR_SEND_ONLY, [(_, h)]) => {
                let h = self.scalar1(h)?;
                if let Some(d) = dst {
                    self.copy_to(&d.locals, vec![h]);
                }
            }
            (id::STD_ACTOR_START, [(_, start), (_, restart), (_, cap), (_, policy)]) => {
                let s = self.state_targ(c)?;
                let f = self.atom(start, &Shape::Fn(Vec::new(), Box::new(s.clone())))?;
                let (restart, cap) = (self.scalar1(restart)?, self.scalar1(cap)?);
                let policy = self.scalar1(policy)?;
                let v = self.var(s.clone());
                self.emit(Inst::CallIndirect {
                    dst: v.locals.clone(),
                    code: f[0].clone(),
                    args: vec![f[1].clone()],
                    params: vec![Ty::Ptr],
                    ret: s.tys(),
                });
                let thunk = Operand::Func(self.thunk_id(Thunk::Restart, &s));
                let how = vec![policy, restart, thunk, f[0].clone(), f[1].clone()];
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
                let buf = self.state_buf(state)?;
                let stops = self.rt(RtFn::ActorGive, I64, vec![h.clone(), buf.clone()]);
                self.free(RtFn::EnvFree, buf);
                let (v, s) = self.atom_any(outcome)?;
                let Some((_, err_at, err)) = s.variant("Err") else {
                    return unsupported("a handler outcome that is not a `Result`");
                };
                let e = err[0].1.clone();
                let failed = self.scalar_op(ScalarOp::Eq, I64, vec![v[0].clone(), Operand::Int(1)]);
                let (check, on) = self.branch(failed);
                self.cur = check;
                let stops = self.scalar_op(ScalarOp::Eq, I64, vec![stops, Operand::Int(1)]);
                let (crash, kept) = self.branch(stops);
                self.cur = kept;
                self.goto(on);
                self.cur = crash;
                // A failed handler stops its actor `Crashed` with the failure's text (R977).
                let text = self.show(&v[err_at..err_at + e.width()], &e, false)?;
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::ActorCrash,
                    args: vec![h, text.clone()],
                });
                self.free(RtFn::StrFree, text);
                self.restarts();
                self.goto(on);
                self.widen(&v, &s, d)?;
            }
            (id::STD_ACTOR_LEND, [(_, h), (_, state)]) => {
                let h = self.scalar1(h)?;
                let buf = self.state_buf(state)?;
                let since = self.rt(RtFn::ActorLend, I64, vec![h, buf.clone()]);
                self.free(RtFn::EnvFree, buf);
                if let Some(d) = dst {
                    self.copy_to(&d.locals, vec![since]);
                }
            }
            (id::STD_ACTOR_RETAKE, [(_, h), (_, since), (_, state)]) => {
                let Some(d) = dst else {
                    return unsupported("`retake` without its result");
                };
                self.retake(h, since, state, d)?;
            }
            (id::STD_ACTOR_REPLY, [(_, outcome)]) => {
                let (v, s) = self.atom_any(outcome)?;
                match dst {
                    Some(d) if d.shape == s => self.copy_to(&d.locals, v),
                    Some(_) => return unsupported("`reply` across result types"),
                    None => {}
                }
            }
            (id::STD_ACTOR_SEND, [(_, h), (_, args), (_, m)]) => {
                let Some(d) = dst else {
                    return unsupported("`send` without its result");
                };
                self.send(c, h, args, m, d)?;
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
        let buf = self.state_buf(state)?;
        self.emit(Inst::Rt {
            dst: None,
            f: RtFn::ActorPut,
            args: vec![h, buf.clone()],
        });
        self.free(RtFn::EnvFree, buf);
        Ok(())
    }

    /// A new block holding the slots of `state`, for yan-rt to copy.
    fn state_buf(&mut self, state: &y::Atom) -> R<Operand> {
        let (v, _) = self.atom_any(state)?;
        let buf = self.rt(
            RtFn::Alloc,
            Ty::Ptr,
            vec![Operand::Int(v.len().max(1) as i128)],
        );
        self.write(&At::Mem(buf.clone(), 0), v);
        Ok(buf)
    }

    /// `retake`: the state back into the `@reentrant` handler's `inout` `state`, then `Ok`, or
    /// `Err` of why not (9.6).
    fn retake(&mut self, h: &y::Atom, since: &y::Atom, state: &y::Atom, d: &Var) -> R<()> {
        let (y::Atom::Name(n, _), Some((_, _, err))) = (state, d.shape.variant("Err")) else {
            return unsupported("`retake` into a value that is not a name");
        };
        let Some(sv) = self.names.get(n).cloned() else {
            return unsupported("`retake` into an unknown name");
        };
        let e = err[0].1.clone();
        let (h, since) = (self.scalar1(h)?, self.scalar1(since)?);
        let out = self.rt(
            RtFn::Alloc,
            Ty::Ptr,
            vec![Operand::Int(sv.shape.width().max(1) as i128)],
        );
        let st = self.blocking(RtFn::ActorRetake, vec![h.clone(), since, out.clone()]);
        let join = self.block_new();
        let (taken, failed) = self.branch_eq(st.clone(), 0);
        self.cur = taken;
        let v = self.read(&At::Mem(out.clone(), 0), &sv.shape);
        self.copy_to(&sv.locals, v);
        let ok = Self::variant_value(&d.shape, "Ok", Vec::new());
        self.copy_to(&d.locals, ok);
        self.goto(join);
        self.cur = failed;
        let f = self.failed(st, &h, &e)?;
        self.copy_to(
            &d.locals,
            Self::variant_value(&d.shape, "Err", Self::ops(&f)),
        );
        self.goto(join);
        self.free(RtFn::EnvFree, out);
        Ok(())
    }

    /// The state type `S`, the first type argument of every `std.actor` operation.
    fn state_targ(&self, c: &y::Call) -> R<Shape> {
        match c.targs.first() {
            Some(y::TArg::Type(t)) => self.shape(t),
            _ => unsupported(format!("`{}` without its state type", c.path.join("."))),
        }
    }

    /// Run the message or restart in the work slots, whose thunk owns the blocks it is given.
    pub(super) fn run_work(&mut self) {
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
    }

    /// Run the message in the work slots.
    pub(super) fn run_message(&mut self) {
        self.run_work();
        self.emit(Inst::Rt {
            dst: None,
            f: RtFn::ActorRan,
            args: Vec::new(),
        });
    }

    /// `send(h, args, message)` (R813): enqueue `message` with `args` as a thunk call, into
    /// `Ok`, or `Err(Undelivered(reason, args))` with the arguments given back (R976).
    fn send(&mut self, c: &y::Call, h: &y::Atom, args: &y::Atom, m: &y::Atom, d: &Var) -> R<()> {
        let (Some((_, _, ok)), Some((_, _, err))) = (d.shape.variant("Ok"), d.shape.variant("Err"))
        else {
            return unsupported("`send` without a `Result`");
        };
        let (ok, und) = (ok[0].1.clone(), err[0].1.clone());
        let Some(a) = (match c.targs.get(2) {
            Some(y::TArg::Type(t)) => Some(self.shape(t)?),
            _ => None,
        }) else {
            return unsupported("`send` without its argument type");
        };
        let Some(ufs) = und.fields() else {
            return unsupported("`send` failing other than `Undelivered`");
        };
        let h = self.scalar1(h)?;
        let v = self.atom(args, &a)?;
        let buf = self.rt(
            RtFn::Alloc,
            Ty::Ptr,
            vec![Operand::Int(a.width().max(1) as i128)],
        );
        self.write(&At::Mem(buf.clone(), 0), v);
        let msg = Shape::Fn(vec![a.clone()], Box::new(Shape::Scalar(Ty::Unit)));
        let m = self.atom(m, &msg)?;
        let thunk = Operand::Func(self.thunk_id(Thunk::Message, &a));
        let st = self.blocking(
            RtFn::ActorSend,
            vec![h.clone(), thunk, m[0].clone(), m[1].clone(), buf.clone()],
        );
        let join = self.block_new();
        let (sent, failed) = self.branch_eq(st.clone(), 0);
        self.cur = sent;
        let unit = vec![Operand::Unit; ok.width()];
        self.copy_to(&d.locals, Self::variant_value(&d.shape, "Ok", unit));
        self.goto(join);
        self.cur = failed;
        self.free(RtFn::EnvFree, m[1].clone());
        let back = self.read(&At::Mem(buf.clone(), 0), &a);
        self.free(RtFn::EnvFree, buf);
        let mut uv = Vec::new();
        for (f, _, s) in ufs {
            match f.as_str() {
                "reason" => uv.extend(Self::ops(&self.failed(st.clone(), &h, s)?)),
                "args" => uv.extend(back.clone()),
                _ => return unsupported(format!("`Undelivered` field `{f}`")),
            }
        }
        self.copy_to(&d.locals, Self::variant_value(&d.shape, "Err", uv));
        self.goto(join);
        Ok(())
    }

    /// The failure of actor `h`'s operation with failed status `st`, as a value of `e`.
    fn failed(&mut self, st: Operand, h: &Operand, e: &Shape) -> R<Var> {
        let out = self.var(e.clone());
        let join = self.block_new();
        for (k, name) in FAILURES {
            if Self::failure(e, name, Vec::new()).is_none() {
                continue;
            }
            let (this, next) = self.branch_eq(st.clone(), *k);
            self.cur = this;
            let payload = if *name == "Stopped" {
                self.stop_reason(h)
            } else {
                Vec::new()
            };
            let v = Self::failure(e, name, payload).expect("checked above");
            self.copy_to(&out.locals, v);
            self.goto(join);
            self.cur = next;
        }
        self.finish(Term::Panic("unreachable"));
        self.cur = join;
        Ok(out)
    }

    /// Why stopped actor `h` stopped: the slots of a `Stopped`.
    fn stop_reason(&mut self, h: &Operand) -> Vec<Operand> {
        let s = stopped();
        let r = s.fields().expect("a record")[0].2.clone();
        let out = self.var(r.clone());
        let join = self.block_new();
        let kind = self.rt(RtFn::ActorReason, I64, vec![h.clone()]);
        let (crashed, shutdown) = self.branch_eq(kind, 2);
        self.cur = crashed;
        let text = self.rt(RtFn::ActorReasonText, Ty::Ptr, vec![h.clone()]);
        self.copy_to(&out.locals, Self::variant_value(&r, "Crashed", vec![text]));
        self.goto(join);
        self.cur = shutdown;
        self.copy_to(&out.locals, Self::variant_value(&r, "Shutdown", Vec::new()));
        self.goto(join);
        Self::ops(&out)
    }

    /// Restart every child a crash queued, each from its start closure through its thunk.
    fn restarts(&mut self) {
        let head = self.block_new();
        self.goto(head);
        let k = self.rt(RtFn::ActorRestartNext, I64, Vec::new());
        let (done, more) = self.branch_eq(k, 0);
        self.cur = more;
        self.run_work();
        self.goto(head);
        self.cur = done;
    }

    /// `take`: `Ok` of the actor's state, or `Err` of why not (RFC 0056 D6, R977).
    fn take(&mut self, h: &y::Atom, d: &Var) -> R<()> {
        let (Some((_, _, ok)), Some((_, _, err))) = (d.shape.variant("Ok"), d.shape.variant("Err"))
        else {
            return unsupported("`take` without a `Result`");
        };
        let (s, e) = (ok[0].1.clone(), err[0].1.clone());
        let h = self.scalar1(h)?;
        let out = self.rt(
            RtFn::Alloc,
            Ty::Ptr,
            vec![Operand::Int(s.width().max(1) as i128)],
        );
        let st = self.blocking(RtFn::ActorTake, vec![h.clone(), out.clone()]);
        let join = self.block_new();
        let (taken, failed) = self.branch_eq(st.clone(), 0);
        self.cur = taken;
        let v = self.read(&At::Mem(out.clone(), 0), &s);
        let v = Self::variant_value(&d.shape, "Ok", v);
        self.copy_to(&d.locals, v);
        self.goto(join);
        self.cur = failed;
        let f = self.failed(st, &h, &e)?;
        self.copy_to(
            &d.locals,
            Self::variant_value(&d.shape, "Err", Self::ops(&f)),
        );
        self.goto(join);
        self.free(RtFn::EnvFree, out);
        Ok(())
    }

    /// The failure `name` with `payload` as a value of the fail type `e`, if `e` has it.
    pub(super) fn failure(e: &Shape, name: &str, payload: Vec<Operand>) -> Option<Vec<Operand>> {
        match e {
            Shape::Rec(n, _) if n == name => Some(payload),
            Shape::Sum(_, _) if e.variant(name).is_some() => {
                Some(Self::variant_value(e, name, payload))
            }
            _ => None,
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

    /// The thunk `k` for shape `s`, made once.
    fn thunk_id(&mut self, k: Thunk, s: &Shape) -> FuncId {
        let mut m = self.mono.borrow_mut();
        if let Some(id) = m.thunks.get(&(k, s.clone())) {
            return *id;
        }
        let id = m.sigs.len() as FuncId;
        m.sigs.push(Sig {
            params: vec![(y::Conv::Let, PTR); 3],
            ok: Shape::Scalar(Ty::Unit),
            ret: Shape::Scalar(Ty::Unit),
            fails: false,
        });
        m.todo.push(Todo::Thunk(k, s.clone()));
        m.thunks.insert((k, s.clone()), id);
        id
    }

    /// The body of thunk `k` for shape `s`.
    pub(super) fn thunk(mut self, k: Thunk, s: &Shape) -> Func {
        let p: Vec<Operand> = (0..3)
            .map(|_| Operand::Local(self.local(Ty::Ptr)))
            .collect();
        self.cur = self.block_new();
        match k {
            // Call the start closure and give the actor the state it returns (R958).
            Thunk::Restart => {
                let v = self.var(s.clone());
                self.emit(Inst::CallIndirect {
                    dst: v.locals.clone(),
                    code: p[0].clone(),
                    args: vec![p[1].clone()],
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
                    args: vec![p[2].clone(), buf.clone()],
                });
                self.free(RtFn::EnvFree, buf);
            }
            // Call the message closure with the arguments `s` it owns.
            Thunk::Message => {
                let mut args = vec![p[1].clone()];
                args.extend(self.read(&At::Mem(p[2].clone(), 0), s));
                self.free(RtFn::EnvFree, p[2].clone());
                let mut params = vec![Ty::Ptr];
                params.extend(s.tys());
                let u = self.local(Ty::Unit);
                self.emit(Inst::CallIndirect {
                    dst: vec![u],
                    code: p[0].clone(),
                    args,
                    params,
                    ret: vec![Ty::Unit],
                });
                self.free(RtFn::EnvFree, p[1].clone());
            }
        }
        self.finish(Term::Return(vec![Operand::Unit]));
        Func {
            name: match k {
                Thunk::Restart => "actor.restart",
                Thunk::Message => "actor.message",
            }
            .into(),
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
        let depth = self.sup_depth();
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

    /// The open scope count when this function opened its first `with supervisor` or `with
    /// cluster`, for [`Self::end_sups`].
    pub(super) fn sup_depth(&mut self) -> Local {
        match self.sup_depth {
            Some(d) => d,
            None => {
                let d = self.entry_local(i128::from(i64::MAX));
                self.sup_depth = Some(d);
                d
            }
        }
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
