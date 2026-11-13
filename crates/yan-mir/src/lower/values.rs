use super::*;

impl<'a, 'm> Fn<'a, 'm> {
    pub(super) fn show_fields(
        &mut self,
        name: &str,
        v: &[Operand],
        fs: &[(String, Shape)],
    ) -> R<Operand> {
        let mut parts = vec![self.lit(&format!("{name}("))];
        let mut at = 0;
        for (i, (n, s)) in fs.iter().enumerate() {
            let sep = if i > 0 { ", " } else { "" };
            parts.push(self.lit(&format!("{sep}{n}=")));
            let w = s.width();
            parts.push(self.show(&v[at..at + w], s, true)?);
            at += w;
        }
        parts.push(self.lit(")"));
        Ok(self.concat(parts))
    }

    pub(super) fn place(&mut self, p: &y::Place) -> R<(At, Shape)> {
        let root = self.name(&p.root)?.clone();
        let mut at = At::Reg(root.locals);
        let mut shape = root.shape;
        for proj in &p.projs {
            match proj {
                y::Proj::Field(f, _) => {
                    let Some((_, start, s)) = shape
                        .fields()
                        .and_then(|fs| fs.into_iter().find(|(n, ..)| n == f))
                    else {
                        return unsupported(format!("field `{f}`"));
                    };
                    let w = s.width();
                    let s = s.clone();
                    at = match at {
                        At::Reg(ls) => At::Reg(ls[start..start + w].to_vec()),
                        At::Mem(p, word) => At::Mem(p, word + start as u32),
                    };
                    shape = s;
                }
                y::Proj::Index(i) => {
                    if matches!(shape, Shape::Bytes | Shape::ByteArray) {
                        let b = self.read(&at, &shape).remove(0);
                        let i = self.scalar1(i)?;
                        at = At::Mem(self.rt(RtFn::BytesIndex, Ty::Ptr, vec![b, i]), 0);
                        shape = Shape::Scalar(Ty::Int(IntTy {
                            bits: 8,
                            signed: false,
                        }));
                        continue;
                    }
                    // A sorted set is its one list of items.
                    if let Shape::Rec(n, fs) = &shape
                        && n == "SortedSet"
                        && let [(_, inner)] = fs.as_slice()
                    {
                        shape = inner.clone();
                    }
                    let (Shape::List(e) | Shape::Deque(e)) = shape.clone() else {
                        return unsupported("indexing a non-list");
                    };
                    let list = self.read(&at, &shape).remove(0);
                    let i = self.scalar1(i)?;
                    at = At::Mem(self.rt(RtFn::ListAt, Ty::Ptr, vec![list, i]), 0);
                    shape = *e;
                }
            }
        }
        Ok((at, shape))
    }

    pub(super) fn has_heap(s: &Shape) -> bool {
        match s {
            Shape::Scalar(_) | Shape::Char | Shape::Actor(_) | Shape::Ptr => false,
            Shape::Str
            | Shape::Bytes
            | Shape::ByteArray
            | Shape::BigInt
            | Shape::Path
            | Shape::WinPath
            | Shape::List(_)
            | Shape::Map(..)
            | Shape::Heap(_)
            | Shape::Deque(_)
            | Shape::Fn(..)
            | Shape::Chan(_)
            | Shape::Set(_) => true,
            Shape::Rec(_, fs) => fs.iter().any(|(_, s)| Self::has_heap(s)),
            Shape::Tuple(ss) => ss.iter().any(Self::has_heap),
            Shape::Sum(_, vs) => vs
                .iter()
                .flat_map(|(_, fs)| fs)
                .any(|(_, s)| Self::has_heap(s)),
        }
    }

