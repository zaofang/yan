use super::*;

impl<'a, 'm> Fn<'a, 'm> {
    /// Defines `ls` on every path, for locals the validator cannot see a guard write before a read.
    pub(super) fn zero_locals(&mut self, ls: &[Local]) {
        for l in ls {
            let src = zero(self.locals[*l as usize]);
            self.emit(Inst::Copy { dst: *l, src });
        }
    }

    pub(super) fn show_fields(
        &mut self,
        name: &str,
        v: &[Operand],
        fs: &[(String, Shape)],
    ) -> R<Operand> {
        // An instance `Box[i64]` shows as `Box`, as in the interpreter.
        let name = name.split('[').next().unwrap_or(name);
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

    /// The field `f` of the aggregate `shape` at `at`.
    pub(super) fn field_at(&mut self, at: At, shape: &Shape, f: &str) -> R<(At, Shape)> {
        let Some((_, start, s)) = shape
            .fields()
            .and_then(|fs| fs.into_iter().find(|(n, ..)| n == f))
        else {
            return unsupported(format!("field `{f}`"));
        };
        Ok(self.sub_at(at, start, s.clone()))
    }

    /// The `s` that starts `start` slots into the aggregate at `at`. A boxed one is the value in
    /// its box, changed there in place.
    pub(super) fn sub_at(&mut self, at: At, start: usize, s: Shape) -> (At, Shape) {
        let w = s.width();
        let at = match at {
            At::Reg(ls) => At::Reg(ls[start..start + w].to_vec()),
            At::Mem(p, word) => At::Mem(p, word + start as u32),
        };
        if let Shape::Boxed(k) = &s {
            let p = self.read(&at, &s).remove(0);
            return (At::Mem(p, 0), k.get().clone());
        }
        (at, s)
    }

    pub(super) fn place(&mut self, p: &y::Place) -> R<(At, Shape)> {
        let root = self.name(&p.root)?.clone();
        let mut at = At::Reg(root.locals);
        let mut shape = root.shape;
        for proj in &p.projs {
            match proj {
                y::Proj::Field(f, _)
                    if f == "value" && matches!(shape, Shape::Guard(_) | Shape::Mutex(_)) =>
                {
                    let (Shape::Guard(inner) | Shape::Mutex(inner)) = shape.clone() else {
                        unreachable!("matched")
                    };
                    let p = self.read(&at, &shape).remove(0);
                    at = At::Mem(p, super::mutex::VALUE);
                    shape = *inner;
                }
                y::Proj::Field(f, _) => (at, shape) = self.field_at(at, &shape, f)?,
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
            Shape::Scalar(_) | Shape::Char | Shape::Actor(_) | Shape::Ptr | Shape::Ref(_) => false,
            Shape::Atomic(_) | Shape::Mutex(_) | Shape::Guard(_) => true,
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
            | Shape::Chan(..)
            | Shape::Arena
            | Shape::Set(_) => true,
            // A type recurs only behind a block.
            Shape::Named(_) | Shape::Boxed(_) => true,
            Shape::Rec(_, fs) => fs.iter().any(|(_, s)| Self::has_heap(s)),
            Shape::Tuple(ss) => ss.iter().any(Self::has_heap),
            Shape::Sum(_, vs) => vs
                .iter()
                .flat_map(|(_, fs)| fs)
                .any(|(_, s)| Self::has_heap(s)),
        }
    }

    /// Run drop glue on each element still queued on channel `ch` when this is its last end.
    fn chan_drain(&mut self, ch: &Operand, e: &Shape) {
        let out = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(e.width() as i128)]);
        let head = self.block_new();
        self.goto(head);
        let got = self.rt(RtFn::ChanDrain, I64, vec![ch.clone(), out.clone()]);
        let (body, exit) = self.branch(got);
        self.cur = body;
        let ev = self.read(&At::Mem(out.clone(), 0), e);
        self.glue(e, &ev);
        self.finish(Term::Jump(head));
        self.cur = exit;
        self.free(RtFn::EnvFree, out);
    }

    /// Drop glue (5.4): free the blocks of the value `v` of shape `s`, fields in declaration
    /// order. A closure's captures block is freed, not the values it captured.
    pub(super) fn glue(&mut self, s: &Shape, v: &[Operand]) {
        if !Self::has_heap(s) {
            return;
        }
        if Self::outlined(s) {
            self.walk_shape(knot::Walk::Drop, s, v);
            return;
        }
        self.glue_body(s, v);
    }

