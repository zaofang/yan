//! `Duration` and `Instant`: 128-bit nanosecond counts held as the two words `hi`, `lo`, with
//! every operation in `yan_rt::timeabi`.

use super::*;

const U64: Ty = Ty::Int(IntTy {
    bits: 64,
    signed: false,
});

/// The shape of a `Duration` (`instant` false) or an `Instant`.
pub(super) fn shape(instant: bool) -> Shape {
    wide(if instant { "Instant" } else { "Duration" })
}

/// Two words, `hi` then `lo`: the shape of a 128-bit integer named `name`.
pub(super) fn wide(name: &str) -> Shape {
    Shape::Rec(
        name.into(),
        vec![
            ("hi".into(), Shape::Scalar(I64)),
            ("lo".into(), Shape::Scalar(U64)),
        ],
    )
}

/// What a time operation returns.
#[derive(Clone, Copy, PartialEq)]
enum Out {
    /// A time value; out of range is an overflow panic.
    Time,
    /// `Option` of a time value; out of range is `None`.
    Opt,
    /// An `int`; out of range is an overflow panic.
    Int,
    /// An `i128`.
    Wide,
    Float,
}

// Operation codes of `yan_rt::timeabi`.
const DADD: i64 = 0;
const DSUB: i64 = 1;
const DMUL: i64 = 2;
const DABS: i64 = 3;
const DNEG: i64 = 4;
const DFLOORDIV: i64 = 5;
const DREM: i64 = 6;
const DDIV: i64 = 7;
const DQUOT: i64 = 8;
const IADD: i64 = 9;
const ISUB: i64 = 10;
const ISINCE: i64 = 11;
const UNIT: i64 = 12;
const FROM_UNIX_NANOS: i64 = 13;
const FROM_UNIX_SECONDS: i64 = 14;
const UNIX_SECONDS: i64 = 15;
const NANOS: i64 = 16;
const SECONDS: i64 = 17;

/// The operation, what it returns, and the scale of a unit constructor.
fn op(id: registry::Id) -> Option<(i64, Out, i64)> {
    use Out::*;
    Some(match id {
        id::STD_TIME_DURATION_OF_NANOS => (UNIT, Time, 1),
        id::STD_TIME_DURATION_OF_MICROS => (UNIT, Time, 1_000),
        id::STD_TIME_DURATION_OF_MILLIS => (UNIT, Time, 1_000_000),
        id::STD_TIME_DURATION_OF_SECONDS => (UNIT, Time, 1_000_000_000),
        id::STD_TIME_DURATION_OF_MINUTES => (UNIT, Time, 60_000_000_000),
        id::STD_TIME_DURATION_OF_HOURS => (UNIT, Time, 3_600_000_000_000),
        id::STD_TIME_DURATION_NANOS | id::STD_TIME_INSTANT_UNIX_NANOS => (NANOS, Wide, 0),
        id::STD_TIME_DURATION_SECONDS => (SECONDS, Int, 0),
        id::STD_TIME_DURATION_AS_SECONDS_FLOAT => (0, Float, 0),
        id::STD_TIME_DURATION_RATIO => (1, Float, 0),
        id::STD_TIME_DURATION_ABS => (DABS, Time, 0),
        id::STD_TIME_DURATION_NEG => (DNEG, Time, 0),
        id::STD_TIME_DURATION_CHECKED_ADD => (DADD, Opt, 0),
        id::STD_TIME_DURATION_CHECKED_SUB => (DSUB, Opt, 0),
        id::STD_TIME_DURATION_CHECKED_MUL => (DMUL, Opt, 0),
        id::STD_TIME_DURATION_ADD => (DADD, Time, 0),
        id::STD_TIME_DURATION_SUB => (DSUB, Time, 0),
        id::STD_TIME_DURATION_MUL => (DMUL, Time, 0),
        id::STD_TIME_DURATION_FLOORDIV => (DFLOORDIV, Time, 0),
        id::STD_TIME_DURATION_REM => (DREM, Time, 0),
        id::STD_TIME_DURATION_DIV => (DDIV, Time, 0),
        id::STD_TIME_DURATION_QUOTIENT => (DQUOT, Int, 0),
        id::STD_TIME_INSTANT_FROM_UNIX_SECONDS => (FROM_UNIX_SECONDS, Time, 0),
        id::STD_TIME_INSTANT_FROM_UNIX_NANOS => (FROM_UNIX_NANOS, Time, 0),
        id::STD_TIME_INSTANT_UNIX_SECONDS => (UNIX_SECONDS, Int, 0),
        id::STD_TIME_INSTANT_ADD => (IADD, Time, 0),
        id::STD_TIME_INSTANT_SUB => (ISUB, Time, 0),
        id::STD_TIME_INSTANT_SINCE => (ISINCE, Time, 0),
        id::STD_TIME_INSTANT_CHECKED_ADD => (IADD, Opt, 0),
        id::STD_TIME_INSTANT_CHECKED_SUB => (ISUB, Opt, 0),
        _ => return None,
    })
}

impl<'a, 'm> Fn<'a, 'm> {
    /// An operand as its words `hi`, `lo`: a time value, or an `int` sign-extended.
    pub(super) fn time_words(&mut self, a: &y::Atom) -> R<Vec<Operand>> {
        let (v, s) = self.atom_any(a)?;
        Ok(match s {
            Shape::Scalar(_) => {
                let sign = self.scalar_op(ScalarOp::Shr, I64, vec![v[0].clone(), Operand::Int(63)]);
                vec![sign, v[0].clone()]
            }
            _ => v,
        })
    }

    /// Lowers the `Duration` and `Instant` operations; false for any other call.
    pub(super) fn time_call(&mut self, id: registry::Id, c: &y::Call, dst: Option<Var>) -> R<bool> {
        let Some((code, out, scale)) = op(id) else {
            return Ok(false);
        };
        let args = Self::value_args(c)?;
        let mut ops = vec![Operand::Int(code.into())];
        for (_, a) in &args {
            ops.extend(self.time_words(a)?);
        }
        // Unit constructors take their scale as the second operand; the rest pad to two.
        if scale != 0 {
            ops.extend([Operand::Int(0), Operand::Int(scale.into())]);
        }
        while ops.len() < 5 {
            ops.extend([Operand::Int(0), Operand::Int(0)]);
        }
        if out == Out::Float {
            let r = self.rt(RtFn::TimeF64, Ty::F64, ops);
            if let Some(d) = dst {
                self.copy_to(&d.locals, vec![r]);
            }
            return Ok(true);
        }
        let p = self.rt(RtFn::TimeOp, Ty::Ptr, ops);
        let words = self.read(&At::Mem(p.clone(), 0), &shape(false));
        let ok = self.read(&At::Mem(p, 2), &Shape::Scalar(I64)).remove(0);
        let Some(d) = dst else {
            return Ok(true);
        };
        // The one-word result of an `int` operation is its `lo` word.
        let value = if out == Out::Int {
            vec![words[1].clone()]
        } else {
            words
        };
        if out == Out::Opt {
            let (some, none) = self.branch(ok);
            let join = self.block_new();
            self.cur = some;
            self.copy_to(&d.locals, Self::variant_value(&d.shape, "Some", value));
            self.goto(join);
            self.cur = none;
            self.copy_to(&d.locals, Self::variant_value(&d.shape, "None", vec![]));
            self.goto(join);
            self.cur = join;
        } else {
            let (good, bad) = self.branch(ok);
            self.cur = bad;
            self.finish(Term::Panic("overflow"));
            self.cur = good;
            self.copy_to(&d.locals, value);
        }
        Ok(true)
    }
}
