//! `sorted_dict` and `sorted_set` (17.3, R488): a sorted dict is its keys and its values as two
//! lists in key order, a sorted set its items in order. Lookups bisect; updates shift the lists.

use super::*;

pub(super) const IDS: &[registry::Id] = &[
    id::STD_PRELUDE_SORTED_DICT,
    id::STD_PRELUDE_SORTED_SET,
    id::STD_COLLECTIONS_SORTED_DICT_INSERT,
    id::STD_COLLECTIONS_SORTED_DICT_INDEX,
    id::STD_COLLECTIONS_SORTED_DICT_GET,
    id::STD_COLLECTIONS_SORTED_DICT_CONTAINS_KEY,
    id::STD_COLLECTIONS_SORTED_DICT_LEN,
    id::STD_COLLECTIONS_SORTED_DICT_REMOVE,
    id::STD_COLLECTIONS_SORTED_DICT_SETDEFAULT,
    id::STD_COLLECTIONS_SORTED_DICT_ITEMS,
    id::STD_COLLECTIONS_SORTED_DICT_KEYS,
    id::STD_COLLECTIONS_SORTED_DICT_VALUES,
    id::STD_COLLECTIONS_SORTED_DICT_FIRST,
    id::STD_COLLECTIONS_SORTED_DICT_LAST,
    id::STD_COLLECTIONS_SORTED_DICT_POP_FIRST,
    id::STD_COLLECTIONS_SORTED_DICT_POP_LAST,
    id::STD_COLLECTIONS_SORTED_DICT_FLOOR,
    id::STD_COLLECTIONS_SORTED_DICT_CEILING,
    id::STD_COLLECTIONS_SORTED_DICT_RANGE,
    id::STD_COLLECTIONS_SORTED_DICT_ITER,
    id::STD_COLLECTIONS_SORTED_SET_ADD,
    id::STD_COLLECTIONS_SORTED_SET_DISCARD,
    id::STD_COLLECTIONS_SORTED_SET_CONTAINS,
    id::STD_COLLECTIONS_SORTED_SET_LEN,
    id::STD_COLLECTIONS_SORTED_SET_UNION,
    id::STD_COLLECTIONS_SORTED_SET_INTERSECTION,
    id::STD_COLLECTIONS_SORTED_SET_FIRST,
    id::STD_COLLECTIONS_SORTED_SET_LAST,
    id::STD_COLLECTIONS_SORTED_SET_POP_FIRST,
    id::STD_COLLECTIONS_SORTED_SET_POP_LAST,
    id::STD_COLLECTIONS_SORTED_SET_FLOOR,
    id::STD_COLLECTIONS_SORTED_SET_CEILING,
    id::STD_COLLECTIONS_SORTED_SET_RANGE,
    id::STD_COLLECTIONS_SORTED_SET_ITER,
];

pub(super) fn sorted_dict(k: Shape, v: Shape) -> Shape {
    Shape::Rec(
        "SortedMap".into(),
        vec![
            ("keys".into(), Shape::List(Box::new(k))),
            ("vals".into(), Shape::List(Box::new(v))),
        ],
    )
}

pub(super) fn sorted_set(t: Shape) -> Shape {
    Shape::Rec(
        "SortedSet".into(),
        vec![("items".into(), Shape::List(Box::new(t)))],
    )
}

/// The key shape and, for a dict, the value shape.
fn kinds(s: &Shape) -> Option<(Shape, Option<Shape>)> {
    match s {
        Shape::Rec(n, fs) if n == "SortedMap" => match fs.as_slice() {
            [(_, Shape::List(k)), (_, Shape::List(v))] => {
                Some(((**k).clone(), Some((**v).clone())))
            }
            _ => None,
        },
        Shape::Rec(n, fs) if n == "SortedSet" => match fs.as_slice() {
            [(_, Shape::List(t))] => Some(((**t).clone(), None)),
            _ => None,
        },
        _ => None,
    }
}

