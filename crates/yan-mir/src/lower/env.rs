//! `std.env` through handler objects in `yan_rt::envhandler`, and `TimeZone` as an interned ID.

use super::*;

impl<'a, 'm> Fn<'a, 'm> {
    /// Lowers the `std.env` calls, `fake_env` and the zone constructors; false for any other call.
    pub(super) fn env_call(&mut self, id: registry::Id, c: &y::Call, dst: Option<Var>) -> R<bool> {
        let args = Self::value_args(c)?;
        // An unbound handler is the host environment (0).
        let handler = |me: &Self| match c.args.iter().find_map(|a| match a {
            y::Arg::Cap(n, _) => me.names.get(n),
            y::Arg::Value(..) => None,
        }) {
            Some(v) => Self::ops(v)[0].clone(),
            None => Operand::Int(0),
        };
        let set = |me: &mut Self, v: Operand| {
            if let Some(d) = &dst {
                me.copy_to(&d.locals.clone(), vec![v]);
            }
        };
        match (id, args.as_slice()) {
            (id::STD_TEST_FAKE_ENV, [(c1, args), (c2, vars), (_, zone)]) => {
                let ((a, sa), (v, sv)) = (self.atom_any(args)?, self.atom_any(vars)?);
                let z = self.atom_any(zone)?.0;
                let h = self.rt(
                    RtFn::EnvFake,
                    I64,
                    vec![a[0].clone(), v[0].clone(), z[0].clone()],
                );
                if matches!(c1, y::Conv::Sink) {
                    self.glue(&sa, &a);
                }
                if matches!(c2, y::Conv::Sink) {
                    self.glue(&sv, &v);
                }
                set(self, h);
            }
            (id::STD_ENV_ONLY, [(conv, vars)]) => {
                let h = handler(self);
                let (v, s) = self.atom_any(vars)?;
                let r = self.rt(RtFn::EnvOnly, I64, vec![h, v[0].clone()]);
                if matches!(conv, y::Conv::Sink) {
                    self.glue(&s, &v);
                }
                set(self, r);
            }
            (id::STD_ENV_ARGS, []) => {
                let h = handler(self);
                let r = self.rt(RtFn::EnvArgs, Ty::Ptr, vec![h]);
                set(self, r);
            }
            (id::STD_ENV_VARS, []) => {
                let h = handler(self);
                let r = self.rt(RtFn::EnvVars, Ty::Ptr, vec![h]);
                set(self, r);
            }
            (id::STD_ENV_LOCAL_ZONE, []) => {
                let h = handler(self);
                let r = self.rt(RtFn::EnvZone, I64, vec![h]);
                set(self, r);
            }
            (id::STD_PRELUDE_SECRET, [(_, v)]) => {
                let (v, _) = self.atom_any(v)?;
                if let Some(d) = &dst {
                    self.copy_to(&d.locals.clone(), v);
                }
            }
            (id::STD_ENV_EXPOSE, [(_, s)]) => {
                let (v, shape) = self.atom_any(s)?;
                let Shape::Rec(_, fs) = &shape else {
                    return unsupported("`expose` of this value");
                };
                let inner = fs[0].1.clone();
                let out = self.clone_value(v, &inner)?;
                if let Some(d) = &dst {
                    self.copy_to(&d.locals.clone(), out);
                }
            }
            (id::STD_ENV_VAR | id::STD_ENV_SECRET, [(_, name)]) => {
                let Some(dst) = dst else {
                    return unsupported("`env.var` without a result");
                };
                let h = handler(self);
                let name = self.scalar1(name)?;
                let p = self.rt(RtFn::EnvVar, Ty::Ptr, vec![h, name]);
                let unset = self.scalar_op(ScalarOp::Eq, I64, vec![p.clone(), Operand::Int(0)]);
                let (e, o) = self.branch(unset);
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
            (id::STD_PRELUDE_TIMEZONE, [(_, id)]) => {
                let id = self.scalar1(id)?;
                let z = self.rt(RtFn::ZoneLookup, I64, vec![id]);
                let bad = self.scalar_op(ScalarOp::Eq, I64, vec![z.clone(), Operand::Int(0)]);
                let (e, o) = self.branch(bad);
                self.cur = e;
                self.finish(Term::Panic("value"));
                self.cur = o;
                set(self, z);
            }
            (id::STD_TIME_ZONE_UTC, []) => {
                let utc = self.lit("UTC");
                let z = self.rt(RtFn::ZoneLookup, I64, vec![utc]);
                set(self, z);
            }
            (id::STD_TIME_ZONE_ID, [(_, z)]) => {
                let z = self.atom_any(z)?.0[0].clone();
                let r = self.rt(RtFn::ZoneId, Ty::Ptr, vec![z]);
                set(self, r);
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}
