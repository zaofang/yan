//! `char` and `str` character operations (17.3, R475): a `char` is its code point.

use super::*;

impl<'a, 'm> Fn<'a, 'm> {
    pub(super) fn chars_call(&mut self, id: registry::Id, c: &y::Call, dst: Option<Var>) -> R<()> {
        let name = c.path.join(".");
        let args = Self::value_args(c)?;
        let mut v = Vec::new();
        for (_, a) in &args {
            v.push(self.scalar1(a)?);
        }
        let set = |me: &mut Self, r: Operand| {
            if let Some(d) = &dst {
                me.copy_to(&d.locals.clone(), vec![r]);
            }
        };
        let op = |n: i128| Operand::Int(n);
        let r = match (id, v.len()) {
            (id::STD_TEXT_CHARS | id::STD_TEXT_ITER, 1) => {
                let l = self.rt(RtFn::StrChars, Ty::Ptr, vec![v[0].clone()]);
                // The cursor of an `iter` is the `limit` slot.
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::DqLimit,
                    args: vec![l.clone(), op(0)],
                });
                l
            }
            (id::STD_TEXT_CHAR_AT, 2) => self.rt(RtFn::CharAt, I64, v),
            (id::STD_PRELUDE_ORD, 1) => v[0].clone(),
            (id::STD_TEXT_COUNT, 2) => self.rt(RtFn::StrCount, I64, v),
            (
                id::STD_TEXT_IS_DIGIT
                | id::STD_TEXT_IS_ALPHA
                | id::STD_TEXT_IS_ALNUM
                | id::STD_TEXT_IS_UPPER
                | id::STD_TEXT_IS_LOWER
                | id::STD_TEXT_IS_SPACE,
                1,
            ) => {
                let k = match id {
                    id::STD_TEXT_IS_DIGIT => 0,
                    id::STD_TEXT_IS_ALPHA => 1,
                    id::STD_TEXT_IS_ALNUM => 2,
                    id::STD_TEXT_IS_UPPER => 3,
                    id::STD_TEXT_IS_LOWER => 4,
                    _ => 5,
                };
                self.rt(RtFn::CharPred, Ty::Bool, vec![op(k), v[0].clone()])
            }
            (
                id::STD_TEXT_ISDIGIT
                | id::STD_TEXT_ISALPHA
                | id::STD_TEXT_ISALNUM
                | id::STD_TEXT_ISUPPER
                | id::STD_TEXT_ISLOWER
                | id::STD_TEXT_ISSPACE,
                1,
            ) => {
                let k = match id {
                    id::STD_TEXT_ISDIGIT => 0,
                    id::STD_TEXT_ISALPHA => 1,
                    id::STD_TEXT_ISALNUM => 2,
                    id::STD_TEXT_ISUPPER => 3,
                    id::STD_TEXT_ISLOWER => 4,
                    _ => 5,
                };
                self.rt(RtFn::StrPred, Ty::Bool, vec![op(k), v[0].clone()])
            }
            _ => return unsupported(format!("call of `{name}`")),
        };
        set(self, r);
        Ok(())
    }
}