    /// Drop glue (5.4): free the blocks of the value `v` of shape `s`, fields in declaration
    /// order. A closure's captures block is freed, not the values it captured.
    pub(super) fn glue(&mut self, s: &Shape, v: &[Operand]) {
        if !Self::has_heap(s) {
            return;
        }
        match s {
            Shape::Scalar(_) | Shape::Char | Shape::Actor(_) | Shape::Ptr => {}
            Shape::Fn(..) => self.free(RtFn::EnvFree, v[1].clone()),
            Shape::Str | Shape::Path | Shape::WinPath => self.free(RtFn::StrFree, v[0].clone()),
            Shape::Bytes | Shape::ByteArray | Shape::BigInt => {
                self.free(RtFn::BytesFree, v[0].clone())
            }
            Shape::Chan(_) => self.free(RtFn::ChanDrop, v[0].clone()),
            Shape::List(e) | Shape::Heap(e) | Shape::Deque(e) => {
                if Self::has_heap(e) {
                    let list = v[0].clone();
                    let n = self.rt(RtFn::ListLen, I64, vec![list.clone()]);
                    let i = self.local(I64);
                    self.emit(Inst::Copy {
                        dst: i,
                        src: Operand::Int(0),
                    });
                    let head = self.block_new();
                    self.goto(head);
                    let done = self.scalar_op(ScalarOp::Ge, I64, vec![Operand::Local(i), n]);
                    let (exit, body) = self.branch(done);
                    self.cur = body;
                    let at = self.rt(RtFn::ListAt, Ty::Ptr, vec![list, Operand::Local(i)]);
                    let ev = self.read(&At::Mem(at, 0), e);
                    self.glue(e, &ev);
                    let next = self.scalar_op(
                        ScalarOp::AddChecked,
                        I64,
                        vec![Operand::Local(i), Operand::Int(1)],
                    );
                    self.emit(Inst::Copy { dst: i, src: next });
                    self.finish(Term::Jump(head));
                    self.cur = exit;
                }
                self.free(RtFn::ListFree, v[0].clone());
            }
            Shape::Set(k) => self.glue(&Self::set_map(k), v),
            Shape::Map(k, e) => {
                let m = v[0].clone();
                let n = self.rt(RtFn::MapLen, I64, vec![m.clone()]);
                self.each(n, |me, i| {
                    let at = me.rt(RtFn::MapAt, Ty::Ptr, vec![m.clone(), i]);
                    let kv = me.read(&At::Mem(at.clone(), 0), k);
                    me.glue(k, &kv);
                    let ev = me.read(&At::Mem(at, k.width() as u32), e);
                    me.glue(e, &ev);
                    Ok(())
                })
                .expect("glue does not fail");
                self.free(RtFn::MapFree, m);
            }
            Shape::Rec(..) | Shape::Tuple(_) => {
                for (_, at, fs) in s.fields().expect("an aggregate") {
                    self.glue(fs, &v[at..at + fs.width()]);
                }
            }
            Shape::Sum(_, vs) => {
                let Operand::Local(tag) = v[0] else {
                    unreachable!("a sum's tag is a local")
                };
                let join = self.block_new();
                let arms: Vec<BlockId> = vs.iter().map(|_| self.block_new()).collect();
                self.finish(Term::Switch {
                    on: tag,
                    targets: arms.clone(),
                });
                let mut at = 1;
                for (arm, (_, fs)) in arms.into_iter().zip(vs) {
                    self.cur = arm;
                    for (_, fs) in fs {
                        let w = fs.width();
                        self.glue(fs, &v[at..at + w]);
                        at += w;
                    }
                    self.finish(Term::Jump(join));
                }
                self.cur = join;
            }
        }
    }

    /// A channel or actor operation; one that parks suspends its task and runs again when the
    /// task resumes. It can park inside a task, or anywhere in a coroutine variant (`co`). One
    /// that cannot park runs the actor message yan-rt hands back, then runs again (RFC 0056 D4).
    /// Its status: 0, or 2 when the task is cancelled (9.4), or an actor operation's own.
    pub(super) fn blocking(&mut self, f: RtFn, mut args: Vec<Operand>) -> Operand {
        args.push(Operand::Int(i128::from(!self.task_starts.is_empty())));
        let op = self.block_new();
        self.goto(op);
        let st = self.rt(f, I64, args);
        let parked = self.scalar_op(ScalarOp::Eq, I64, vec![st.clone(), Operand::Int(1)]);
        let (suspend, on) = self.branch(parked);
        self.cur = suspend;
        self.finish(Term::Suspend { resume: op });
        self.cur = on;
        let work = self.scalar_op(
            ScalarOp::Eq,
            I64,
            vec![st.clone(), Operand::Int(actor::WORK)],
        );
        let (run, on) = self.branch(work);
        self.cur = run;
        self.run_message();
        self.goto(op);
        self.cur = on;
        st
    }

