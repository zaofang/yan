//! `std.ct`: selection and comparison on `int` without a data-dependent branch (R611).

use yan_std::registry::id::*;

use crate::Value::{Bool, Int};
use crate::bind::{Bind, bad_args};

macro_rules! f {
    ($id:ident, $pat:pat => $e:expr) => {
        (
            $id,
            Bind::Plain(|a| match &*a {
                $pat => Ok($e),
                _ => bad_args($id),
            }),
        )
    };
}

pub(crate) const BINDS: &[(yan_std::registry::Id, Bind)] = &[
    f!(STD_CT_CT_SELECT, [Bool(c), Int(a), Int(b)] => {
        let mask = -(*c as i128);
        Int(b ^ ((a ^ b) & mask))
    }),
    f!(STD_CT_CT_EQ, [Int(a), Int(b)] => Bool((a ^ b) == 0)),
    f!(STD_CT_CT_LT, [Int(a), Int(b)] => Bool((a.wrapping_sub(*b) >> 127) & 1 == 1 && a != b)),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn call(id: yan_std::registry::Id, args: &[crate::Value]) -> crate::Value {
        let (_, Bind::Plain(f)) = BINDS.iter().find(|(i, _)| *i == id).unwrap() else {
            unreachable!()
        };
        f(&mut args.to_vec()).unwrap()
    }

    #[test]
    fn select_eq_and_lt_agree_with_the_branching_forms() {
        let xs = [i64::MIN as i128, -7, -1, 0, 1, 7, i64::MAX as i128];
        for &a in &xs {
            assert_eq!(
                call(STD_CT_CT_SELECT, &[Bool(true), Int(a), Int(3)]),
                Int(a)
            );
            assert_eq!(
                call(STD_CT_CT_SELECT, &[Bool(false), Int(a), Int(3)]),
                Int(3)
            );
            for &b in &xs {
                assert_eq!(call(STD_CT_CT_EQ, &[Int(a), Int(b)]), Bool(a == b));
                assert_eq!(call(STD_CT_CT_LT, &[Int(a), Int(b)]), Bool(a < b));
            }
        }
    }
}
