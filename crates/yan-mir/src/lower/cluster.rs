//! `std.cluster` in native code (RFC 0063): the `Wire` round trip of every value that crosses
//! to a node (R402), over `yan_rt::wireslots`.

use super::*;

const WIRE_IDS: [registry::Id; 3] = [
    id::STD_CLUSTER_WIRE,
    id::STD_CLUSTER_WIRE_REPLY,
    id::STD_CLUSTER_WIRE_START,
];

impl Fn<'_, '_> {
    pub(super) fn cluster_call(
        &mut self,
        id: registry::Id,
        c: &y::Call,
        dst: Option<&Var>,
    ) -> R<bool> {
        if !WIRE_IDS.contains(&id) {
            return Ok(false);
        }
        let args = Self::value_args(c)?;
        let ([(_, x), (_, schema)], Some(shape), Some(dst)) = (args.as_slice(), self.targ(c)?, dst)
        else {
            return unsupported(format!("call of `{}`", c.path.join(".")));
        };
        let v = self.atom(x, &shape)?;
        let schema = self.scalar1(schema)?;
        let n = Operand::Int(v.len() as i128);
        let buf = self.rt(RtFn::Alloc, Ty::Ptr, vec![n.clone()]);
        self.write(&At::Mem(buf.clone(), 0), v);
        let out = self.rt(RtFn::Alloc, Ty::Ptr, vec![n]);
        let start = Operand::Int(i128::from(id == id::STD_CLUSTER_WIRE_START));
        let st = self.rt(
            RtFn::WireRoundtrip,
            I64,
            vec![out.clone(), buf.clone(), schema, start],
        );
        // The argument is borrowed: free the block, not the value.
        self.free(RtFn::EnvFree, buf);
        let back = self.read(&At::Mem(out.clone(), 0), &shape);
        self.free(RtFn::EnvFree, out);
        // Until native `admit` exists (#929 4b'), a rejected call argument or reply panics
        // instead of failing the call `SchemaMismatch` (R408).
        let bad = self.scalar_op(ScalarOp::Ne, I64, vec![st, Operand::Int(0)]);
        let (fail, good) = self.branch(bad);
        self.cur = fail;
        self.finish(Term::Panic("wire"));
        self.cur = good;
        self.copy_to(&dst.locals, back);
        Ok(true)
    }
}
