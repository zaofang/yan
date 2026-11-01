//! The functions that take `key: (T) -> K` (R514): sorts, bisects, `insort`, `sorted`, `min` and
//! `max`. A key is a closure, so these run inside the VM. An omitted key is the identity.

use std::cmp::Ordering;

use super::prelude::cmp;
use super::vm::Vm;
use yong_std::registry::{Id, id::*};

use super::bind::bad_args;
use super::{R, Value};

impl Vm<'_> {
    /// `key(x)`, or `x` for an omitted key.
    pub(crate) fn key(&mut self, key: &Value, x: &Value) -> R<Value> {
        match key {
            Value::Closure(f) => self.apply(f, vec![x.clone()]),
            Value::Data { name, fields } if name == "Some" => match fields.first() {
                Some((_, Value::Closure(f))) => self.apply(&f.clone(), vec![x.clone()]),
                _ => Ok(x.clone()),
            },
            _ => Ok(x.clone()),
        }
    }

    /// Sorted by key, calling `key` once per element.
    fn sort_by_key(&mut self, xs: Vec<Value>, key: &Value, stable: bool) -> R<Vec<Value>> {
        let mut ks = Vec::with_capacity(xs.len());
        for x in &xs {
            ks.push(self.key(key, x)?);
        }
        let mut ix: Vec<usize> = (0..xs.len()).collect();
        if stable {
            ix.sort_by(|a, b| cmp(&ks[*a], &ks[*b]));
        } else {
            ix.sort_unstable_by(|a, b| cmp(&ks[*a], &ks[*b]));
        }
        let mut xs: Vec<Option<Value>> = xs.into_iter().map(Some).collect();
        Ok(ix.into_iter().filter_map(|i| xs[i].take()).collect())
    }

    /// The first index whose key is not before `x` (`right`: not at or before).
    fn bisect(&mut self, xs: &[Value], x: &Value, key: &Value, right: bool) -> R<usize> {
        let (mut lo, mut hi) = (0, xs.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            let o = cmp(&self.key(key, &xs[mid])?, x);
            if o == Ordering::Less || (right && o == Ordering::Equal) {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        Ok(lo)
    }

    /// `args` are the call's arguments in place; `var` ones are written back by the caller.
    pub(crate) fn keyed_call(&mut self, id: Id, args: &mut [Value]) -> R<Value> {
        use Value::*;
        let Some((key, args)) = args.split_last_mut() else {
            return bad_args(id);
        };
        let key = key.clone();
        Ok(match (id, args) {
            (STD_LIST_SORT | STD_LIST_SORT_UNSTABLE, [List(xs)]) => {
                *xs = self.sort_by_key(std::mem::take(xs), &key, id == STD_LIST_SORT)?;
                Unit
            }
            (STD_PRELUDE_SORTED, [Iter(i)]) => {
                let xs = self.drain(&i.clone())?;
                List(self.sort_by_key(xs, &key, true)?)
            }
            (STD_LIST_BISECT_LEFT | STD_LIST_BISECT_RIGHT, [List(xs), x]) => {
                Int(self.bisect(xs, x, &key, id == STD_LIST_BISECT_RIGHT)? as i128)
            }
            (STD_LIST_INSORT, [List(xs), x]) => {
                let k = self.key(&key, x)?;
                let at = self.bisect(xs, &k, &key, true)?;
                xs.insert(at, std::mem::take(x));
                Unit
            }
            (STD_PRELUDE_MIN | STD_PRELUDE_MAX, [a, b]) => {
                let o = cmp(&self.key(&key, b)?, &self.key(&key, a)?);
                let pick_b = if id == STD_PRELUDE_MIN {
                    o == Ordering::Less
                } else {
                    o == Ordering::Greater
                };
                if pick_b { b.clone() } else { a.clone() }
            }
            _ => return bad_args(id),
        })
    }
}

/// The ids `keyed_call` handles.
pub(crate) const IDS: &[Id] = &[
    STD_LIST_BISECT_LEFT,
    STD_LIST_BISECT_RIGHT,
    STD_LIST_INSORT,
    STD_LIST_SORT,
    STD_LIST_SORT_UNSTABLE,
    STD_PRELUDE_MAX,
    STD_PRELUDE_MIN,
    STD_PRELUDE_SORTED,
];
