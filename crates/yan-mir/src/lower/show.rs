use super::*;

impl<'a, 'm> Fn<'a, 'm> {
    pub(super) fn concat(&mut self, parts: Vec<Operand>) -> Operand {
        let mut it = parts.into_iter();
        let first = it.next().expect("at least one part");
        it.fold(first, |acc, p| self.concat2(acc, p))
    }

    /// `a` and `b` are fresh blocks or literals, so the result replaces both.
    pub(super) fn concat2(&mut self, a: Operand, b: Operand) -> Operand {
        let r = self.rt(RtFn::Concat, Ty::Ptr, vec![a.clone(), b.clone()]);
        self.free(RtFn::StrFree, a);
        self.free(RtFn::StrFree, b);
        r
    }

    /// CPython's `str` of a value, or its `repr` inside a container (SQ20).
    pub(super) fn show(&mut self, v: &[Operand], s: &Shape, nested: bool) -> R<Operand> {
        Ok(match s {
            Shape::Scalar(Ty::Int(t)) => {
                let f = if t.signed {
                    RtFn::ShowI64
                } else {
                    RtFn::ShowU64
                };
                self.rt(f, Ty::Ptr, vec![v[0].clone()])
            }
            Shape::Scalar(Ty::F64) => self.rt(RtFn::ShowF64, Ty::Ptr, vec![v[0].clone()]),
            Shape::Scalar(Ty::Bool) => self.rt(RtFn::ShowBool, Ty::Ptr, vec![v[0].clone()]),
            Shape::Scalar(Ty::Unit) => self.lit("None"),
            Shape::Scalar(Ty::Ptr) => unreachable!("not a Yan Core type"),
            Shape::Fn(..) => self.lit("<function>"),
            Shape::Chan(_) => return unsupported("`show` of a channel end"),
            Shape::Actor(_) => return unsupported("`show` of an actor handle"),
            Shape::Set(k) => self.show_set(v[0].clone(), k)?,
            Shape::Rec(n, _) if n == "SortedMap" || n == "SortedSet" => self.show_sorted(v, s)?,
            Shape::Rec(name, _)
                if matches!(
                    name.as_str(),
                    "PlainDate"
                        | "PlainTime"
                        | "PlainDateTime"
                        | "Period"
                        | "ZonedDateTime"
                        | "TimeError"
                        | "MonoInstant"
                ) =>
            {
                let op = match name.as_str() {
                    "PlainDate" => "show.date",
                    "PlainTime" => "show.time",
                    "PlainDateTime" => "show.datetime",
                    "Period" => "show.period",
                    "ZonedDateTime" => "show.zoned",
                    "MonoInstant" => "show.mono",
                    _ => "show.error",
                };
                self.civil_show(op, v.to_vec())?
            }
            Shape::Rec(name, _) if name == "CliError" => {
                let m = self.clone_value(v[..1].to_vec(), &Shape::Str)?;
                m[0].clone()
            }
            Shape::Rec(name, _) if name == "Secret" => self.lit("Secret(***)"),
            // `Command(['echo', 'hi'])` and `<program>: <message>`.
            Shape::Rec(name, _) if name == "Command" => {
                let argv = self.argv(v)?;
                let list = Shape::List(Box::new(Shape::Str));
                let shown = self.show(std::slice::from_ref(&argv), &list, false)?;
                self.glue(&list, &[argv]);
                let (open, close) = (self.lit("Command("), self.lit(")"));
                self.concat(vec![open, shown, close])
            }
            Shape::Rec(name, _) if name == "ProcError" => {
                let p = self.clone_value(v[..1].to_vec(), &Shape::Str)?;
                let m = self.clone_value(v[v.len() - 1..].to_vec(), &Shape::Str)?;
                let sep = self.lit(": ");
                self.concat(vec![p[0].clone(), sep, m[0].clone()])
            }
            Shape::Rec(name, _) if name == "TimeZone" => self.rt(
                RtFn::TimeShow,
                Ty::Ptr,
                vec![Operand::Int(3), v[0].clone(), Operand::Int(0)],
            ),
            Shape::Rec(name, _) if matches!(name.as_str(), "Duration" | "Instant" | "i128") => {
                let instant = Operand::Int(match name.as_str() {
                    "Duration" => 0,
                    "Instant" => 1,
                    _ => 2,
                });
                self.rt(
                    RtFn::TimeShow,
                    Ty::Ptr,
                    vec![instant, v[0].clone(), v[1].clone()],
                )
            }
            Shape::BigInt => self.rt(RtFn::BigStr, Ty::Ptr, vec![v[0].clone()]),
            Shape::Bytes => self.rt(RtFn::BytesRepr, Ty::Ptr, vec![v[0].clone()]),
            Shape::ByteArray => {
                let r = self.rt(RtFn::BytesRepr, Ty::Ptr, vec![v[0].clone()]);
                let (open, close) = (self.lit("bytearray("), self.lit(")"));
                self.concat(vec![open, r, close])
            }
            // A path prints as its text, also inside a container.
            Shape::Char => {
                let s = self.rt(RtFn::CharStr, Ty::Ptr, vec![v[0].clone()]);
                if !nested {
                    return Ok(s);
                }
                let r = self.rt(RtFn::Repr, Ty::Ptr, vec![s.clone()]);
                self.free(RtFn::StrFree, s);
                r
            }
            Shape::Path | Shape::WinPath => {
                let empty = self.lit("");
                self.rt(RtFn::Concat, Ty::Ptr, vec![v[0].clone(), empty])
            }
            Shape::Str if nested => self.rt(RtFn::Repr, Ty::Ptr, vec![v[0].clone()]),
            Shape::Str => {
                let empty = self.lit("");
                self.rt(RtFn::Concat, Ty::Ptr, vec![v[0].clone(), empty])
            }
            Shape::Rec(name, _) if name == "EncodingError" => self.rt(
                RtFn::EncodingErrorShow,
                Ty::Ptr,
                vec![v[0].clone(), v[1].clone()],
            ),
            Shape::Rec(name, _) if name == "JsonError" => {
                let buf = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(v.len() as i128)]);
                self.write(&At::Mem(buf.clone(), 0), v.to_vec());
                let r = self.rt(RtFn::JsonErrorShow, Ty::Ptr, vec![buf.clone()]);
                self.free(RtFn::EnvFree, buf);
                r
            }
            Shape::Rec(name, _) if name == "CsvError" => {
                let buf = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(v.len() as i128)]);
                self.write(&At::Mem(buf.clone(), 0), v.to_vec());
                let r = self.rt(RtFn::CsvErrorShow, Ty::Ptr, vec![buf.clone()]);
                self.free(RtFn::EnvFree, buf);
                r
            }
            Shape::Rec(name, fs) => self.show_fields(name, v, fs)?,
            Shape::Tuple(ss) => {
                let mut parts = vec![self.lit("(")];
                let mut at = 0;
                for (i, e) in ss.iter().enumerate() {
                    if i > 0 {
                        parts.push(self.lit(", "));
                    }
                    let w = e.width();
                    parts.push(self.show(&v[at..at + w], e, true)?);
                    at += w;
                }
                parts.push(self.lit(if ss.len() == 1 { ",)" } else { ")" }));
                self.concat(parts)
            }
            Shape::Sum(name, vs) => {
                // A union shows as its member, as `Option` shows as its value.
                let option = name == "Option" || name == "Union";
                let out = self.local(Ty::Ptr);
                let join = self.block_new();
                for (n, fs) in vs {
                    let (tag, at, _) = s.variant(n).expect("its own variant");
                    let w: usize = fs.iter().map(|(_, s)| s.width()).sum();
                    let is =
                        self.scalar_op(ScalarOp::Eq, I64, vec![v[0].clone(), Operand::Int(tag)]);
                    let (t, e) = self.branch(is);
                    self.cur = t;
                    let r = match (option, fs.first()) {
                        (true, Some((_, inner))) => self.show(&v[at..at + w], inner, nested)?,
                        (true, None) => self.lit("None"),
                        _ => self.show_fields(n, &v[at..at + w], fs)?,
                    };
                    self.emit(Inst::Copy { dst: out, src: r });
                    self.finish(Term::Jump(join));
                    self.cur = e;
                }
                self.finish(Term::Panic("unreachable"));
                self.cur = join;
                Operand::Local(out)
            }
            Shape::Map(k, e) => self.show_map(v[0].clone(), k, e)?,
            Shape::Heap(e) => {
                let mut schema = String::new();
                Self::key_schema(e, &mut schema)?;
                let schema = self.lit(&schema);
                let l = self.rt(RtFn::ListCopy, Ty::Ptr, v.to_vec());
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::HeapSort,
                    args: vec![l.clone(), schema],
                });
                let inner = self.show(std::slice::from_ref(&l), &Shape::List(e.clone()), nested)?;
                self.free(RtFn::ListFree, l);
                let open = self.lit("heap(");
                let head = self.concat2(open, inner);
                let close = self.lit(")");
                self.concat2(head, close)
            }
            Shape::Deque(e) => {
                let l = self.rt(RtFn::ListCopy, Ty::Ptr, v.to_vec());
                let inner = self.show(std::slice::from_ref(&l), &Shape::List(e.clone()), nested)?;
                self.free(RtFn::ListFree, l);
                let open = self.lit("deque(");
                let head = self.concat2(open, inner);
                let suffix = self.rt(RtFn::DqSuffix, Ty::Ptr, v.to_vec());
                let head = self.concat2(head, suffix);
                let close = self.lit(")");
                self.concat2(head, close)
            }
            Shape::List(e) => {
                let list = v[0].clone();
                let out = self.local(Ty::Ptr);
                let open = self.lit("[");
                self.emit(Inst::Copy {
                    dst: out,
                    src: open,
                });
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
                let first =
                    self.scalar_op(ScalarOp::Eq, I64, vec![Operand::Local(i), Operand::Int(0)]);
                let (skip, sep) = self.branch(first);
                self.cur = sep;
                let comma = self.lit(", ");
                let r = self.concat2(Operand::Local(out), comma);
                self.emit(Inst::Copy { dst: out, src: r });
                self.goto(skip);
                let at = self.rt(RtFn::ListAt, Ty::Ptr, vec![list, Operand::Local(i)]);
                let ev = self.read(&At::Mem(at, 0), e);
                let es = self.show(&ev, e, true)?;
                let r = self.concat2(Operand::Local(out), es);
                self.emit(Inst::Copy { dst: out, src: r });
                let next = self.scalar_op(
                    ScalarOp::AddChecked,
                    I64,
                    vec![Operand::Local(i), Operand::Int(1)],
                );
                self.emit(Inst::Copy { dst: i, src: next });
                self.finish(Term::Jump(head));
                self.cur = exit;
                let close = self.lit("]");
                let r = self.concat2(Operand::Local(out), close);
                self.emit(Inst::Copy { dst: out, src: r });
                Operand::Local(out)
            }
        })
    }

    pub(super) fn show_map(&mut self, m: Operand, k: &Shape, e: &Shape) -> R<Operand> {
        let out = self.local(Ty::Ptr);
        let open = self.lit("{");
        self.emit(Inst::Copy {
            dst: out,
            src: open,
        });
        let n = self.rt(RtFn::MapLen, I64, vec![m.clone()]);
        self.each(n, |me, i| {
            let first = me.scalar_op(ScalarOp::Eq, I64, vec![i.clone(), Operand::Int(0)]);
            let (skip, sep) = me.branch(first);
            me.cur = sep;
            let comma = me.lit(", ");
            let r = me.concat2(Operand::Local(out), comma);
            me.emit(Inst::Copy { dst: out, src: r });
            me.goto(skip);
            let at = me.rt(RtFn::MapAt, Ty::Ptr, vec![m.clone(), i]);
            let kv = me.read(&At::Mem(at.clone(), 0), k);
            let ks = me.show(&kv, k, true)?;
            let colon = me.lit(": ");
            let ev = me.read(&At::Mem(at, k.width() as u32), e);
            let es = me.show(&ev, e, true)?;
            let r = me.concat(vec![Operand::Local(out), ks, colon, es]);
            me.emit(Inst::Copy { dst: out, src: r });
            Ok(())
        })?;
        let close = self.lit("}");
        Ok(self.concat2(Operand::Local(out), close))
    }

    /// `{a, b}` in insertion order, or `set()` when empty.
    fn show_set(&mut self, m: Operand, k: &Shape) -> R<Operand> {
        let n = self.rt(RtFn::MapLen, I64, vec![m.clone()]);
        let out = self.local(Ty::Ptr);
        let empty = self.scalar_op(ScalarOp::Eq, I64, vec![n.clone(), Operand::Int(0)]);
        let (none, some) = self.branch(empty);
        let join = self.block_new();
        self.cur = none;
        let lit = self.lit("set()");
        self.emit(Inst::Copy { dst: out, src: lit });
        self.goto(join);
        self.cur = some;
        let open = self.lit("{");
        self.emit(Inst::Copy {
            dst: out,
            src: open,
        });
        self.each(n, |me, i| {
            let first = me.scalar_op(ScalarOp::Eq, I64, vec![i.clone(), Operand::Int(0)]);
            let (skip, sep) = me.branch(first);
            me.cur = sep;
            let comma = me.lit(", ");
            let r = me.concat2(Operand::Local(out), comma);
            me.emit(Inst::Copy { dst: out, src: r });
            me.goto(skip);
            let at = me.rt(RtFn::MapAt, Ty::Ptr, vec![m.clone(), i]);
            let kv = me.read(&At::Mem(at, 0), k);
            let ks = me.show(&kv, k, true)?;
            let r = me.concat2(Operand::Local(out), ks);
            me.emit(Inst::Copy { dst: out, src: r });
            Ok(())
        })?;
        let close = self.lit("}");
        let r = self.concat2(Operand::Local(out), close);
        self.emit(Inst::Copy { dst: out, src: r });
        self.goto(join);
        Ok(Operand::Local(out))
    }

    /// `i` from 0 below `n`.
    pub(super) fn each(
        &mut self,
        n: Operand,
        mut body: impl FnMut(&mut Self, Operand) -> R<()>,
    ) -> R<()> {
        let i = self.local(I64);
        self.emit(Inst::Copy {
            dst: i,
            src: Operand::Int(0),
        });
        let head = self.block_new();
        self.goto(head);
        let done = self.scalar_op(ScalarOp::Ge, I64, vec![Operand::Local(i), n]);
        let (exit, b) = self.branch(done);
        self.cur = b;
        body(self, Operand::Local(i))?;
        let next = self.scalar_op(
            ScalarOp::AddChecked,
            I64,
            vec![Operand::Local(i), Operand::Int(1)],
        );
        self.emit(Inst::Copy { dst: i, src: next });
        self.finish(Term::Jump(head));
        self.cur = exit;
        Ok(())
    }

    /// A copy of `v` that owns its own blocks.
    pub(super) fn clone_value(&mut self, v: Vec<Operand>, s: &Shape) -> R<Vec<Operand>> {
        if !Self::has_heap(s) {
            return Ok(v);
        }
        Ok(match s {
            Shape::Scalar(_) | Shape::Char => v,
            Shape::Str | Shape::Path | Shape::WinPath => {
                let empty = self.lit("");
                vec![self.rt(RtFn::Concat, Ty::Ptr, vec![v[0].clone(), empty])]
            }
            Shape::Bytes | Shape::ByteArray | Shape::BigInt => {
                vec![self.rt(RtFn::BytesCopy, Ty::Ptr, v)]
            }
            Shape::List(e) | Shape::Heap(e) | Shape::Deque(e) => {
                let l = self.rt(RtFn::ListCopy, Ty::Ptr, v);
                if Self::has_heap(e) {
                    let n = self.rt(RtFn::ListLen, I64, vec![l.clone()]);
                    self.each(n, |me, i| {
                        let at = At::Mem(me.rt(RtFn::ListAt, Ty::Ptr, vec![l.clone(), i]), 0);
                        let ev = me.read(&at, e);
                        let c = me.clone_value(ev, e)?;
                        me.write(&at, c);
                        Ok(())
                    })?;
                }
                vec![l]
            }
            Shape::Set(k) => self.clone_value(v, &Self::set_map(k))?,
            Shape::Map(k, e) => {
                let m = self.rt(RtFn::MapCopy, Ty::Ptr, v);
                let n = self.rt(RtFn::MapLen, I64, vec![m.clone()]);
                self.each(n, |me, i| {
                    let p = me.rt(RtFn::MapAt, Ty::Ptr, vec![m.clone(), i]);
                    for (at, s) in [(0, k), (k.width() as u32, e)] {
                        if Self::has_heap(s) {
                            let at = At::Mem(p.clone(), at);
                            let x = me.read(&at, s);
                            let c = me.clone_value(x, s)?;
                            me.write(&at, c);
                        }
                    }
                    Ok(())
                })?;
                vec![m]
            }
            Shape::Rec(..) | Shape::Tuple(_) => {
                let mut out = Vec::with_capacity(v.len());
                for (_, at, fs) in s.fields().expect("an aggregate") {
                    out.extend(self.clone_value(v[at..at + fs.width()].to_vec(), fs)?);
                }
                out
            }
            Shape::Sum(_, vs) => {
                let out = self.var(s.clone());
                self.copy_to(&out.locals, v.clone());
                let Operand::Local(tag) = v[0] else {
                    return unsupported("a copy of a constant sum");
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
                        let c = self.clone_value(v[at..at + w].to_vec(), fs)?;
                        self.copy_to(&out.locals[at..at + w], c);
                        at += w;
                    }
                    self.finish(Term::Jump(join));
                }
                self.cur = join;
                Self::ops(&out)
            }
            Shape::Fn(..) => return unsupported("a copy of a closure"),
            Shape::Chan(_) => {
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::ChanClone,
                    args: v.clone(),
                });
                v
            }
            Shape::Actor(_) => return unsupported("a copy of an actor handle"),
        })
    }

    /// How `yan_rt::map` encodes a key of shape `s`.
    pub(super) fn key_schema(s: &Shape, out: &mut String) -> R<()> {
        match s {
            Shape::Scalar(Ty::Int(t)) => {
                out.push(if t.signed { 'i' } else { 'u' });
                out.push_str(&t.bits.to_string());
            }
            Shape::Scalar(Ty::F64) => out.push('f'),
            Shape::Scalar(Ty::Bool) => out.push('b'),
            Shape::Scalar(Ty::Unit) => out.push('n'),
            Shape::Str => out.push('s'),
            Shape::Bytes | Shape::ByteArray => out.push('y'),
            Shape::BigInt => out.push('g'),
            Shape::Path => out.push('p'),
            Shape::WinPath => out.push('w'),
            Shape::Char => out.push_str("u32"),
            Shape::Tuple(ss) => {
                out.push('(');
                for s in ss {
                    Self::key_schema(s, out)?;
                }
                out.push(')');
            }
            Shape::Rec(name, fs) => {
                out.push('{');
                out.push_str(name);
                out.push('|');
                for (_, s) in fs {
                    Self::key_schema(s, out)?;
                }
                out.push('}');
            }
            Shape::Sum(_, vs) => {
                out.push('<');
                for (n, fs) in vs {
                    out.push_str(n);
                    out.push('|');
                    for (_, s) in fs {
                        Self::key_schema(s, out)?;
                    }
                    out.push(';');
                }
                out.push('>');
            }
            _ => return unsupported("a dict key of this type"),
        }
        Ok(())
    }

    /// `key`'s slots in a fresh block, freed with `EnvFree`.
    pub(super) fn key_buf(&mut self, key: Vec<Operand>) -> Operand {
        let buf = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(key.len() as i128)]);
        self.write(&At::Mem(buf.clone(), 0), key);
        buf
    }

    /// An argument a `take` parameter owns: a borrowed one is copied.
    pub(super) fn owned(&mut self, conv: &y::Conv, a: &y::Atom, s: &Shape) -> R<Vec<Operand>> {
        let v = self.atom(a, s)?;
        if matches!(conv, y::Conv::Sink) || matches!(a, y::Atom::Lit(..)) {
            return Ok(v);
        }
        self.clone_value(v, s)
    }

    pub(super) fn map_arg(&mut self, a: &y::Atom) -> R<(Operand, Shape, Shape)> {
        let (mut v, s) = self.atom_any(a)?;
        let Shape::Map(k, e) = s else {
            return unsupported("a `std.dict` call on a non-dict");
        };
        Ok((v.remove(0), *k, *e))
    }
}
