//! `std.regex` (17.3): a `Regex` is its pattern, a `Match` its five words (`yan_rt::regexabi`).

use super::*;

/// The variant of `s` that is not `None`, with its fields.
fn present(s: &Shape) -> Option<(String, Fields)> {
    let Shape::Sum(_, vs) = s else {
        return None;
    };
    let [(a, fa), (b, fb)] = vs.as_slice() else {
        return None;
    };
    match (a.as_str(), b.as_str()) {
        (_, "None") => Some((a.clone(), fa.clone())),
        ("None", _) => Some((b.clone(), fb.clone())),
        _ => None,
    }
}

impl<'a, 'm> Fn<'a, 'm> {
    pub(super) fn regex_call(&mut self, id: registry::Id, c: &y::Call, dst: Option<Var>) -> R<()> {
        let name = c.path.join(".");
        let args = Self::value_args(c)?;
        let mut v = Vec::new();
        for (_, a) in &args {
            v.push(self.atom_any(a)?.0);
        }
        let set = |me: &mut Self, r: Vec<Operand>| {
            if let Some(d) = &dst {
                me.copy_to(&d.locals.clone(), r);
            }
        };
        let op = |n: i128| Operand::Int(n);
        match (id, v.len()) {
            (id::STD_REGEX_ESCAPE, 1) => {
                let r = self.rt(RtFn::RegexEscape, Ty::Ptr, vec![v[0][0].clone()]);
                set(self, vec![r]);
            }
            (id::STD_REGEX_SPLIT, 2) => {
                let r = self.rt(
                    RtFn::RegexSplit,
                    Ty::Ptr,
                    vec![v[0][0].clone(), v[1][0].clone()],
                );
                set(self, vec![r]);
            }
            (id::STD_REGEX_SUB, 3) => {
                let a = vec![v[0][0].clone(), v[1][0].clone(), v[2][0].clone()];
                let r = self.rt(RtFn::RegexSub, Ty::Ptr, a);
                set(self, vec![r]);
            }
            (id::STD_REGEX_FIND_ALL, 2) => {
                let r = self.rt(
                    RtFn::RegexFindAll,
                    Ty::Ptr,
                    vec![v[0][0].clone(), v[1][0].clone()],
                );
                set(self, vec![r]);
            }
            (id::STD_REGEX_COMPILE, 1) => {
                let Some(d) = &dst else {
                    return Ok(());
                };
                let ds = d.shape.clone();
                let p = v[0][0].clone();
                // A literal pattern was checked at stage 0, so its call cannot fail.
                if ds.variant("Ok").is_none() {
                    let empty = self.lit("");
                    let pat = self.rt(RtFn::Concat, Ty::Ptr, vec![p, empty]);
                    self.copy_to(&d.locals.clone(), vec![pat]);
                    return Ok(());
                }
                let reason = self.rt(RtFn::RegexCheck, Ty::Ptr, vec![p.clone()]);
                let ok = self.scalar_op(ScalarOp::Eq, I64, vec![reason.clone(), op(0)]);
                let (good, bad) = self.branch(ok);
                let join = self.block_new();
                let empty = self.lit("");
                self.cur = good;
                let pat = self.rt(RtFn::Concat, Ty::Ptr, vec![p.clone(), empty.clone()]);
                self.copy_to(&d.locals.clone(), Self::variant_value(&ds, "Ok", vec![pat]));
                self.goto(join);
                self.cur = bad;
                let pat = self.rt(RtFn::Concat, Ty::Ptr, vec![p, empty]);
                self.copy_to(
                    &d.locals.clone(),
                    Self::variant_value(&ds, "Err", vec![pat, reason]),
                );
                self.goto(join);
            }
            (id::STD_REGEX_SEARCH | id::STD_REGEX_MATCH | id::STD_REGEX_FULLMATCH, 2) => {
                let Some(d) = &dst else {
                    return Ok(());
                };
                let ds = d.shape.clone();
                let Some((some, fields)) = present(&ds) else {
                    return unsupported(format!("call of `{name}`"));
                };
                let [(_, shape)] = fields.as_slice() else {
                    return unsupported(format!("call of `{name}`"));
                };
                let anchor = match id {
                    id::STD_REGEX_SEARCH => 0,
                    id::STD_REGEX_MATCH => 1,
                    _ => 2,
                };
                let buf = self.rt(RtFn::Alloc, Ty::Ptr, vec![op(5)]);
                let found = self.rt(
                    RtFn::RegexFind,
                    Ty::Bool,
                    vec![v[0][0].clone(), v[1][0].clone(), op(anchor), buf.clone()],
                );
                let (yes, no) = self.branch(found);
                let join = self.block_new();
                self.cur = yes;
                let m = self.read(&At::Mem(buf.clone(), 0), shape);
                self.copy_to(&d.locals.clone(), Self::variant_value(&ds, &some, m));
                self.goto(join);
                self.cur = no;
                self.copy_to(&d.locals.clone(), Self::variant_value(&ds, "None", vec![]));
                self.goto(join);
                self.free(RtFn::EnvFree, buf);
            }
            (id::STD_REGEX_GROUP | id::STD_REGEX_NAMED, 2) => {
                let Some(d) = &dst else {
                    return Ok(());
                };
                let ds = d.shape.clone();
                // The runtime's tags: 0 for the text, 1 for `None`.
                if ds.variant("None").map(|x| x.0) != Some(1) {
                    return unsupported(format!("call of `{name}`"));
                }
                let m = &v[0];
                let i = if id == id::STD_REGEX_GROUP {
                    v[1][0].clone()
                } else {
                    let key = self.key_buf(vec![v[1][0].clone()]);
                    let p = self.rt(RtFn::MapFind, Ty::Ptr, vec![m[4].clone(), key.clone()]);
                    self.free(RtFn::EnvFree, key);
                    let none = self.scalar_op(ScalarOp::Eq, I64, vec![p.clone(), op(0)]);
                    let (missing, present) = self.branch(none);
                    self.cur = missing;
                    self.finish(Term::Panic("bounds"));
                    self.cur = present;
                    self.read(&At::Mem(p, 0), &Shape::Scalar(I64)).remove(0)
                };
                let neg = self.scalar_op(ScalarOp::Le, I64, vec![i.clone(), op(-1)]);
                let (bad, ok) = self.branch(neg);
                self.cur = bad;
                self.finish(Term::Panic("bounds"));
                self.cur = ok;
                let at = self.rt(RtFn::ListAt, Ty::Ptr, vec![m[3].clone(), i]);
                let g = self.read(&At::Mem(at, 0), &ds);
                let g = self.clone_value(g, &ds)?;
                set(self, g);
            }
            _ => return unsupported(format!("call of `{name}`")),
        }
        Ok(())
    }
}
