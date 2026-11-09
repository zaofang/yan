//! Module constants (`let x: int = 1` at module level): each function binds them on entry, so a
//! later local of the same name shadows one as it does in the interpreter.

use super::*;

impl<'m> Fn<'_, 'm> {
    pub(super) fn bind_consts(&mut self) -> R<()> {
        let consts: Vec<&'m y::Const> = self.mono.borrow().consts.clone();
        for c in consts {
            let name = c.path.join(".");
            if self.names.contains_key(&name) {
                continue;
            }
            let shape = self.shape(&c.ty)?;
            let value = match (&c.value, &shape) {
                (y::ConstVal::Lit(y::Literal::Int(s)), Shape::Scalar(Ty::F64)) => {
                    vec![Operand::F64(parse_int(s)? as f64)]
                }
                (y::ConstVal::Lit(l), _) => vec![self.literal(l)?.0],
                _ => {
                    return unsupported(format!("module constant `{name}` of this form"));
                }
            };
            let var = self.var(shape);
            self.copy_to(&var.locals.clone(), value);
            self.names.insert(name, var);
        }
        Ok(())
    }
}
