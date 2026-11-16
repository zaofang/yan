//! `std.json.Value` (17.3, R569 to R575): parsing and dumping through `yan_rt::jsonslots`, and the
//! total accessors over the sum's slots.

use super::*;

pub(super) const JSON_VALUE_IDS: &[registry::Id] = &[
    id::STD_JSON_PARSE_VALUE,
    id::STD_JSON_PARSE_FLOAT,
    id::STD_JSON_DUMP_VALUE,
    id::STD_JSON_GET,
    id::STD_JSON_AT,
    id::STD_JSON_AS_BOOL,
    id::STD_JSON_AS_INT,
    id::STD_JSON_AS_FLOAT,
    id::STD_JSON_AS_STR,
    id::STD_JSON_AS_LIST,
    id::STD_JSON_AS_DICT,
    id::STD_JSON_IS_NULL,
];

impl<'m> Fn<'_, 'm> {
    fn std_named(&self, n: &str) -> R<Shape> {
        self.shape(&y::Type::Named(y::Named {
            path: vec![n.into()],
            args: Vec::new(),
        }))
    }

    /// The `Result` that `parse_value` (or `parse_float`) makes, for `?` on it.
    pub(super) fn json_parse_result(&self, float: bool) -> R<Shape> {
        let ok = if float {
            Shape::Scalar(Ty::F64)
        } else {
            self.std_named("Value")?
        };
        Ok(result(ok, self.std_named("JsonError")?))
    }

    pub(super) fn json_value_call(
        &mut self,
        id: registry::Id,
        c: &y::Call,
        dst: Option<Var>,
    ) -> R<()> {
        let args = Self::value_args(c)?;
        let value = self.std_named("Value")?;
        match (id, args.as_slice()) {
            (id::STD_JSON_PARSE_VALUE | id::STD_JSON_PARSE_FLOAT, [(_, s), (_, depth)]) => {
                let float = id == id::STD_JSON_PARSE_FLOAT;
                let unused = dst.is_none();
                let d = match dst {
                    Some(d) => d,
                    None => self.var(self.json_parse_result(float)?),
                };
                let (s, depth) = (self.scalar1(s)?, self.scalar1(depth)?);
                let w = Operand::Int(d.locals.len() as i128);
                let buf = self.rt(RtFn::Alloc, Ty::Ptr, vec![w]);
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::JsonParseValue,
                    args: vec![buf.clone(), s, depth, Operand::Int(i128::from(float))],
                });
                let v = self.read(&At::Mem(buf.clone(), 0), &d.shape);
                self.copy_to(&d.locals, v);
                self.free(RtFn::EnvFree, buf);
                if unused {
                    self.glue(&d.shape, &Self::ops(&d));
                }
            }
            (id::STD_JSON_DUMP_VALUE, [(_, v), (_, indent)]) => {
                let v = self.atom(v, &value)?;
                let (tag, n) = match indent {
                    y::Atom::Name(n, _) if n == "None" => (Operand::Int(1), Operand::Int(0)),
                    _ => match self.atom_any(indent)? {
                        (v, Shape::Scalar(_)) => (Operand::Int(0), v[0].clone()),
                        (v, Shape::Sum(_, vs)) if vs.len() == 2 => (v[0].clone(), v[1].clone()),
                        _ => return unsupported("`json.dump_value` with this indent"),
                    },
                };
                let buf = self.key_buf(v);
                let r = self.rt(RtFn::JsonDumpValue, Ty::Ptr, vec![buf.clone(), tag, n]);
                self.free(RtFn::EnvFree, buf);
                if let Some(d) = dst {
                    self.copy_to(&d.locals, vec![r]);
                }
            }
            (id::STD_JSON_IS_NULL, [(_, v)]) => {
                let v = self.atom(v, &value)?;
                let r = self.scalar_op(ScalarOp::Eq, I64, vec![v[0].clone(), Operand::Int(0)]);
                if let Some(d) = dst {
                    self.copy_to(&d.locals, vec![r]);
                }
            }
            (_, [(_, v), rest @ ..]) => {
                let Some(d) = dst else { return Ok(()) };
                let v = self.atom(v, &value)?;
                let arg = match rest {
                    [] => None,
                    [(_, a)] => Some(self.atom_any(a)?.0),
                    _ => return unsupported(format!("call of `{}`", c.path.join("."))),
                };
                self.json_access(id, &value, &v, arg, d)?;
            }
            _ => return unsupported(format!("call of `{}`", c.path.join("."))),
        }
        Ok(())
    }

    /// Sets `ok` to whether `lo <= x <= hi`, then goes to `join`.
    fn within(&mut self, ok: Local, x: Operand, lo: Operand, hi: Operand, join: BlockId) {
        let c = self.scalar_op(ScalarOp::Ge, I64, vec![x.clone(), lo]);
        let (yes, no) = self.branch(c);
        self.cur = no;
        self.emit(Inst::Copy {
            dst: ok,
            src: Operand::Bool(false),
        });
        self.goto(join);
        self.cur = yes;
        let r = self.scalar_op(ScalarOp::Le, I64, vec![x, hi]);
        self.emit(Inst::Copy { dst: ok, src: r });
        self.goto(join);
    }

    /// An accessor of R574 on the `Value` `v`, with its argument, into `d`.
    fn json_access(
        &mut self,
        id: registry::Id,
        value: &Shape,
        v: &[Operand],
        arg: Option<Vec<Operand>>,
        d: Var,
    ) -> R<()> {
        let dst = Some(d.clone());
        let field = |name: &str| {
            let (tag, at, fs) = value.variant(name).expect("a variant of `Value`");
            (tag, at, fs[0].1.clone())
        };
        let is = |me: &mut Self, tag: i128| {
            me.scalar_op(ScalarOp::Eq, I64, vec![v[0].clone(), Operand::Int(tag)])
        };
        let variant = match id {
            id::STD_JSON_AS_BOOL => "Bool",
            id::STD_JSON_AS_INT => "Int",
            id::STD_JSON_AS_STR => "Str",
            id::STD_JSON_AS_LIST | id::STD_JSON_AT => "Array",
            id::STD_JSON_AS_DICT | id::STD_JSON_GET => "Object",
            id::STD_JSON_AS_FLOAT => "Float",
            _ => return unsupported("this `json.Value` accessor"),
        };
        let (tag, at, s) = field(variant);
        let here = v[at..at + s.width()].to_vec();
        match (id, arg) {
            (id::STD_JSON_AT, Some(i)) => {
                let Shape::List(e) = &s else {
                    unreachable!("`Array` holds a list")
                };
                let e = (**e).clone();
                let (list, i) = (here[0].clone(), i[0].clone());
                let ok = self.local(Ty::Bool);
                self.emit(Inst::Copy {
                    dst: ok,
                    src: Operand::Bool(false),
                });
                let c = is(self, tag);
                let (arr, join) = self.branch(c);
                self.cur = arr;
                let n = self.rt(RtFn::ListLen, I64, vec![list.clone()]);
                let last = self.scalar_op(ScalarOp::SubChecked, I64, vec![n, Operand::Int(1)]);
                self.within(ok, i.clone(), Operand::Int(0), last, join);
                self.some_if(Operand::Local(ok), &dst, &d.shape, |me| {
                    let p = me.rt(RtFn::ListAt, Ty::Ptr, vec![list.clone(), i.clone()]);
                    let x = me.read(&At::Mem(p, 0), &e);
                    me.clone_value(x, &e)
                })
            }
            (id::STD_JSON_GET, Some(k)) => {
                let Shape::Map(_, e) = &s else {
                    unreachable!("`Object` holds a dict")
                };
                let e = (**e).clone();
                let p = self.local(Ty::Ptr);
                self.emit(Inst::Copy {
                    dst: p,
                    src: Operand::Int(0),
                });
                let c = is(self, tag);
                let (obj, join) = self.branch(c);
                self.cur = obj;
                let buf = self.key_buf(k);
                let found = self.rt(RtFn::MapFind, Ty::Ptr, vec![here[0].clone(), buf.clone()]);
                self.free(RtFn::EnvFree, buf);
                self.emit(Inst::Copy { dst: p, src: found });
                self.goto(join);
                let some =
                    self.scalar_op(ScalarOp::Ne, I64, vec![Operand::Local(p), Operand::Int(0)]);
                self.some_if(some, &dst, &d.shape, |me| {
                    let x = me.read(&At::Mem(Operand::Local(p), 0), &e);
                    me.clone_value(x, &e)
                })
            }
            // An `Int` widens exactly or gives `None` (R574).
            (id::STD_JSON_AS_FLOAT, None) => {
                let (itag, iat, _) = field("Int");
                let i = v[iat].clone();
                let f = self.local(Ty::F64);
                let ok = self.local(Ty::Bool);
                let c = is(self, tag);
                self.emit(Inst::Copy { dst: ok, src: c });
                self.emit(Inst::Copy {
                    dst: f,
                    src: here[0].clone(),
                });
                let c = is(self, itag);
                let (int, join) = self.branch(c);
                self.cur = int;
                let x = self.rt(RtFn::IntToF64, Ty::F64, vec![i.clone()]);
                self.emit(Inst::Copy { dst: f, src: x });
                let bound = 1 << 53;
                self.within(ok, i, Operand::Int(-bound), Operand::Int(bound), join);
                self.some_if(Operand::Local(ok), &dst, &d.shape, |_| {
                    Ok(vec![Operand::Local(f)])
                })
            }
            (_, None) => {
                let c = is(self, tag);
                self.some_if(c, &dst, &d.shape, |me| me.clone_value(here.clone(), &s))
            }
            _ => unsupported("this `json.Value` accessor"),
        }
    }
}
