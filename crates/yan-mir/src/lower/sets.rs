//! `set` calls (17.3, R475): a set is a dict block whose values take no slots, so each call is
//! its `dict` counterpart with the value left out.

use super::*;

pub(super) const IDS: &[registry::Id] = &[
    id::STD_SET_NEW,
    id::STD_SET_ADD,
    id::STD_SET_DISCARD,
    id::STD_SET_CONTAINS,
];

impl<'m> Fn<'_, 'm> {
    pub(super) fn set_call(&mut self, id: registry::Id, c: &y::Call, dst: Option<Var>) -> R<()> {
        let args = Self::value_args(c)?;
        let set = |me: &mut Self, v: Vec<Operand>| {
            if let Some(d) = &dst {
                me.copy_to(&d.locals.clone(), v);
            }
        };
        if id == id::STD_SET_NEW {
            let [y::TArg::Type(k)] = c.targs.as_slice() else {
                return unsupported("`std.set.new` without its element type");
            };
            let k = self.shape(k)?;
            let mut schema = String::new();
            Self::key_schema(&k, &mut schema)?;
            let schema = self.lit(&schema);
            let r = self.rt(
                RtFn::MapNew,
                Ty::Ptr,
                vec![Operand::Int(k.width() as i128), Operand::Int(0), schema],
            );
            set(self, vec![r]);
            return Ok(());
        }
        let [(_, s), (kc, key)] = args.as_slice() else {
            return unsupported(format!("call of `{}`", c.path.join(".")));
        };
        let (mut m, Shape::Set(k)) = self.atom_any(s)? else {
            return unsupported("a `std.set` call on a non-set");
        };
        let m = m.remove(0);
        let k = *k;
        match id {
            id::STD_SET_ADD => {
                let key = self.owned(kc, key, &k)?;
                let buf = self.key_buf(key.clone());
                let p = self.rt(RtFn::MapFind, Ty::Ptr, vec![m.clone(), buf.clone()]);
                let missing = self.scalar_op(ScalarOp::Eq, I64, vec![p, Operand::Int(0)]);
                let (new, old) = self.branch(missing);
                let join = self.block_new();
                self.cur = new;
                self.rt(RtFn::MapPush, Ty::Ptr, vec![m, buf.clone()]);
                self.goto(join);
                self.cur = old;
                self.glue(&k, &key);
                self.goto(join);
                self.free(RtFn::EnvFree, buf);
            }
            id::STD_SET_CONTAINS => {
                let key = self.atom(key, &k)?;
                let buf = self.key_buf(key);
                let p = self.rt(RtFn::MapFind, Ty::Ptr, vec![m, buf.clone()]);
                self.free(RtFn::EnvFree, buf);
                let r = self.scalar_op(ScalarOp::Ne, I64, vec![p, Operand::Int(0)]);
                set(self, vec![r]);
            }
            _ => {
                let key = self.atom(key, &k)?;
                let buf = self.key_buf(key);
                let out = self.rt(RtFn::Alloc, Ty::Ptr, vec![Operand::Int(k.width() as i128)]);
                let r = self.rt(RtFn::MapRemove, I64, vec![m, buf.clone(), out.clone()]);
                self.free(RtFn::EnvFree, buf);
                let missing = self.scalar_op(ScalarOp::Eq, I64, vec![r, Operand::Int(0)]);
                let (none, some) = self.branch(missing);
                let join = self.block_new();
                self.cur = none;
                self.goto(join);
                self.cur = some;
                let kv = self.read(&At::Mem(out.clone(), 0), &k);
                self.glue(&k, &kv);
                self.goto(join);
                self.free(RtFn::EnvFree, out);
            }
        }
        Ok(())
    }
}