    /// `dst` of a channel operation with status `st`: `Ok` of what `ok` reads, or
    /// `Err(Cancelled)`.
    pub(super) fn chan_result(
        &mut self,
        dst: &Var,
        st: Operand,
        ok: impl FnOnce(&mut Self) -> Vec<Operand>,
    ) -> R<()> {
        let Some((_, _, efs)) = dst.shape.variant("Err") else {
            return unsupported("a channel operation without its failure");
        };
        let e = &efs[0].1;
        let err = match e {
            Shape::Rec(n, fs) if n == "Cancelled" && fs.is_empty() => Vec::new(),
            Shape::Sum(_, _) if e.variant("Cancelled").is_some() => {
                Self::variant_value(e, "Cancelled", Vec::new())
            }
            _ => return unsupported("a channel failure without `Cancelled`"),
        };
        let err = Self::variant_value(&dst.shape, "Err", err);
        let cancelled = self.scalar_op(ScalarOp::Eq, I64, vec![st, Operand::Int(2)]);
        let (c, o) = self.branch(cancelled);
        let join = self.block_new();
        self.cur = c;
        self.copy_to(&dst.locals, err);
        self.goto(join);
        self.cur = o;
        let v = ok(self);
        let v = Self::variant_value(&dst.shape, "Ok", v);
        self.copy_to(&dst.locals, v);
        self.goto(join);
        Ok(())
    }

    pub(super) fn free(&mut self, f: RtFn, block: Operand) {
        self.emit(Inst::Rt {
            dst: None,
            f,
            args: vec![block],
        });
    }

    pub(super) fn read(&mut self, at: &At, s: &Shape) -> Vec<Operand> {
        match at {
            At::Reg(ls) => ls.iter().map(|l| Operand::Local(*l)).collect(),
            At::Mem(p, word) => s
                .tys()
                .into_iter()
                .enumerate()
                .map(|(i, t)| {
                    let d = self.local(t);
                    self.emit(Inst::Load {
                        dst: d,
                        ptr: p.clone(),
                        word: word + i as u32,
                    });
                    Operand::Local(d)
                })
                .collect(),
        }
    }

    pub(super) fn write(&mut self, at: &At, v: Vec<Operand>) {
        match at {
            At::Reg(ls) => self.copy_to(&ls.clone(), v),
            At::Mem(p, word) => {
                for (i, src) in v.into_iter().enumerate() {
                    self.emit(Inst::Store {
                        ptr: p.clone(),
                        word: word + i as u32,
                        src,
                    });
                }
            }
        }
    }

    /// The value of `r`, of type `want`.
    pub(super) fn value(&mut self, r: &'m y::Rhs, want: &Shape) -> R<Vec<Operand>> {
        match r {
            y::Rhs::Atom(a) => self.atom(a, want),
            y::Rhs::Call(c) => {
                let v = self.var(want.clone());
                let ops = Self::ops(&v);
                self.call(c, Some(v))?;
                Ok(ops)
            }
            y::Rhs::Proj(p) | y::Rhs::Move(p) => {
                let (at, s) = self.place(p)?;
                Ok(self.read(&at, &s))
            }
            y::Rhs::Copy(p) => {
                let (at, s) = self.place(p)?;
                let v = self.read(&at, &s);
                self.clone_value(v, &s)
            }
            y::Rhs::Ctor(c) => self.ctor(c, want),
            y::Rhs::Try(c) => {
                let v = self.var(want.clone());
                let ops = Self::ops(&v);
                self.call(c, Some(v))?;
                Ok(ops)
            }
            y::Rhs::Propagate {
                call,
                via: None,
                drops,
            } => self.propagate(call, drops, want),
            y::Rhs::Propagate { .. } => unsupported("`?` through a conversion"),
            y::Rhs::Closure(c) => self.make_closure(c, want),
        }
    }

    /// `c?`: its `Ok` value, or return its error from this function.
    pub(super) fn drop_name(&mut self, n: &str) {
        if self.pipes.remove(n).is_some() {
            return;
        }
        if let Some(v) = self.names.get(n).cloned() {
            let ops: Vec<Operand> = v.locals.iter().map(|l| Operand::Local(*l)).collect();
            self.glue(&v.shape, &ops);
        }
    }

    pub(super) fn propagate(
        &mut self,
        c: &y::Call,
        drops: &[y::Name],
        want: &Shape,
    ) -> R<Vec<Operand>> {
        let mine = self.sig(self.me as FuncId).ret.clone();
        let theirs = match self.callee(c)? {
            Some(func) => self.sig(func).ret.clone(),
            // `fs.read` fails with the `FsError` the caller propagates; its lowering checks that.
            None if registry::id(&c.path.join(".")) == Some(id::STD_FS_READ) => {
                let Some((_, _, mine_e)) = mine.variant("Err") else {
                    return unsupported("`?` outside a failing function");
                };
                result(Shape::Str, mine_e[0].1.clone())
            }
            None => match self.intrinsic_result(c)? {
                Some(s) => s,
                None => return unsupported(format!("`?` on `{}`", c.path.join("."))),
            },
        };
        let (Some((_, ok_at, _)), Some((_, err_at, efs)), Some((_, _, mine_e))) = (
            theirs.variant("Ok"),
            theirs.variant("Err"),
            mine.variant("Err"),
        ) else {
            return unsupported("`?` outside a failing function");
        };
        let (e, mine_e) = (efs[0].1.clone(), mine_e[0].1.clone());
        let r = self.var(theirs);
        let v = Self::ops(&r);
        self.call(c, Some(r))?;
        let is_ok = self.scalar_op(ScalarOp::Eq, I64, vec![v[0].clone(), Operand::Int(0)]);
        let (ok, err) = self.branch(is_ok);
        self.cur = err;
        let ev = v[err_at..err_at + e.width()].to_vec();
        let ev = self.widen_err(ev, &e, &mine_e)?;
        self.unwind(drops, Some((&mine_e, &ev)));
        self.ret(Self::variant_value(&mine, "Err", ev));
        self.cur = ok;
        Ok(v[ok_at..ok_at + want.width()].to_vec())
    }

