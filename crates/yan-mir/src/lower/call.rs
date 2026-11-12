use super::*;

impl<'a, 'm> Fn<'a, 'm> {
    pub(super) fn call(&mut self, c: &y::Call, dst: Option<Var>) -> R<()> {
        let name = c.path.join(".");
        if let Some(func) = self.callee(c)? {
            let (args, back) = self.user_args(func, c)?;
            let mut d = match dst {
                Some(v) => v.locals,
                None => self.var(self.sig(func).ret.clone()).locals,
            };
            d.extend(back);
            self.emit(Inst::Call { dst: d, func, args });
            return Ok(());
        }
        if let (
            [n],
            Some(Var {
                shape: Shape::Fn(ps, ret),
                locals,
            }),
        ) = (
            c.path.as_slice(),
            self.names.get(&c.path.join(".")).cloned(),
        ) {
            let args = Self::value_args(c)?;
            if args.len() != ps.len() {
                return unsupported(format!("call of `{n}` with these arguments"));
            }
            let mut ops = vec![Operand::Local(locals[1])];
            let mut tys = vec![Ty::Ptr];
            for ((_, a), s) in args.into_iter().zip(&ps) {
                ops.extend(self.atom(a, s)?);
                tys.extend(s.tys());
            }
            let d = match dst {
                Some(v) => v.locals,
                None => self.var((*ret).clone()).locals,
            };
            self.emit(Inst::CallIndirect {
                dst: d,
                code: Operand::Local(locals[0]),
                args: ops,
                params: tys,
                ret: ret.tys(),
            });
            return Ok(());
        }
        let Some(id) = registry::id(&name) else {
            return unsupported(format!("call of `{name}`"));
        };
        let args = Self::value_args(c)?;
        let dst1 = dst.as_ref().and_then(|v| v.locals.first().copied());
        let ct = match id {
            id::STD_CT_CT_SELECT => Some(ScalarOp::CtSelect),
            id::STD_CT_CT_EQ => Some(ScalarOp::CtEq),
            id::STD_CT_CT_LT => Some(ScalarOp::CtLt),
            _ => None,
        };
        if let Some(op) = ct {
            let args = args
                .iter()
                .map(|(_, a)| self.scalar1(a))
                .collect::<R<Vec<_>>>()?;
            self.emit(Inst::Scalar {
                dst: dst1,
                id,
                op,
                ty: I64,
                args,
            });
            return Ok(());
        }
        if let Some((op, ty)) = scalar(&name) {
            let args = args
                .iter()
                .map(|(_, a)| self.scalar1(a))
                .collect::<R<Vec<_>>>()?;
            let ty = match ty {
                Some(t) => t,
                None => match args.first() {
                    Some(Operand::Local(l)) => self.locals[*l as usize],
                    _ => I64,
                },
            };
            self.emit(Inst::Scalar {
                dst: dst1,
                id,
                op,
                ty,
                args,
            });
            return Ok(());
        }
        let set = |me: &mut Self, v: Operand| {
            if let Some(d) = dst1 {
                me.emit(Inst::Copy { dst: d, src: v });
            }
        };
        if [
            id::STD_DICT_NEW,
            id::STD_DICT_INSERT,
            id::STD_DICT_INDEX,
            id::STD_DICT_GET,
            id::STD_DICT_CONTAINS_KEY,
            id::STD_DICT_LEN,
            id::STD_DICT_REMOVE,
            id::STD_DICT_SETDEFAULT,
            id::STD_DICT_ITEMS,
            id::STD_DICT_KEYS,
            id::STD_DICT_VALUES,
        ]
        .contains(&id)
        {
            return self.dict_call(id, c, dst);
        }
        if self.actor_call(id, c, dst.as_ref())? {
            return Ok(());
        }
        if self.cluster_call(id, c, dst.as_ref())? {
            return Ok(());
        }
        if [
            id::STD_LIST_REVERSE,
            id::STD_LIST_SORT,
            id::STD_LIST_SORT_UNSTABLE,
            id::STD_PRELUDE_SORTED,
        ]
        .contains(&id)
        {
            return self.ordering_call(id, c, dst);
        }
        if DEQUE_IDS.contains(&id) {
            return self.deque_call(id, c, dst);
        }
        if ITERTOOLS_IDS.contains(&id) {
            return self.itertools_call(id, c, dst);
        }
        if [
            id::STD_PRELUDE_HEAP,
            id::STD_COLLECTIONS_HEAP_PUSH,
            id::STD_COLLECTIONS_HEAP_POP,
            id::STD_COLLECTIONS_HEAP_PEEK,
            id::STD_COLLECTIONS_HEAP_PUSHPOP,
            id::STD_COLLECTIONS_HEAP_INTO_SORTED,
            id::STD_COLLECTIONS_HEAP_LEN,
        ]
        .contains(&id)
        {
            return self.heap_call(id, c, dst);
        }
        match (id, args.as_slice()) {
            (id::STD_IO_PRINT, [(_, a)]) => {
                let s = self.scalar1(a)?;
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::Print,
                    args: vec![s],
                });
            }
            (id::STD_FS_READ, [(_, path)]) => {
                let Some(dst) = dst else {
                    return unsupported("`fs.read` without a result");
                };
                let Some((_, _, efs)) = dst.shape.variant("Err") else {
                    return unsupported("`fs.read` without its failure");
                };
                if !matches!(&efs[0].1, Shape::Rec(n, fs) if n == "FsError" && fs.len() == 2) {
                    return unsupported("`fs.read` with another failure type");
                }
                let path = self.scalar1(path)?;
                let p = self.rt(RtFn::FsRead, Ty::Ptr, vec![path.clone()]);
                let failed = self.scalar_op(ScalarOp::Eq, I64, vec![p.clone(), Operand::Int(0)]);
                let (e, o) = self.branch(failed);
                let join = self.block_new();
                self.cur = e;
                let empty = self.lit("");
                let at = self.rt(RtFn::Concat, Ty::Ptr, vec![path, empty]);
                let reason = self.rt(RtFn::FsReason, Ty::Ptr, vec![]);
                let err = Self::variant_value(&dst.shape, "Err", vec![at, reason]);
                self.copy_to(&dst.locals, err);
                self.goto(join);
                self.cur = o;
                let ok = Self::variant_value(&dst.shape, "Ok", vec![p]);
                self.copy_to(&dst.locals, ok);
                self.goto(join);
            }
            (id::STD_IO_READ_ALL, []) => {
                let r = self.rt(RtFn::ReadAll, Ty::Ptr, vec![]);
                set(self, r);
            }
            (id::STD_IO_READ_LINE, []) => {
                let Some(dst) = dst else {
                    return unsupported("`read_line` without a result");
                };
                let p = self.rt(RtFn::ReadLine, Ty::Ptr, vec![]);
                let eof = self.scalar_op(ScalarOp::Eq, I64, vec![p.clone(), Operand::Int(0)]);
                let (e, o) = self.branch(eof);
                let join = self.block_new();
                self.cur = e;
                let none = Self::variant_value(&dst.shape, "None", Vec::new());
                self.copy_to(&dst.locals, none);
                self.goto(join);
                self.cur = o;
                let some = Self::variant_value(&dst.shape, "Some", vec![p]);
                self.copy_to(&dst.locals, some);
                self.goto(join);
                self.cur = join;
            }
            (id::STD_TEXT_REPEAT, [(_, a), (_, n)]) => {
                let (a, n) = (self.scalar1(a)?, self.scalar1(n)?);
                let r = self.rt(RtFn::StrRepeat, Ty::Ptr, vec![a, n]);
                set(self, r);
            }
            (id::STD_TEXT_CONCAT, [(_, a), (_, b)]) => {
                let (a, b) = (self.scalar1(a)?, self.scalar1(b)?);
                let r = self.rt(RtFn::Concat, Ty::Ptr, vec![a, b]);
                set(self, r);
            }
            (id::STD_FMT_SHOW, [(_, a)]) => {
                let (v, s) = match self.targ(c)? {
                    Some(s) => (self.atom(a, &s)?, s),
                    None => self.atom_any(a)?,
                };
                let r = self.show(&v, &s, false)?;
                set(self, r);
            }
            (id::STD_JSON_DUMP, [(_, a), (_, schema)]) => {
                let Some(shape) = self.targ(c)? else {
                    return unsupported("`json.dump` without its type");
                };
                let v = self.atom(a, &shape)?;
                let schema = self.scalar1(schema)?;
                let buf = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(v.len() as i128)]);
                self.write(&At::Mem(buf.clone(), 0), v);
                let r = self.rt(RtFn::JsonDump, Ty::Ptr, vec![buf.clone(), schema]);
                self.free(RtFn::EnvFree, buf);
                set(self, r);
            }
            (id::STD_CHAN_CHANNEL, [(_, cap)]) => {
                let (Some(e), Some(dst)) = (self.targ(c)?, dst) else {
                    return unsupported("`Channel` without its type or result");
                };
                let cap = self.scalar1(cap)?;
                let words = Operand::Int(e.width() as i128);
                let ch = self.rt(RtFn::ChanNew, Ty::Ptr, vec![cap, words]);
                self.copy_to(&dst.locals, vec![ch.clone(), ch]);
            }
            (id::STD_CHAN_SENDER_SEND, [(_, tx), (_, a)]) => {
                let (Some(e), Some(dst)) = (self.targ(c)?, dst) else {
                    return unsupported("`send` without its type or result");
                };
                let tx = self.scalar1(tx)?;
                let v = self.atom(a, &e)?;
                let buf = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(v.len() as i128)]);
                self.write(&At::Mem(buf.clone(), 0), v);
                let st = self.blocking(RtFn::ChanSend, vec![tx, buf.clone()]);
                self.free(RtFn::EnvFree, buf);
                self.chan_result(&dst, st, |_| Vec::new())?;
            }
            (id::STD_CHAN_RECEIVER_RECV, [(_, rx)]) => {
                let (Some(e), Some(dst)) = (self.targ(c)?, dst) else {
                    return unsupported("`recv` without its type or result");
                };
                let rx = self.scalar1(rx)?;
                let out = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(e.width() as i128)]);
                let st = self.blocking(RtFn::ChanRecv, vec![rx, out.clone()]);
                self.chan_result(&dst, st, |me| me.read(&At::Mem(out.clone(), 0), &e))?;
                self.free(RtFn::EnvFree, out);
            }
            (id::STD_JSON_PARSE, [(_, a), (_, schema)]) => {
                let Some(dst) = dst else {
                    return unsupported("`json.parse` without a result");
                };
                let (src, schema) = (self.scalar1(a)?, self.scalar1(schema)?);
                let buf = self.rt(
                    RtFn::Alloc,
                    Ty::Ptr,
                    vec![Operand::Int(dst.locals.len() as i128)],
                );
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::JsonParse,
                    args: vec![buf.clone(), src, schema],
                });
                let v = self.read(&At::Mem(buf.clone(), 0), &dst.shape);
                self.copy_to(&dst.locals, v);
                self.free(RtFn::EnvFree, buf);
            }
            (id::STD_CSV_DUMP, [(_, rows), (_, delim), (_, schema)]) => {
                let (rows, delim, schema) = (
                    self.scalar1(rows)?,
                    self.scalar1(delim)?,
                    self.scalar1(schema)?,
                );
                let r = self.rt(RtFn::CsvDump, Ty::Ptr, vec![rows, delim, schema]);
                set(self, r);
            }
            (id::STD_CSV_PARSE, [(_, a), (_, delim), (_, schema)]) => {
                let Some(dst) = dst else {
                    return unsupported("`csv.parse` without a result");
                };
                let (src, delim, schema) = (
                    self.scalar1(a)?,
                    self.scalar1(delim)?,
                    self.scalar1(schema)?,
                );
                let buf = self.rt(
                    RtFn::Alloc,
                    Ty::Ptr,
                    vec![Operand::Int(dst.locals.len() as i128)],
                );
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::CsvParse,
                    args: vec![buf.clone(), src, delim, schema],
                });
                let v = self.read(&At::Mem(buf.clone(), 0), &dst.shape);
                self.copy_to(&dst.locals, v);
                self.free(RtFn::EnvFree, buf);
            }
            (id::STD_TEXT_CHAR_COUNT, [(_, a)]) => {
                let s = self.scalar1(a)?;
                let r = self.rt(RtFn::CharCount, I64, vec![s]);
                set(self, r);
            }
            (id::STD_TEXT_BYTE_LEN, [(_, a)]) => {
                let s = self.scalar1(a)?;
                let r = self.rt(RtFn::ByteLen, I64, vec![s]);
                set(self, r);
            }
            (
                id::STD_TEXT_SLICE_CLAMPED | id::STD_LIST_SLICE_CLAMPED,
                [(_, a), (_, b), (_, c), (_, d)],
            ) => {
                let (mut v, s) = self.atom_any(a)?;
                let mut args = vec![v.remove(0)];
                let opt = Shape::Sum(
                    "Option".into(),
                    vec![
                        ("Some".into(), vec![("value".into(), Shape::Scalar(I64))]),
                        ("None".into(), vec![]),
                    ],
                );
                for bound in [b, c, d] {
                    args.extend(self.atom(bound, &opt)?);
                }
                let r = match &s {
                    Shape::Str if id == id::STD_TEXT_SLICE_CLAMPED => {
                        self.rt(RtFn::StrSlice, Ty::Ptr, args)
                    }
                    Shape::List(e) if id == id::STD_LIST_SLICE_CLAMPED => {
                        let l = self.rt(RtFn::ListSlice, Ty::Ptr, args);
                        if Self::has_heap(e) {
                            let n = self.rt(RtFn::ListLen, I64, vec![l.clone()]);
                            self.each(n, |me, i| {
                                let at =
                                    At::Mem(me.rt(RtFn::ListAt, Ty::Ptr, vec![l.clone(), i]), 0);
                                let ev = me.read(&at, e);
                                let c = me.clone_value(ev, e)?;
                                me.write(&at, c);
                                Ok(())
                            })?;
                        }
                        l
                    }
                    _ => return unsupported("a slice of this type"),
                };
                set(self, r);
            }
            (id::STD_PRELUDE_INT, [(_, a)]) => {
                let x = self.scalar1(a)?;
                let r = self.rt(RtFn::TruncF64, I64, vec![x]);
                set(self, r);
            }
            (id::STD_PRELUDE_ROUND, [(_, a)]) => {
                let x = self.scalar1(a)?;
                let r = self.rt(RtFn::RoundF64, I64, vec![x]);
                set(self, r);
            }
            (id::STD_PRELUDE_FLOAT, [(_, a)]) => {
                let x = self.scalar1(a)?;
                let r = self.rt(RtFn::IntToF64, Ty::F64, vec![x]);
                set(self, r);
            }
            (id::STD_PRELUDE_ABS, [(_, a)]) => {
                let (v, s) = self.atom_any(a)?;
                let (f, ty) = match s {
                    Shape::Scalar(Ty::F64) => (RtFn::AbsF64, Ty::F64),
                    Shape::Scalar(t) if t == I64 => (RtFn::AbsI64, I64),
                    _ => return unsupported("`abs` of this type"),
                };
                let r = self.rt(f, ty, v);
                set(self, r);
            }
            (
                id::STD_MATH_FLOOR
                | id::STD_MATH_CEIL
                | id::STD_MATH_TRUNC
                | id::STD_MATH_NEXT_UP
                | id::STD_MATH_NEXT_DOWN,
                [(_, a)],
            ) => {
                let x = self.scalar1(a)?;
                let f = match id {
                    id::STD_MATH_FLOOR => RtFn::Floor,
                    id::STD_MATH_CEIL => RtFn::Ceil,
                    id::STD_MATH_TRUNC => RtFn::TruncF,
                    id::STD_MATH_NEXT_UP => RtFn::NextUp,
                    _ => RtFn::NextDown,
                };
                let r = self.rt(f, Ty::F64, vec![x]);
                set(self, r);
            }
            (id::STD_MATH_FMOD | id::STD_MATH_COPYSIGN, [(_, a), (_, b)]) => {
                let x = self.scalar1(a)?;
                let y = self.scalar1(b)?;
                let f = if id == id::STD_MATH_FMOD {
                    RtFn::Fmod
                } else {
                    RtFn::Copysign
                };
                let r = self.rt(f, Ty::F64, vec![x, y]);
                set(self, r);
            }
            // Two arguments: the default rounding, nearest-even. A directed mode is refused.
            (id::STD_MATH_POW, [(_, x), (_, y), (_, y::Atom::Name(round, _))])
                if round == "NearestEven" =>
            {
                let (x, y) = (self.scalar1(x)?, self.scalar1(y)?);
                let r = self.rt(RtFn::Pow, Ty::F64, vec![x, y]);
                set(self, r);
            }
            // Only the default rounding, nearest-even; a directed mode is refused like the interpreter.
            (id::STD_MATH_POWN, [(_, x), (_, n), (_, y::Atom::Name(round, _))])
                if round == "NearestEven" =>
            {
                let (x, n) = (self.scalar1(x)?, self.scalar1(n)?);
                let r = self.rt(RtFn::Pown, Ty::F64, vec![x, n]);
                set(self, r);
            }
            (id::STD_MATH_IS_NAN | id::STD_MATH_IS_INF | id::STD_MATH_IS_FINITE, [(_, a)]) => {
                let x = self.scalar1(a)?;
                let f = match id {
                    id::STD_MATH_IS_NAN => RtFn::IsNan,
                    id::STD_MATH_IS_INF => RtFn::IsInf,
                    _ => RtFn::IsFinite,
                };
                let r = self.rt(f, Ty::Bool, vec![x]);
                set(self, r);
            }
            (id::STD_TEXT_INT_OF, [(_, a)]) => {
                let s = self.scalar1(a)?;
                let r = self.rt(RtFn::IntOf, I64, vec![s]);
                set(self, r);
            }
            (id::STD_LIST_NEW, []) => {
                let Some(e) = self.targ(c)? else {
                    return unsupported("`std.list.new` without its element type");
                };
                let r = self.rt(
                    RtFn::ListNew,
                    Ty::Ptr,
                    vec![Operand::Int(e.width() as i128)],
                );
                set(self, r);
            }
            (id::STD_LIST_APPEND, [(_, y::Atom::Name(l, _)), (_, x)]) => {
                let list = self.name(l)?.clone();
                let Shape::List(e) = &list.shape else {
                    return unsupported(format!("call of `{name}`"));
                };
                let v = self.atom(x, e)?;
                let at = self.rt(RtFn::ListPush, Ty::Ptr, Self::ops(&list));
                self.write(&At::Mem(at, 0), v);
            }
            // An `iter` is its list, its cursor in the `limit` slot.
            (id::STD_LIST_ITER, [(conv, a)]) => {
                let (_, s) = self.atom_any(a)?;
                let v = self.owned(conv, a, &s)?;
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::DqLimit,
                    args: vec![v[0].clone(), Operand::Int(0)],
                });
                set(self, v[0].clone());
            }
            (id::STD_PRELUDE_SET, [(conv, a)]) => {
                let r = self.set_of(conv, a)?;
                set(self, r);
            }
            (id::STD_PRELUDE_LIST | id::STD_ITER_OPS_COLLECT, [(conv, a)]) => {
                let (_, s) = self.atom_any(a)?;
                if !matches!(s, Shape::List(_)) {
                    return unsupported("`list` of a non-list");
                }
                let v = self.owned(conv, a, &s)?;
                set(self, v[0].clone());
            }
            // The cursor of an `iter` from `std.itertools` is the list block's `limit` slot.
            (id::STD_ITER_OPS_DONE, [(_, a)]) => {
                let l = self.scalar1(a)?;
                let pos = self.rt(RtFn::DqMaxlen, I64, vec![l.clone()]);
                let n = self.rt(RtFn::ListLen, I64, vec![l]);
                let done = self.scalar_op(ScalarOp::Ge, I64, vec![pos, n]);
                set(self, done);
            }
            (id::STD_CMP_EQ | id::STD_CMP_NE, [(_, a), (_, b)]) => {
                let (x, sx) = self.atom_any(a)?;
                let (y, sy) = self.atom_any(b)?;
                // A `Node` is its id (R403).
                let node = cluster::std_type("Node");
                let text = |s: &Shape| *s == Shape::Str || Some(s) == node.as_ref();
                if sx != sy {
                    return unsupported(format!("call of `{name}` on this type"));
                }
                let mut r = if text(&sx) {
                    self.rt(RtFn::StrEq, Ty::Bool, vec![x[0].clone(), y[0].clone()])
                } else {
                    self.key_eq(&sx, x, y, &name)?
                };
                if id == id::STD_CMP_NE {
                    r = self.scalar_op(ScalarOp::Not, Ty::Bool, vec![r]);
                }
                set(self, r);
            }
            (id::STD_ITER_OPS_CURRENT, [(_, a)]) => {
                let (v, s) = self.atom_any(a)?;
                let Shape::List(e) = s else {
                    return unsupported("`current` of this iterator");
                };
                let l = v[0].clone();
                let pos = self.rt(RtFn::DqMaxlen, I64, vec![l.clone()]);
                let x = self.element(&l, pos.clone(), &e)?;
                let next = self.scalar_op(ScalarOp::AddChecked, I64, vec![pos, Operand::Int(1)]);
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::DqLimit,
                    args: vec![l, next],
                });
                if let Some(d) = &dst {
                    self.copy_to(&d.locals.clone(), x);
                }
            }
            (id::STD_LIST_LEN, [(_, a)]) => {
                let l = self.scalar1(a)?;
                let r = self.rt(RtFn::ListLen, I64, vec![l]);
                set(self, r);
            }
            _ => return unsupported(format!("call of `{name}`")),
        }
        Ok(())
    }
}