    /// `glue` of `s` with its own type expanded and the types it holds called.
    pub(super) fn glue_body(&mut self, s: &Shape, v: &[Operand]) {
        if !Self::has_heap(s) {
            return;
        }
        match s {
            Shape::Scalar(_) | Shape::Char | Shape::Actor(_) | Shape::Ptr | Shape::Ref(_) => {}
            Shape::Arena => self.free(RtFn::ArenaFree, v[0].clone()),
            Shape::Atomic(_) => self.free(RtFn::EnvFree, v[0].clone()),
            Shape::Mutex(inner) => self.mutex_glue(inner, v[0].clone()),
            Shape::Guard(_) => self.unlock(v[0].clone()),
            Shape::Named(k) => {
                self.walk_call(knot::Walk::Drop, k, v);
            }
            Shape::Boxed(k) => {
                let x = self.unbox(k, &v[0]);
                self.walk_call(knot::Walk::Drop, k, &x);
                self.free(RtFn::EnvFree, v[0].clone());
            }
            Shape::Fn(..) => self.free(RtFn::EnvFree, v[1].clone()),
            Shape::Str | Shape::Path | Shape::WinPath => self.free(RtFn::StrFree, v[0].clone()),
            Shape::Bytes | Shape::ByteArray | Shape::BigInt => {
                self.free(RtFn::BytesFree, v[0].clone())
            }
            Shape::Chan(e, sender) => {
                if Self::has_heap(e) {
                    self.chan_drain(&v[0], e);
                }
                let f = if *sender {
                    RtFn::ChanDropSender
                } else {
                    RtFn::ChanDrop
                };
                self.free(f, v[0].clone())
            }
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
        args.push(Operand::Int(i128::from(self.in_task())));
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
        self.chan_result_with(dst, st, false, ok)
    }

    /// As [`Self::chan_result`]; with `closed`, status 3 is the receive of a closed channel,
    /// `Ok(None)`, and `ok` yields the `Some` payload.
    pub(super) fn chan_result_with(
        &mut self,
        dst: &Var,
        st: Operand,
        closed: bool,
        ok: impl FnOnce(&mut Self) -> Vec<Operand>,
    ) -> R<()> {
        let Some((_, _, efs)) = dst.shape.variant("Err") else {
            return unsupported("a channel operation without its failure");
        };
        let e = &efs[0].1;
        let join = self.block_new();
        for (name, status) in [("Cancelled", 2), ("Timeout", 5)] {
            let v = match e {
                Shape::Rec(n, fs) if n == name && fs.is_empty() => Vec::new(),
                Shape::Sum(_, _) if e.variant(name).is_some() => {
                    Self::variant_value(e, name, Vec::new())
                }
                _ if name == "Cancelled" => {
                    return unsupported("a channel failure without `Cancelled`");
                }
                _ => continue,
            };
            let err = Self::variant_value(&dst.shape, "Err", v);
            let hit = self.scalar_op(ScalarOp::Eq, I64, vec![st.clone(), Operand::Int(status)]);
            let (c, o) = self.branch(hit);
            self.cur = c;
            self.copy_to(&dst.locals, err);
            self.goto(join);
            self.cur = o;
        }
        if closed {
            let Some((_, _, ofs)) = dst.shape.variant("Ok") else {
                return unsupported("a receive without its value");
            };
            let opt = ofs[0].1.clone();
            let shut = self.scalar_op(ScalarOp::Eq, I64, vec![st, Operand::Int(3)]);
            let (n, s) = self.branch(shut);
            self.cur = n;
            let none = Self::variant_value(&opt, "None", Vec::new());
            let none = Self::variant_value(&dst.shape, "Ok", none);
            self.copy_to(&dst.locals, none);
            self.goto(join);
            self.cur = s;
            let some = Self::variant_value(&opt, "Some", ok(self));
            let v = Self::variant_value(&dst.shape, "Ok", some);
            self.copy_to(&dst.locals, v);
            self.goto(join);
            return Ok(());
        }
        let v = ok(self);
        let v = Self::variant_value(&dst.shape, "Ok", v);
        self.copy_to(&dst.locals, v);
        self.goto(join);
        Ok(())
    }

    /// The channel of the `Sender` atom `tx`; one `close` ended is a `contract` panic (9.5).
    pub(super) fn open_sender(&mut self, tx: &y::Atom) -> R<Operand> {
        let ch = self.scalar1(tx)?;
        let closed = self.scalar_op(ScalarOp::Eq, I64, vec![ch.clone(), Operand::Int(0)]);
        let (bad, good) = self.branch(closed);
        self.cur = bad;
        self.finish(Term::Panic("contract"));
        self.cur = good;
        Ok(ch)
    }

    /// `dst` of a `try_send` with status `st`: `Ok`, or the failure its status means: 2
    /// `Cancelled`, 4 `Full[T]` carrying back the unsent `payload`, which the channel did not take.
    pub(super) fn try_send_result(
        &mut self,
        dst: &Var,
        st: Operand,
        payload: Vec<Operand>,
    ) -> R<()> {
        let Some((_, _, efs)) = dst.shape.variant("Err") else {
            return unsupported("a channel operation without its failure");
        };
        let e = efs[0].1.clone();
        let join = self.block_new();
        let full = match &e {
            Shape::Sum(_, vs) => vs.iter().map(|(n, _)| n).find(|n| n.starts_with("Full[")),
            _ => None,
        }
        .cloned();
        for (k, name, payload) in [
            (2, "Cancelled".to_owned(), Vec::new()),
            (4, full.unwrap_or_default(), payload),
        ] {
            let Some(v) = Self::failure(&e, &name, payload) else {
                continue;
            };
            let (this, next) = self.branch_eq(st.clone(), k);
            self.cur = this;
            let v = Self::variant_value(&dst.shape, "Err", v);
            self.copy_to(&dst.locals, v);
            self.goto(join);
            self.cur = next;
        }
        let v = Self::variant_value(&dst.shape, "Ok", Vec::new());
        self.copy_to(&dst.locals, v);
        self.goto(join);
        self.cur = join;
        Ok(())
    }

    /// `TryRecv[t]`, declared in `std.chan`.
    pub(super) fn try_recv_shape(&self, t: &y::Type) -> R<Shape> {
        self.shape(&y::Type::Named(y::Named {
            path: vec!["TryRecv".into()],
            args: vec![y::TArg::Type(t.clone())],
        }))
    }

    /// The `Result[TryRecv[T], Cancelled]` of a `try_recv` that returned status `st`: 2 cancelled,
    /// 3 closed, 4 empty, else `ok` yields the received value.
    pub(super) fn try_recv_result(
        &mut self,
        dst: &Var,
        st: Operand,
        ok: impl FnOnce(&mut Self) -> Vec<Operand>,
    ) -> R<()> {
        let (Some((_, _, efs)), Some((_, _, ofs))) =
            (dst.shape.variant("Err"), dst.shape.variant("Ok"))
        else {
            return unsupported("`try_recv` without its result");
        };
        let (e, t) = (efs[0].1.clone(), ofs[0].1.clone());
        let Some(cancelled) = Self::failure(&e, "Cancelled", Vec::new()) else {
            return unsupported("a channel failure without `Cancelled`");
        };
        let join = self.block_new();
        let (this, next) = self.branch_eq(st.clone(), 2);
        self.cur = this;
        let v = Self::variant_value(&dst.shape, "Err", cancelled);
        self.copy_to(&dst.locals, v);
        self.goto(join);
        self.cur = next;
        for (k, name) in [(3, "Closed"), (4, "Empty")] {
            let (this, next) = self.branch_eq(st.clone(), k);
            self.cur = this;
            let v = Self::variant_value(&t, name, Vec::new());
            let v = Self::variant_value(&dst.shape, "Ok", v);
            self.copy_to(&dst.locals, v);
            self.goto(join);
            self.cur = next;
        }
        let got = ok(self);
        let v = Self::variant_value(&t, "Got", got);
        let v = Self::variant_value(&dst.shape, "Ok", v);
        self.copy_to(&dst.locals, v);
        self.goto(join);
        self.cur = join;
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
            y::Rhs::Rest { root, moved } => {
                let Some(v) = self.names.get(root) else {
                    return unsupported(format!("`rest` of `{root}`, which is not a local"));
                };
                if let Some(path) = moved.iter().find(|p| !v.shape.resolves(p)) {
                    return unsupported(format!(
                        "a move out of `{root}.{}`, which is not a field",
                        path.join(".")
                    ));
                }
                Ok(v.locals.iter().map(|l| Operand::Local(*l)).collect())
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
            match self.rests.get(n).cloned() {
                Some(moved) => self.glue_except(&v.shape, &ops, &moved),
                None => self.glue(&v.shape, &ops),
            }
        }
    }

    /// `glue` of what is left of `s` once the fields at the paths `moved` were moved out: every
    /// other field is dropped, and the block of a boxed value is freed.
    pub(super) fn glue_except(&mut self, s: &Shape, v: &[Operand], moved: &[Vec<String>]) {
        if moved.is_empty() || !Self::has_heap(s) {
            return self.glue(s, v);
        }
        match s {
            Shape::Rec(..) | Shape::Tuple(_) => {
                for (f, at, fs) in s.fields().expect("an aggregate") {
                    self.glue_field(fs, &v[at..at + fs.width()], moved, &f);
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
                for (arm, (name, fs)) in arms.into_iter().zip(vs) {
                    self.cur = arm;
                    let inside = Self::under(moved, name);
                    for (f, fs) in fs {
                        let w = fs.width();
                        self.glue_field(fs, &v[at..at + w], &inside, f);
                        at += w;
                    }
                    self.finish(Term::Jump(join));
                }
                self.cur = join;
            }
            Shape::Boxed(k) => {
                let x = self.unbox(k, &v[0]);
                self.glue_except(k.get(), &x, moved);
                self.free(RtFn::EnvFree, v[0].clone());
            }
            Shape::Named(k) => self.glue_except(k.get(), v, moved),
            _ => self.glue(s, v),
        }
    }

    /// The paths of `moved` that start with `name`, without it.
    fn under(moved: &[Vec<String>], name: &str) -> Vec<Vec<String>> {
        moved
            .iter()
            .filter(|p| p.first().is_some_and(|f| f == name))
            .map(|p| p[1..].to_vec())
            .collect()
    }

    fn glue_field(&mut self, fs: &Shape, v: &[Operand], moved: &[Vec<String>], name: &str) {
        let inside = Self::under(moved, name);
        // A path that ends at the field moved it whole. The value left its block, which a boxed
        // field still owns.
        if inside.iter().any(Vec::is_empty) {
            if matches!(fs, Shape::Boxed(_)) {
                self.free(RtFn::EnvFree, v[0].clone());
            }
            return;
        }
        self.glue_except(fs, v, &inside);
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
            // `fs.read` and `fs.list_dir` fail with the `FsError` the caller propagates; their
            // lowering checks that.
            None if matches!(
                registry::id(&c.path.join(".")),
                Some(id::STD_FS_READ | id::STD_FS_LIST_DIR)
            ) =>
            {
                let Some((_, _, mine_e)) = mine.variant("Err") else {
                    return unsupported("`?` outside a failing function");
                };
                let ok = match registry::id(&c.path.join(".")) {
                    Some(id::STD_FS_LIST_DIR) => Shape::List(Box::new(Shape::Path)),
                    _ => Shape::Str,
                };
                result(ok, mine_e[0].1.clone())
            }
            None => match self.intrinsic_result(c)? {
                Some(s) => s,
                None => return unsupported(format!("`?` on `{}`", c.path.join("."))),
            },
        };
        let (Some((_, ok_at, okfs)), Some((_, err_at, efs)), Some((_, _, mine_e))) = (
            theirs.variant("Ok"),
            theirs.variant("Err"),
            mine.variant("Err"),
        ) else {
            return unsupported("`?` outside a failing function");
        };
        let (e, mine_e) = (efs[0].1.clone(), mine_e[0].1.clone());
        let got = okfs[0].1.clone();
        let r = self.var(theirs);
        let v = Self::ops(&r);
        self.call(c, Some(r))?;
        let is_ok = self.scalar_op(ScalarOp::Eq, I64, vec![v[0].clone(), Operand::Int(0)]);
        let (ok, err) = self.branch(is_ok);
        self.cur = err;
        let ev = v[err_at..err_at + e.width()].to_vec();
        let mut ev = self.widen_err(ev, &e, &mine_e)?;
        if self.shields > 0 {
            ev = self.shield_failure(&mine_e, ev);
        }
        self.unwind(drops, Some((&mine_e, &ev)));
        self.ret(Self::variant_value(&mine, "Err", ev));
        self.cur = ok;
        // A statement `c?` asks for `Unit` and drops the value it discards.
        if *want == Shape::Scalar(Ty::Unit) && got != *want {
            self.glue(&got, &v[ok_at..ok_at + got.width()]);
        }
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
            Some(i @ (id::STD_JSON_PARSE_VALUE | id::STD_JSON_PARSE_FLOAT)) => {
                Some(self.json_parse_result(i == id::STD_JSON_PARSE_FLOAT)?)
            }
            Some(id::STD_PRELUDE_CHR) => Some(self.encoding_shape(Shape::Char)?),
            Some(id::STD_ENCODING_BASE64_DECODE) => Some(self.encoding_shape(Shape::Bytes)?),
            Some(id::STD_ENCODING_HEX_DECODE) => Some(self.encoding_shape(Shape::Bytes)?),
            Some(id::STD_CHAN_RECEIVER_RECV) => self.targ(c)?.map(|e| {
                let option = Shape::Sum(
                    "Option".into(),
                    vec![
                        ("Some".into(), vec![("value".into(), e)]),
                        ("None".into(), vec![]),
                    ],
                );
                result(option, failure_union(&["Cancelled", "Timeout"]))
            }),
            Some(id::STD_PRELUDE_SELECT_WAIT) => Some(result(
                Shape::Scalar(I64),
                failure_union(&["Cancelled", "Timeout"]),
            )),
            Some(id::STD_CHAN_RECEIVER_TRY_RECV) => match c.targs.as_slice() {
                [y::TArg::Type(t)] => Some(result(self.try_recv_shape(t)?, cancelled())),
                _ => None,
            },
            Some(id::STD_CHAN_SENDER_TRY_SEND) => match c.targs.as_slice() {
                [y::TArg::Type(t)] => {
                    let named = |n: &str, args| {
                        y::Type::Named(y::Named {
                            path: vec![n.into()],
                            args,
                        })
                    };
                    let fail = y::Type::Union(vec![
                        named("Full", vec![y::TArg::Type(t.clone())]),
                        named("Cancelled", Vec::new()),
                    ]);
                    Some(result(Shape::Scalar(Ty::Unit), self.shape(&fail)?))
                }
                _ => None,
            },
            Some(id::STD_CHAN_SENDER_SEND) => Some(result(
                Shape::Scalar(Ty::Unit),
                failure_union(&["Cancelled", "Timeout"]),
            )),
            Some(id::STD_SYNC_ATOMIC_WAIT) => Some(result(Shape::Scalar(Ty::Unit), cancelled())),
            Some(id::STD_TIME_SLEEP | id::STD_TIME_SLEEP_UNTIL) => Some(result(
                Shape::Scalar(Ty::Unit),
                failure_union(&["Cancelled", "Timeout"]),
            )),
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
        if let (Shape::Sum(en, evs), Shape::Rec(fname, _)) = (e, f)
            && en == "Union"
        {
            return self.narrow_err(ev, evs, fname, f);
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
                // W863: the failure is not in the declared fail type; it cannot propagate.
                let Some(t) = tag else {
                    return unsupported("`?` across error types");
                };
                let is = self.scalar_op(ScalarOp::Eq, I64, vec![ev[0].clone(), Operand::Int(t)]);
                let (this, next) = self.branch(is);
                self.cur = this;
                self.finish(Term::Panic("a failure outside the declared fail type"));
                self.cur = next;
                continue;
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

    /// The failure `ev` of the union `evs` as the single failure type `f` named `fname`: its
    /// member of that name, and a panic for any other member (W863).
    fn narrow_err(
        &mut self,
        ev: Vec<Operand>,
        evs: &[(String, Vec<(String, Shape)>)],
        fname: &str,
        f: &Shape,
    ) -> R<Vec<Operand>> {
        let Some(tag) = evs.iter().position(|(n, _)| n == fname) else {
            return unsupported("`?` across error types");
        };
        let at = 1 + evs[..tag]
            .iter()
            .flat_map(|(_, fs)| fs)
            .map(|(_, s)| s.width())
            .sum::<usize>();
        let w = f.width();
        let is = self.scalar_op(
            ScalarOp::Eq,
            I64,
            vec![ev[0].clone(), Operand::Int(tag as i128)],
        );
        let (this, other) = self.branch(is);
        self.cur = other;
        self.finish(Term::Panic("a failure outside the declared fail type"));
        self.cur = this;
        Ok(ev[at..at + w].to_vec())
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
                    let xv = match s {
                        Shape::Boxed(k) => {
                            let v = self.atom(x, k.get())?;
                            vec![self.boxed(v)]
                        }
                        s => self.atom(x, s)?,
                    };
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