    /// The `Result` a fallible std call `c` makes, for `?` on it.
    fn intrinsic_result(&mut self, c: &y::Call) -> R<Option<Shape>> {
        let cancelled = || Shape::Rec("Cancelled".into(), Vec::new());
        Ok(match registry::id(&c.path.join(".")) {
            Some(id::STD_ACTOR_REPLY) => match Self::value_args(c)?.as_slice() {
                [(_, a)] => Some(self.atom_any(a)?.1),
                _ => None,
            },
            Some(id::STD_ENCODING_DECODE_UTF8) => Some(self.encoding_shape(Shape::Str)?),
            Some(id::STD_REGEX_COMPILE) => {
                let named = |me: &mut Self, n: &str| {
                    me.shape(&y::Type::Named(y::Named {
                        path: vec![n.into()],
                        args: Vec::new(),
                    }))
                };
                let ok = named(self, "Regex")?;
                let err = named(self, "RegexError")?;
                Some(result(ok, err))
            }
            Some(id::STD_PROC_RUN) => {
                let named = |me: &mut Self, n: &str| {
                    me.shape(&y::Type::Named(y::Named {
                        path: vec![n.into()],
                        args: Vec::new(),
                    }))
                };
                let (ok, err) = (named(self, "Output")?, named(self, "ProcError")?);
                Some(result(ok, err))
            }
            Some(id::STD_CLI_PARSE_ARGS) => match self.targ(c)? {
                Some(ok) => super::civil::std_type("CliError").map(|e| result(ok, e)),
                None => None,
            },
            Some(i) if super::civil::fail_ok(i).is_some() => {
                let ok = super::civil::std_type(super::civil::fail_ok(i).expect("checked"));
                ok.zip(super::civil::std_type("TimeError"))
                    .map(|(o, e)| result(o, e))
            }
            Some(id::STD_ERROR_CONTEXT) => self.context_shape(c)?,
            Some(id::STD_PRELUDE_CHR) => Some(self.encoding_shape(Shape::Char)?),
            Some(id::STD_ENCODING_BASE64_DECODE) => Some(self.encoding_shape(Shape::Bytes)?),
            Some(id::STD_ENCODING_HEX_DECODE) => Some(self.encoding_shape(Shape::Bytes)?),
            Some(id::STD_CHAN_RECEIVER_RECV) => self.targ(c)?.map(|e| result(e, cancelled())),
            Some(id::STD_CHAN_SENDER_SEND | id::STD_TIME_SLEEP | id::STD_TIME_SLEEP_UNTIL) => {
                Some(result(Shape::Scalar(Ty::Unit), cancelled()))
            }
            Some(id::STD_ACTOR_RETAKE) => match c.targs.get(2) {
                Some(y::TArg::Type(f)) => Some(result(Shape::Scalar(Ty::Unit), self.shape(f)?)),
                _ => None,
            },
            _ => None,
        })
    }

