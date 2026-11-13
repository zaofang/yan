//! `std.log`: events and spans through handler objects in `yan_rt::loghandler`.

use super::*;

impl<'a, 'm> Fn<'a, 'm> {
    /// A `Level` as its rank: a variant name, or the tag of a value.
    fn level(&mut self, a: &y::Atom) -> R<Operand> {
        if let y::Atom::Name(n, _) = a
            && !self.names.contains_key(n)
            && let Some(rank) = ["DebugLevel", "InfoLevel", "WarnLevel", "ErrorLevel"]
                .iter()
                .position(|v| v == n)
        {
            return Ok(Operand::Int(rank as i128));
        }
        Ok(self.atom_any(a)?.0[0].clone())
    }

    /// Lowers the `std.log` calls and the log handlers of `std.test`; false for any other call.
    pub(super) fn log_call(&mut self, id: registry::Id, c: &y::Call, dst: Option<Var>) -> R<bool> {
        let args = Self::value_args(c)?;
        // An unbound handler is the ambient default (0).
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
            (id::STD_LOG_EVENT, [(_, level), (_, msg), (_, fields)]) => {
                let h = handler(self);
                let level = self.level(level)?;
                let (msg, fields) = (self.scalar1(msg)?, self.scalar1(fields)?);
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::LogEvent,
                    args: vec![h, level, msg, fields],
                });
            }
            (id::STD_LOG_SPAN, [(_, name), (_, fields)]) => {
                let h = handler(self);
                let (name, fields) = (self.scalar1(name)?, self.scalar1(fields)?);
                let r = self.rt(RtFn::LogSpan, I64, vec![h, name, fields]);
                set(self, r);
            }
            (id::STD_TEST_LOG_STDERR | id::STD_TEST_LOG_STDOUT, [(_, level)]) => {
                let sink = i128::from(id == id::STD_TEST_LOG_STDOUT);
                let level = self.level(level)?;
                let r = self.rt(RtFn::LogNew, I64, vec![Operand::Int(sink), level]);
                set(self, r);
            }
            (id::STD_TEST_DISCARD_LOG, []) => {
                let r = self.rt(RtFn::LogNew, I64, vec![Operand::Int(2), Operand::Int(0)]);
                set(self, r);
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}
