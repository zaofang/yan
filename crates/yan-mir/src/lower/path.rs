//! `std.path` on `Path` (17.3, R587 to R591): runtime calls over the normalized text.

use super::*;

impl<'a, 'm> Fn<'a, 'm> {
    pub(super) fn path_call(&mut self, id: registry::Id, c: &y::Call, dst: Option<Var>) -> R<()> {
        let name = c.path.join(".");
        let args = Self::value_args(c)?;
        let mut blocks = Vec::new();
        for (_, a) in &args {
            blocks.push(self.atom_any(a)?.0[0].clone());
        }
        let set = |me: &mut Self, v: Operand| {
            if let Some(d) = &dst {
                me.copy_to(&d.locals.clone(), vec![v]);
            }
        };
        let b = |i: usize| blocks[i].clone();
        let op = |n: i128| Operand::Int(n);
        match (id, blocks.len()) {
            (id::STD_PRELUDE_PATH | id::STD_TEXT_AS_PATH, 1) => {
                let r = self.rt(RtFn::PathNew, Ty::Ptr, vec![b(0)]);
                set(self, r);
            }
            (id::STD_PATH_AS_PATH | id::STD_PATH_TEXT, 1) => {
                let empty = self.lit("");
                let r = self.rt(RtFn::Concat, Ty::Ptr, vec![b(0), empty]);
                set(self, r);
            }
            (
                id::STD_PATH_PARENT
                | id::STD_PATH_NAME
                | id::STD_PATH_SUFFIX
                | id::STD_PATH_STEM
                | id::STD_PATH_NORMALIZE,
                1,
            ) => {
                let k = match id {
                    id::STD_PATH_PARENT => 0,
                    id::STD_PATH_NAME => 1,
                    id::STD_PATH_SUFFIX => 2,
                    id::STD_PATH_STEM => 3,
                    _ => 4,
                };
                let r = self.rt(RtFn::PathOp, Ty::Ptr, vec![op(k), b(0)]);
                set(self, r);
            }
            (id::STD_PATH_WITH_NAME | id::STD_PATH_WITH_SUFFIX, 2) => {
                let k = i128::from(id == id::STD_PATH_WITH_SUFFIX);
                let r = self.rt(RtFn::PathWith, Ty::Ptr, vec![op(k), b(0), b(1)]);
                set(self, r);
            }
            (id::STD_PATH_JOIN, 2) => {
                let r = self.rt(RtFn::PathJoin, Ty::Ptr, vec![b(0), b(1)]);
                set(self, r);
            }
            (id::STD_PATH_IS_ABSOLUTE, 1) => {
                let r = self.rt(RtFn::PathIsAbsolute, Ty::Bool, vec![b(0)]);
                set(self, r);
            }
            (id::STD_PATH_STARTS_WITH, 2) => {
                let r = self.rt(RtFn::PathStartsWith, Ty::Bool, vec![b(0), b(1)]);
                set(self, r);
            }
            (id::STD_PATH_PARTS, 1) => {
                let r = self.rt(RtFn::PathParts, Ty::Ptr, vec![b(0)]);
                set(self, r);
            }
            (id::STD_PATH_RELATIVE_TO, 2) => {
                let Some(d) = &dst else {
                    return Ok(());
                };
                let ds = d.shape.clone();
                let r = self.rt(RtFn::PathRelativeTo, Ty::Ptr, vec![b(0), b(1)]);
                let none = self.scalar_op(ScalarOp::Eq, I64, vec![r.clone(), op(0)]);
                let (no, yes) = self.branch(none);
                let join = self.block_new();
                self.cur = no;
                self.copy_to(&d.locals.clone(), Self::variant_value(&ds, "None", vec![]));
                self.goto(join);
                self.cur = yes;
                self.copy_to(&d.locals.clone(), Self::variant_value(&ds, "Some", vec![r]));
                self.goto(join);
            }
            _ => return unsupported(format!("call of `{name}`")),
        }
        Ok(())
    }
}