impl<'a, 'm> Fn<'a, 'm> {
    /// Where `x` is in the sorted list `l`: `(at, found)`; `at` is where it would go if absent.
    fn locate(
        &mut self,
        name: &str,
        l: &Operand,
        k: &Shape,
        x: &[Operand],
    ) -> R<(Operand, Operand)> {
        let at = self.bisect(name, l.clone(), k, x.to_vec(), false, None)?;
        let after = self.bisect(name, l.clone(), k, x.to_vec(), true, None)?;
        let found = self.scalar_op(ScalarOp::Ne, I64, vec![at.clone(), after]);
        Ok((at, found))
    }

    fn insert_at(&mut self, l: &Operand, i: &Operand, v: Vec<Operand>) {
        let slot = self.rt(RtFn::ListInsert, Ty::Ptr, vec![l.clone(), i.clone()]);
        self.write(&At::Mem(slot, 0), v);
    }

    /// Entry `i` of `l`, removed; the value is the caller's to drop.
    fn take_at(&mut self, l: &Operand, i: &Operand, e: &Shape) -> Vec<Operand> {
        let at = At::Mem(
            self.rt(RtFn::ListAt, Ty::Ptr, vec![l.clone(), i.clone()]),
            0,
        );
        let v = self.read(&at, e);
        self.emit(Inst::Rt {
            dst: None,
            f: RtFn::ListRemove,
            args: vec![l.clone(), i.clone()],
        });
        v
    }

    fn len_of(&mut self, l: &Operand) -> Operand {
        self.rt(RtFn::ListLen, I64, vec![l.clone()])
    }

    /// `(K, V)` of entry `i` of a dict's lists, or `T` of a set's, as copies.
    fn pair_at(
        &mut self,
        parts: &[Operand],
        i: &Operand,
        ks: &Shape,
        vs: &Option<Shape>,
    ) -> R<Vec<Operand>> {
        let mut out = self.element(&parts[0], i.clone(), ks)?;
        if let Some(vs) = vs {
            out.extend(self.element(&parts[1], i.clone(), vs)?);
        }
        Ok(out)
    }

    /// `Some(x)` when `ok`, else `None`, into `dst`; `x` is built in the `Some` branch.
    fn some_if(
        &mut self,
        ok: Operand,
        dst: &Option<Var>,
        ds: &Shape,
        mut build: impl FnMut(&mut Self) -> R<Vec<Operand>>,
    ) -> R<()> {
        let (yes, no) = self.branch(ok);
        let join = self.block_new();
        self.cur = no;
        if let Some(d) = dst {
            self.copy_to(&d.locals.clone(), Self::variant_value(ds, "None", vec![]));
        }
        self.goto(join);
        self.cur = yes;
        let x = build(self)?;
        match dst {
            Some(d) => self.copy_to(&d.locals.clone(), Self::variant_value(ds, "Some", x)),
            None => {
                if let Shape::Sum(_, vs) = ds {
                    let inner = vs[0].1[0].1.clone();
                    self.glue(&inner, &x);
                }
            }
        }
        self.goto(join);
        self.cur = join;
        Ok(())
    }

