//! `bigint` operations: blocks of two's-complement bytes handled by `yan_rt::bigint`.

use super::*;

/// Operation codes of `yan_rt::bigint`, which the runtime crate defines.
mod rt {
    pub const ADD: i64 = 0;
    pub const SUB: i64 = 1;
    pub const MUL: i64 = 2;
    pub const FLOORDIV: i64 = 3;
    pub const MOD: i64 = 4;
    pub const AND: i64 = 5;
    pub const OR: i64 = 6;
    pub const XOR: i64 = 7;
    pub const EQ: i64 = 0;
    pub const NE: i64 = 1;
    pub const LT: i64 = 2;
    pub const LE: i64 = 3;
    pub const GT: i64 = 4;
    pub const GE: i64 = 5;
}

impl<'a, 'm> Fn<'a, 'm> {
    /// Lowers the `<op>_bigint` operations, `bigint_pow` and `bigint(int)`; false for any other call.
    pub(super) fn big_call(&mut self, id: registry::Id, c: &y::Call, dst: Option<Var>) -> R<bool> {
        let name = c.path.join(".");
        let from_int = id == id::STD_PRELUDE_BIGINT;
        let Some(op) = (if from_int {
            Some("from_int")
        } else if name == "bigint_pow" {
            Some("pow")
        } else {
            name.strip_suffix("_bigint")
        }) else {
            return Ok(false);
        };
        let args = Self::value_args(c)?;
        let xs = args
            .iter()
            .map(|(_, a)| self.scalar1(a))
            .collect::<R<Vec<_>>>()?;
        let bin = |code: i64| (RtFn::BigBin, Ty::Ptr, code);
        let cmp = |code: i64| (RtFn::BigCmp, Ty::Bool, code);
        let (f, ty, lead): (RtFn, Ty, Option<i64>) = match op {
            "from_int" => (RtFn::BigFromInt, Ty::Ptr, None),
            "pow" => (RtFn::BigPow, Ty::Ptr, None),
            "not" => (RtFn::BigNot, Ty::Ptr, None),
            "shl" => (RtFn::BigShift, Ty::Ptr, Some(1)),
            "shr" => (RtFn::BigShift, Ty::Ptr, Some(0)),
            "count_ones" => (RtFn::BigBits, I64, Some(0)),
            "bit_width" => (RtFn::BigBits, I64, Some(1)),
            _ => {
                let (f, ty, code) = match op {
                    "add" => bin(rt::ADD),
                    "sub" => bin(rt::SUB),
                    "mul" => bin(rt::MUL),
                    "floordiv" => bin(rt::FLOORDIV),
                    "mod" => bin(rt::MOD),
                    "and" => bin(rt::AND),
                    "or" => bin(rt::OR),
                    "xor" => bin(rt::XOR),
                    "eq" => cmp(rt::EQ),
                    "ne" => cmp(rt::NE),
                    "lt" => cmp(rt::LT),
                    "le" => cmp(rt::LE),
                    "gt" => cmp(rt::GT),
                    "ge" => cmp(rt::GE),
                    _ => return unsupported(format!("call of `{name}`")),
                };
                (f, ty, Some(code))
            }
        };
        let mut ops = xs;
        if let Some(l) = lead {
            ops.insert(0, Operand::Int(l.into()));
        }
        let r = self.rt(f, ty, ops);
        match dst {
            Some(d) => self.copy_to(&d.locals.clone(), vec![r]),
            None if ty == Ty::Ptr => self.free(RtFn::BytesFree, r),
            None => {}
        }
        Ok(true)
    }
}