    /// The failure `ev` of shape `e` as one of the fail type `f`: `e` itself, a member of the
    /// union `f`, or a union of its members.
    pub(super) fn widen_err(&mut self, ev: Vec<Operand>, e: &Shape, f: &Shape) -> R<Vec<Operand>> {
        if e == f {
            return Ok(ev);
        }
        let Shape::Sum(fu, fvs) = f else {
            return unsupported("`?` across error types");
        };
        if fu != "Union" {
            return unsupported("`?` across error types");
        }
        // Each member of `e` with its tag in `e` (none when `e` is one member) and its slots.
        let members: Vec<(Option<i128>, String, Vec<Operand>)> = match e {
            Shape::Sum(n, vs) if n == "Union" => {
                let mut at = 1;
                let mut out = Vec::new();
                for (tag, (name, fs)) in vs.iter().enumerate() {
                    let w: usize = fs.iter().map(|(_, s)| s.width()).sum();
                    out.push((Some(tag as i128), name.clone(), ev[at..at + w].to_vec()));
                    at += w;
                }
                out
            }
            _ => match fvs
                .iter()
                .find(|(_, fs)| matches!(fs.as_slice(), [(_, m)] if m == e))
            {
                Some((name, _)) => vec![(None, name.clone(), ev.clone())],
                None => return unsupported("`?` across error types"),
            },
        };
        let out = self.var(f.clone());
        let join = self.block_new();
        for (tag, name, payload) in members {
            if f.variant(&name).is_none() {
                return unsupported("`?` across error types");
            }
            let next = match tag {
                Some(t) => {
                    let is =
                        self.scalar_op(ScalarOp::Eq, I64, vec![ev[0].clone(), Operand::Int(t)]);
                    let (this, next) = self.branch(is);
                    self.cur = this;
                    Some(next)
                }
                None => None,
            };
            let v = Self::variant_value(f, &name, payload);
            self.copy_to(&out.locals, v);
            self.goto(join);
            if let Some(n) = next {
                self.cur = n;
            }
        }
        if !self.done[self.cur as usize] && self.cur != join {
            self.finish(Term::Panic("unreachable"));
        }
        self.cur = join;
        Ok(Self::ops(&out))
    }

    /// Lift `c` to its own function; its value is that code and a block of the captures.
    pub(super) fn make_closure(&mut self, c: &'m y::Closure, want: &Shape) -> R<Vec<Operand>> {
        let mut caps = Vec::new();
        let mut vals = Vec::new();
        for (conv, n, _) in &c.captures {
            if *conv == y::Conv::Inout {
                return unsupported("an `inout` capture");
            }
            let v = self.name(n)?.clone();
            vals.extend(Self::ops(&v));
            caps.push((n.clone(), v.shape));
        }
        let Shape::Fn(ps, ret) = want else {
            return unsupported("a closure of this type");
        };
        let mut params = vec![(y::Conv::Let, Shape::Scalar(Ty::Ptr))];
        params.extend(ps.iter().map(|s| (y::Conv::Let, s.clone())));
        let ok = match ret.variant("Err").filter(|_| c.fail.is_some()) {
            Some(_) => match &**ret {
                Shape::Sum(_, vs) => vs[0].1[0].1.clone(),
                _ => unreachable!("a Result"),
            },
            None => (**ret).clone(),
        };
        let id = {
            let mut m = self.mono.borrow_mut();
            let id = m.sigs.len() as FuncId;
            m.sigs.push(Sig {
                params,
                ok,
                ret: (**ret).clone(),
                fails: c.fail.is_some(),
            });
            m.todo.push(Todo::Closure(c, caps));
            id
        };
        if self.costing && !vals.is_empty() {
            self.count(1);
        }
        let block = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(vals.len() as i128)]);
        self.write(&At::Mem(block.clone(), 0), vals);
        Ok(vec![Operand::Func(id), block])
    }

    pub(super) fn ctor(&mut self, c: &y::Ctor, want: &Shape) -> R<Vec<Operand>> {
        match (c, want) {
            (y::Ctor::Tuple(xs), Shape::Tuple(ss)) if xs.len() == ss.len() => {
                let mut v = Vec::new();
                for (x, s) in xs.iter().zip(ss) {
                    v.extend(self.atom(x, s)?);
                }
                Ok(v)
            }
            (y::Ctor::Named { path, fields, .. }, _) => {
                let name = path.last().expect("named");
                let (tag, start, fs): (Option<i128>, usize, Vec<(String, Shape)>) = match want {
                    Shape::Rec(_, fs) => (None, 0, fs.clone()),
                    Shape::Sum(..) => match want.variant(name) {
                        Some((t, at, fs)) => (Some(t), at, fs.to_vec()),
                        None => return unsupported(format!("constructor `{name}`")),
                    },
                    _ => return unsupported(format!("constructor `{name}`")),
                };
                let mut v: Vec<Operand> = want.tys().into_iter().map(zero).collect();
                if let Some(t) = tag {
                    v[0] = Operand::Int(t);
                }
                let mut at = start;
                for (n, s) in &fs {
                    let Some((_, x)) = fields.iter().find(|(f, _)| f == n) else {
                        return unsupported(format!("constructor `{name}` without field `{n}`"));
                    };
                    let xv = self.atom(x, s)?;
                    let w = xv.len();
                    v.splice(at..at + w, xv);
                    at += w;
                }
                Ok(v)
            }
            _ => unsupported("a constructor"),
        }
    }
}