    pub(super) fn sorted_call(&mut self, id: registry::Id, c: &y::Call, dst: Option<Var>) -> R<()> {
        let name = c.path.join(".");
        let args = Self::value_args(c)?;
        let ds = dst.as_ref().map(|d| d.shape.clone());
        let is_set = name.contains("sorted_set");
        // Constructors have no container argument.
        if id == id::STD_PRELUDE_SORTED_DICT || id == id::STD_PRELUDE_SORTED_SET {
            let Some(d) = &dst else {
                if let [(_, a)] = args.as_slice() {
                    let (v, s) = self.atom_any(a)?;
                    self.glue(&s, &v);
                }
                return Ok(());
            };
            let Some((k, v)) = kinds(&d.shape) else {
                return unsupported(format!("call of `{name}`"));
            };
            let lists: Vec<Operand> = std::iter::once(&k)
                .chain(v.as_ref())
                .map(|s| {
                    self.rt(
                        RtFn::ListNew,
                        Ty::Ptr,
                        vec![Operand::Int(s.width() as i128)],
                    )
                })
                .collect();
            if let [(conv, a)] = args.as_slice() {
                if is_set {
                    let xs = self.owned(conv, a, &Shape::List(Box::new(k.clone())))?;
                    let n = self.len_of(&xs[0]);
                    self.each(n, |me, i| {
                        let x = me.element(&xs[0], i, &k)?;
                        let (at, found) = me.locate(&name, &lists[0], &k, &x)?;
                        let (yes, no) = me.branch(found);
                        let join = me.block_new();
                        me.cur = yes;
                        me.glue(&k, &x);
                        me.goto(join);
                        me.cur = no;
                        me.insert_at(&lists[0], &at, x);
                        me.goto(join);
                        me.cur = join;
                        Ok(())
                    })?;
                    self.glue(&Shape::List(Box::new(k.clone())), &xs);
                } else {
                    let v = v.clone().expect("a dict");
                    let (m, ms) = self.atom_any(a)?;
                    let Shape::Map(mk, me_) = &ms else {
                        return unsupported(format!("call of `{name}` on this type"));
                    };
                    let kw = mk.width() as u32;
                    let n = self.rt(RtFn::MapLen, I64, vec![m[0].clone()]);
                    self.each(n, |me, i| {
                        let at = me.rt(RtFn::MapAt, Ty::Ptr, vec![m[0].clone(), i]);
                        let kv = me.read(&At::Mem(at.clone(), 0), &k);
                        let kv = me.clone_value(kv, &k)?;
                        let vv = me.read(&At::Mem(at, kw), me_);
                        let vv = me.clone_value(vv, &v)?;
                        let (pos, _) = me.locate(&name, &lists[0], &k, &kv)?;
                        me.insert_at(&lists[0], &pos, kv);
                        me.insert_at(&lists[1], &pos, vv);
                        Ok(())
                    })?;
                    if matches!(conv, y::Conv::Sink) {
                        self.glue(&ms, &m);
                    }
                }
            }
            self.copy_to(&d.locals.clone(), lists);
            return Ok(());
        }
        let Some((first, _)) = args.first() else {
            return unsupported(format!("call of `{name}`"));
        };
        let _ = first;
        let (parts, shape) = self.atom_any(args[0].1)?;
        let Some((k, v)) = kinds(&shape) else {
            return unsupported(format!("call of `{name}` on this type"));
        };
        let keys = parts[0].clone();
        let rest: Vec<&y::Atom> = args[1..].iter().map(|(_, a)| *a).collect();
        let set = |me: &mut Self, r: Vec<Operand>| {
            if let Some(d) = &dst {
                me.copy_to(&d.locals.clone(), r);
            }
        };
        let list_of = |s: &Shape| Shape::List(Box::new(s.clone()));
        // Dict and set arms share their names' tails.
        let tail = name.rsplit('.').next().unwrap_or("").to_owned();
        match tail.as_str() {
            "len" => {
                let n = self.len_of(&keys);
                set(self, vec![n]);
            }
            "contains" | "contains_key" => {
                let x = self.atom(rest[0], &k)?;
                let (_, found) = self.locate(&name, &keys, &k, &x)?;
                set(self, vec![found]);
            }
            "insert" | "setdefault" | "add" => {
                let kx = self.atom(rest[0], &k)?;
                let vx = match &v {
                    Some(v) => Some(self.atom(rest[1], v)?),
                    None => None,
                };
                let (at, found) = self.locate(&name, &keys, &k, &kx)?;
                let (yes, no) = self.branch(found);
                let join = self.block_new();
                let set_default = tail == "setdefault";
                let out = match (&v, set_default) {
                    (Some(v), true) => Some(self.var(v.clone())),
                    _ => None,
                };
                self.cur = yes;
                if let (Some(v), Some(vx)) = (&v, &vx) {
                    if set_default {
                        let cur = self.element(&parts[1], at.clone(), v)?;
                        self.copy_to(&out.as_ref().expect("out").locals.clone(), cur);
                        self.glue(v, vx);
                    } else {
                        let slot =
                            self.rt(RtFn::ListAt, Ty::Ptr, vec![parts[1].clone(), at.clone()]);
                        let old = self.read(&At::Mem(slot.clone(), 0), v);
                        self.glue(v, &old);
                        self.write(&At::Mem(slot, 0), vx.clone());
                    }
                }
                self.glue(&k, &kx);
                self.goto(join);
                self.cur = no;
                self.insert_at(&keys, &at, kx);
                if let (Some(v), Some(vx)) = (&v, vx) {
                    if set_default {
                        let copy = self.clone_value(vx.clone(), v)?;
                        self.copy_to(&out.as_ref().expect("out").locals.clone(), copy);
                    }
                    self.insert_at(&parts[1], &at, vx);
                }
                self.goto(join);
                self.cur = join;
                if let Some(out) = out {
                    let ops = Self::ops(&out);
                    match &dst {
                        Some(d) => self.copy_to(&d.locals.clone(), ops),
                        None => self.glue(&out.shape, &ops),
                    }
                }
            }
            "index" | "get" => {
                let v = v.expect("a dict");
                let x = self.atom(rest[0], &k)?;
                let (at, found) = self.locate(&name, &keys, &k, &x)?;
                if tail == "index" {
                    let (yes, no) = self.branch(found);
                    self.cur = no;
                    self.finish(Term::Panic("bounds"));
                    self.cur = yes;
                    let cur = self.element(&parts[1], at, &v)?;
                    set(self, cur);
                } else {
                    let ds = ds.clone().expect("get has a result");
                    let vals = parts[1].clone();
                    self.some_if(found, &dst, &ds, |me| me.element(&vals, at.clone(), &v))?;
                }
            }
            "remove" | "discard" => {
                let x = self.atom(rest[0], &k)?;
                let (at, found) = self.locate(&name, &keys, &k, &x)?;
                let (yes, no) = self.branch(found);
                let join = self.block_new();
                self.cur = no;
                if let (Some(d), Some(ds)) = (&dst, &ds) {
                    self.copy_to(&d.locals.clone(), Self::variant_value(ds, "None", vec![]));
                }
                self.goto(join);
                self.cur = yes;
                let old = self.take_at(&keys, &at, &k);
                self.glue(&k, &old);
                if let Some(v) = &v {
                    let val = self.take_at(&parts[1], &at, v);
                    match (&dst, &ds) {
                        (Some(d), Some(ds)) => {
                            self.copy_to(&d.locals.clone(), Self::variant_value(ds, "Some", val));
                        }
                        _ => self.glue(v, &val),
                    }
                }
                self.goto(join);
                self.cur = join;
            }
            "keys" | "values" | "iter" => {
                let l = match (tail.as_str(), &v) {
                    ("values", Some(v)) => (parts[1].clone(), v.clone()),
                    _ => (parts[0].clone(), k.clone()),
                };
                if v.is_some() && tail == "iter" {
                    // A dict's `iter` yields its items.
                    let out = self.items(&parts, &k, &v)?;
                    set(self, vec![out]);
                } else {
                    let copy = self.clone_value(vec![l.0], &list_of(&l.1))?;
                    set(self, copy);
                }
            }
            "items" => {
                let out = self.items(&parts, &k, &v)?;
                set(self, vec![out]);
            }
            "first" | "last" | "pop_first" | "pop_last" => {
                let ds = ds.clone().expect("a result");
                let n = self.len_of(&keys);
                let ok = self.scalar_op(ScalarOp::Ge, I64, vec![n.clone(), Operand::Int(1)]);
                let last = tail.ends_with("last");
                let pop = tail.starts_with("pop");
                let (parts, v2, k2) = (parts.clone(), v.clone(), k.clone());
                self.some_if(ok, &dst, &ds, |me| {
                    let at = if last {
                        me.scalar_op(ScalarOp::AddChecked, I64, vec![n.clone(), Operand::Int(-1)])
                    } else {
                        Operand::Int(0)
                    };
                    if pop {
                        let mut out = me.take_at(&parts[0], &at, &k2);
                        if let Some(v2) = &v2 {
                            out.extend(me.take_at(&parts[1], &at, v2));
                        }
                        Ok(out)
                    } else {
                        me.pair_at(&parts, &at, &k2, &v2)
                    }
                })?;
            }
            "floor" | "ceiling" => {
                let ds = ds.clone().expect("a result");
                let x = self.atom(rest[0], &k)?;
                let floor = tail == "floor";
                let at = self.bisect(&name, keys.clone(), &k, x, floor, None)?;
                let (ok, idx) = if floor {
                    let ok = self.scalar_op(ScalarOp::Ge, I64, vec![at.clone(), Operand::Int(1)]);
                    (
                        ok,
                        self.scalar_op(ScalarOp::AddChecked, I64, vec![at, Operand::Int(-1)]),
                    )
                } else {
                    let n = self.len_of(&keys);
                    let past = self.scalar_op(ScalarOp::Ge, I64, vec![at.clone(), n]);
                    (self.scalar_op(ScalarOp::Not, Ty::Bool, vec![past]), at)
                };
                let (parts, v2, k2) = (parts.clone(), v.clone(), k.clone());
                self.some_if(ok, &dst, &ds, |me| me.pair_at(&parts, &idx, &k2, &v2))?;
            }
            "range" => {
                let lo = self.atom(rest[0], &k)?;
                let hi = self.atom(rest[1], &k)?;
                let i = self.bisect(&name, keys.clone(), &k, lo, false, None)?;
                let j = self.bisect(&name, keys.clone(), &k, hi, false, None)?;
                let d = self.scalar_op(ScalarOp::SubChecked, I64, vec![j, i.clone()]);
                let count = self.local(I64);
                self.emit(Inst::Copy {
                    dst: count,
                    src: d.clone(),
                });
                let neg = self.scalar_op(ScalarOp::Le, I64, vec![d, Operand::Int(0)]);
                let (zero, keep) = self.branch(neg);
                let join = self.block_new();
                self.cur = zero;
                self.emit(Inst::Copy {
                    dst: count,
                    src: Operand::Int(0),
                });
                self.goto(join);
                self.cur = keep;
                self.goto(join);
                self.cur = join;
                let words = k.width() + v.as_ref().map_or(0, Shape::width);
                let out = self.rt(RtFn::ListNew, Ty::Ptr, vec![Operand::Int(words as i128)]);
                self.each(Operand::Local(count), |me, t| {
                    let at = me.scalar_op(ScalarOp::AddChecked, I64, vec![i.clone(), t]);
                    let x = me.pair_at(&parts, &at, &k, &v)?;
                    let slot = me.rt(RtFn::ListPush, Ty::Ptr, vec![out.clone()]);
                    me.write(&At::Mem(slot, 0), x);
                    Ok(())
                })?;
                set(self, vec![out]);
            }
            "union" | "intersection" => {
                let (other, os) = self.atom_any(rest[0])?;
                if os != shape {
                    return unsupported(format!("call of `{name}` on this type"));
                }
                let union = tail == "union";
                let out = if union {
                    self.clone_value(vec![keys.clone()], &list_of(&k))?
                        .remove(0)
                } else {
                    self.rt(
                        RtFn::ListNew,
                        Ty::Ptr,
                        vec![Operand::Int(k.width() as i128)],
                    )
                };
                let (src, probe) = if union {
                    (other[0].clone(), out.clone())
                } else {
                    (keys.clone(), other[0].clone())
                };
                let n = self.len_of(&src);
                self.each(n, |me, i| {
                    let x = me.element(&src, i, &k)?;
                    let (at, found) = me.locate(&name, &probe, &k, &x)?;
                    let (yes, no) = me.branch(found);
                    let join = me.block_new();
                    me.cur = yes;
                    if union {
                        me.glue(&k, &x);
                    } else {
                        let slot = me.rt(RtFn::ListPush, Ty::Ptr, vec![out.clone()]);
                        me.write(&At::Mem(slot, 0), x.clone());
                    }
                    me.goto(join);
                    me.cur = no;
                    if union {
                        me.insert_at(&out, &at, x);
                    } else {
                        me.glue(&k, &x);
                    }
                    me.goto(join);
                    me.cur = join;
                    Ok(())
                })?;
                set(self, vec![out]);
            }
            _ => return unsupported(format!("call of `{name}`")),
        }
        Ok(())
    }

