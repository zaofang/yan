//! `std.sync.once` and `std.sync.lazy` (6.3): a cell holds its value once set. A cooperative task
//! never runs two initializers at once, so the first to run wins and a re-entrant one is a
//! deadlock (R396).

use yan_std::registry::{Id, id::*};

use super::bind::bad_args;
use super::vm::Vm;
use super::{R, Value, data, panic_with};

pub(crate) const IDS: &[Id] = &[
    STD_SYNC_ONCE_ONCE,
    STD_SYNC_ONCE_GET_OR_INIT,
    STD_SYNC_ONCE_GET,
    STD_SYNC_LAZY_LAZY,
    STD_SYNC_LAZY_FORCE,
];

fn cell(v: &Value) -> usize {
    match v {
        Value::Int(i) => *i as usize,
        _ => unreachable!("a Once or Lazy"),
    }
}

impl Vm<'_> {
    pub(crate) fn once_call(&mut self, id: Id, args: &mut [Value]) -> R<Value> {
        match (id, args) {
            (STD_SYNC_ONCE_ONCE, []) => {
                self.it.onces.push((None, None));
                Ok(Value::Int(self.it.onces.len() as i128 - 1))
            }
            (STD_SYNC_LAZY_LAZY, [f]) => {
                self.it.onces.push((None, Some(f.clone())));
                Ok(Value::Int(self.it.onces.len() as i128 - 1))
            }
            (STD_SYNC_ONCE_GET, [c]) => Ok(match &self.it.onces[cell(c)].0 {
                Some(v) => data("Some", "value", v.clone()),
                None => Value::Data {
                    name: "None".into(),
                    fields: Vec::new(),
                },
            }),
            (STD_SYNC_ONCE_GET_OR_INIT, [c, f]) => self.init(cell(c), Some(f.clone())),
            (STD_SYNC_LAZY_FORCE, [c]) => self.init(cell(c), None),
            _ => bad_args(id),
        }
    }

    fn init(&mut self, c: usize, f: Option<Value>) -> R<Value> {
        if let Some(v) = &self.it.onces[c].0 {
            return Ok(v.clone());
        }
        let f = match f {
            Some(f) => f,
            None => match self.it.onces[c].1.take() {
                Some(f) => f,
                None => {
                    return panic_with("deadlock", "a `Lazy` forced inside its own initializer");
                }
            },
        };
        let Value::Closure(clo) = f else {
            return bad_args(STD_SYNC_ONCE_GET_OR_INIT);
        };
        let v = self.apply(&clo, Vec::new())?;
        // A re-entrant `get_or_init` may have set the cell first; the first value stays.
        let slot = &mut self.it.onces[c].0;
        Ok(slot.get_or_insert(v).clone())
    }
}