    /// A list of `(K, V)` pairs of a dict's entries, or `T`s of a set's, as copies.
    fn items(&mut self, parts: &[Operand], k: &Shape, v: &Option<Shape>) -> R<Operand> {
        let words = k.width() + v.as_ref().map_or(0, Shape::width);
        let out = self.rt(RtFn::ListNew, Ty::Ptr, vec![Operand::Int(words as i128)]);
        let n = self.len_of(&parts[0]);
        self.each(n, |me, i| {
            let x = me.pair_at(parts, &i, k, v)?;
            let slot = me.rt(RtFn::ListPush, Ty::Ptr, vec![out.clone()]);
            me.write(&At::Mem(slot, 0), x);
            Ok(())
        })?;
        Ok(out)
    }

    /// `sorted_dict({k: v, ...})` or `sorted_set({a, b})` for `show`.
    pub(super) fn show_sorted(&mut self, v: &[Operand], s: &Shape) -> R<Operand> {
        let Some((k, e)) = kinds(s) else {
            return unsupported("`show` of this type");
        };
        let n = self.len_of(&v[0]);
        let out = self.local(Ty::Ptr);
        let done = self.block_new();
        let head = if e.is_some() {
            "sorted_dict({"
        } else {
            "sorted_set({"
        };
        if e.is_none() {
            let empty = self.scalar_op(ScalarOp::Eq, I64, vec![n.clone(), Operand::Int(0)]);
            let (none, some) = self.branch(empty);
            self.cur = none;
            let lit = self.lit("sorted_set()");
            self.emit(Inst::Copy { dst: out, src: lit });
            self.goto(done);
            self.cur = some;
        }
        let open = self.lit(head);
        self.emit(Inst::Copy {
            dst: out,
            src: open,
        });
        let (keys, vals) = (v[0].clone(), v.get(1).cloned());
        self.each(n, |me, i| {
            let first = me.scalar_op(ScalarOp::Eq, I64, vec![i.clone(), Operand::Int(0)]);
            let (skip, sep) = me.branch(first);
            me.cur = sep;
            let comma = me.lit(", ");
            let r = me.concat2(Operand::Local(out), comma);
            me.emit(Inst::Copy { dst: out, src: r });
            me.goto(skip);
            let at = At::Mem(
                me.rt(RtFn::ListAt, Ty::Ptr, vec![keys.clone(), i.clone()]),
                0,
            );
            let kv = me.read(&at, &k);
            let ks = me.show(&kv, &k, true)?;
            let r = me.concat2(Operand::Local(out), ks);
            me.emit(Inst::Copy { dst: out, src: r });
            if let (Some(vals), Some(e)) = (&vals, &e) {
                let colon = me.lit(": ");
                let at = At::Mem(me.rt(RtFn::ListAt, Ty::Ptr, vec![vals.clone(), i]), 0);
                let ev = me.read(&at, e);
                let es = me.show(&ev, e, true)?;
                let r = me.concat(vec![Operand::Local(out), colon, es]);
                me.emit(Inst::Copy { dst: out, src: r });
            }
            Ok(())
        })?;
        let close = self.lit("})");
        let r = self.concat2(Operand::Local(out), close);
        self.emit(Inst::Copy { dst: out, src: r });
        self.goto(done);
        self.cur = done;
        Ok(Operand::Local(out))
    }
}
