//! std.iter (R508 to R515): lazy pipelines that pull one item at a time from their end. Adapters
//! call closures, so this stage runs inside the VM.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt;
use std::rc::Rc;

use super::native::opt;
use super::prelude::cmp;
use super::vm::{Clo, Vm};
use indexmap::IndexMap;

use yong_std::registry::{Id, id::*};

use super::bind::bad_args;
use super::{Key, R, Value, panic, unsupported};

/// A shared, single-pass pipeline; copies of the value pull from the same state.
#[derive(Clone)]
pub(crate) struct It(Rc<RefCell<Stage>>);

impl PartialEq for It {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for It {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("<iter>")
    }
}

struct Stage {
    pipe: Pipe,
    /// The item `done` pulled and `current` hands to a `for` body.
    cur: Option<Value>,
}

type F = Rc<Clo>;

enum Pipe {
    Items(VecDeque<Value>),
    Count(i128, i128),
    Repeat(Value, Option<i128>),
    Map(It, F),
    Filter(It, F),
    TakeWhile(It, F, bool),
    DropWhile(It, F, bool),
    Take(It, i128),
    Skip(It, i128),
    StepBy(It, i128, bool),
    Zip(It, It),
    Enumerate(It, i128),
    FlatMap(It, F, VecDeque<Value>),
    Chain(It, It),
    Windows(It, usize, VecDeque<Value>),
    Chunks(It, usize),
}

fn it(p: Pipe) -> Value {
    Value::Iter(It(Rc::new(RefCell::new(Stage { pipe: p, cur: None }))))
}

fn items(xs: impl IntoIterator<Item = Value>) -> Value {
    it(Pipe::Items(xs.into_iter().collect()))
}

fn truthy(v: Value) -> bool {
    matches!(v, Value::Bool(true))
}

impl Vm<'_> {
    pub(crate) fn pull(&mut self, i: &It) -> R<Option<Value>> {
        let mut s = i.0.borrow_mut();
        Ok(match &mut s.pipe {
            Pipe::Items(xs) => xs.pop_front(),
            Pipe::Count(n, step) => {
                let v = *n;
                *n += *step;
                Some(Value::Int(v))
            }
            Pipe::Repeat(_, Some(0)) => None,
            Pipe::Repeat(x, n) => {
                if let Some(n) = n {
                    *n -= 1;
                }
                Some(x.clone())
            }
            Pipe::Map(src, f) => match self.pull(src)? {
                Some(x) => Some(self.apply(f, vec![x])?),
                None => None,
            },
            Pipe::Filter(src, f) => loop {
                match self.pull(src)? {
                    Some(x) if truthy(self.apply(f, vec![x.clone()])?) => break Some(x),
                    Some(_) => {}
                    None => break None,
                }
            },
            Pipe::TakeWhile(_, _, true) => None,
            Pipe::TakeWhile(src, f, done) => match self.pull(src)? {
                Some(x) if truthy(self.apply(f, vec![x.clone()])?) => Some(x),
                _ => {
                    *done = true;
                    None
                }
            },
            Pipe::DropWhile(src, f, dropping) => loop {
                match self.pull(src)? {
                    Some(x) if *dropping && truthy(self.apply(f, vec![x.clone()])?) => {}
                    x => {
                        *dropping = false;
                        break x;
                    }
                }
            },
            Pipe::Take(_, 0) => None,
            Pipe::Take(src, n) => {
                *n -= 1;
                self.pull(src)?
            }
            Pipe::Skip(src, n) => {
                while *n > 0 {
                    *n -= 1;
                    if self.pull(src)?.is_none() {
                        return Ok(None);
                    }
                }
                self.pull(src)?
            }
            Pipe::StepBy(src, n, first) => {
                if !*first {
                    for _ in 1..*n {
                        if self.pull(src)?.is_none() {
                            return Ok(None);
                        }
                    }
                }
                *first = false;
                self.pull(src)?
            }
            Pipe::Zip(a, b) => match self.pull(a)? {
                Some(x) => self.pull(b)?.map(|y| Value::Tuple(vec![x, y])),
                None => None,
            },
            Pipe::Enumerate(src, i) => {
                let n = *i;
                *i += 1;
                self.pull(src)?
                    .map(|x| Value::Tuple(vec![Value::Int(n), x]))
            }
            Pipe::FlatMap(src, f, buf) => loop {
                if let Some(x) = buf.pop_front() {
                    break Some(x);
                }
                match self.pull(src)? {
                    Some(x) => match self.apply(f, vec![x])? {
                        Value::List(ys) => buf.extend(ys),
                        v => return unsupported(format!("flat_map result {v:?}")),
                    },
                    None => break None,
                }
            },
            Pipe::Chain(a, b) => match self.pull(a)? {
                Some(x) => Some(x),
                None => self.pull(b)?,
            },
            Pipe::Windows(src, n, buf) => {
                while buf.len() < *n {
                    match self.pull(src)? {
                        Some(x) => buf.push_back(x),
                        None => return Ok(None),
                    }
                }
                let w = Value::List(buf.iter().cloned().collect());
                buf.pop_front();
                Some(w)
            }
            Pipe::Chunks(src, n) => {
                let mut c = Vec::new();
                while c.len() < *n {
                    match self.pull(src)? {
                        Some(x) => c.push(x),
                        None => break,
                    }
                }
                (!c.is_empty()).then_some(Value::List(c))
            }
        })
    }

    pub(crate) fn drain(&mut self, i: &It) -> R<Vec<Value>> {
        let mut out = Vec::new();
        while let Some(x) = self.pull(i)? {
            out.push(x);
        }
        Ok(out)
    }

    /// The std.iter functions and the `iter()` methods of the containers.
    pub(crate) fn iter_call(&mut self, id: Id, args: &mut [Value]) -> R<Value> {
        use Value::*;
        let size = |n: i128| -> R<usize> {
            if n < 1 {
                panic("value")
            } else {
                Ok(n as usize)
            }
        };
        Ok(match (id, &*args) {
            (STD_LIST_ITER, [List(xs)]) => items(xs.clone()),
            (STD_TEXT_ITER | STD_TEXT_CHARS, [Str(s)]) => {
                items(s.chars().map(Char).collect::<Vec<_>>())
            }
            (STD_SET_ITER, [Set(xs)]) => items(xs.iter().map(|k| k.0.clone())),
            (STD_DICT_ITER, [Dict(d)]) => items(d.keys().map(|k| k.0.clone())),
            (STD_COLLECTIONS_DEQUE_ITER, [Deque(xs, _)]) => items(xs.clone()),
            (STD_COLLECTIONS_SORTED_SET_ITER, [SortedSet(xs)]) => items(xs.clone()),
            (STD_COLLECTIONS_SORTED_DICT_ITER, [SortedMap(kv)]) => {
                items(kv.iter().map(|(k, _)| k.clone()))
            }
            (STD_ITER_COUNT, [Int(a), Int(s)]) => it(Pipe::Count(*a, *s)),
            (STD_ITER_REPEAT, [x, n]) => it(Pipe::Repeat(
                x.clone(),
                match n {
                    Data { fields, .. } => match fields.first() {
                        Some((_, Int(n))) => Some((*n).max(0)),
                        _ => None,
                    },
                    Int(n) => Some((*n).max(0)),
                    _ => None,
                },
            )),
            (STD_ITER_ONCE, [x]) => items([x.clone()]),
            (STD_PRELUDE_SET, [Iter(i)]) => {
                Set(self.drain(&i.clone())?.into_iter().map(Key).collect())
            }
            (STD_PRELUDE_DICT, [Iter(i)]) => {
                let mut d = IndexMap::default();
                for kv in self.drain(&i.clone())? {
                    if let Tuple(mut kv) = kv
                        && let (Some(v), Some(k)) = (kv.pop(), kv.pop())
                    {
                        d.insert(Key(k), v);
                    }
                }
                Dict(d)
            }
            (STD_ITER_EMPTY, []) => items([]),
            (_, [Iter(i), rest @ ..]) => {
                let i = i.clone();
                match (id, rest) {
                    (STD_ITER_OPS_MAP, [Closure(f)]) => it(Pipe::Map(i, f.clone())),
                    (STD_ITER_OPS_FILTER, [Closure(f)]) => it(Pipe::Filter(i, f.clone())),
                    (STD_ITER_OPS_TAKE_WHILE, [Closure(f)]) => {
                        it(Pipe::TakeWhile(i, f.clone(), false))
                    }
                    (STD_ITER_OPS_DROP_WHILE, [Closure(f)]) => {
                        it(Pipe::DropWhile(i, f.clone(), true))
                    }
                    (STD_ITER_OPS_TAKE, [Int(n)]) => it(Pipe::Take(i, (*n).max(0))),
                    (STD_ITER_OPS_SKIP, [Int(n)]) => it(Pipe::Skip(i, (*n).max(0))),
                    (STD_ITER_OPS_STEP_BY, [Int(n)]) => {
                        it(Pipe::StepBy(i, size(*n)? as i128, true))
                    }
                    (STD_ITER_OPS_ZIP, [Iter(b)]) => it(Pipe::Zip(i, b.clone())),
                    (STD_ITER_OPS_ENUMERATE, []) => it(Pipe::Enumerate(i, 0)),
                    (STD_ITER_OPS_FLAT_MAP, [Closure(f)]) => {
                        it(Pipe::FlatMap(i, f.clone(), VecDeque::new()))
                    }
                    (STD_ITER_OPS_CHAIN, [Iter(b)]) => it(Pipe::Chain(i, b.clone())),
                    (STD_ITER_OPS_WINDOWS, [Int(n)]) => {
                        it(Pipe::Windows(i, size(*n)?, VecDeque::new()))
                    }
                    (STD_ITER_OPS_CHUNKS, [Int(n)]) => it(Pipe::Chunks(i, size(*n)?)),
                    (STD_ITER_OPS_NEXT, []) => opt(self.pull(&i)?),
                    (STD_ITER_OPS_COLLECT, []) => List(self.drain(&i)?),
                    (STD_ITER_OPS_FOLD, [init, Closure(f)]) => {
                        let mut acc = init.clone();
                        while let Some(x) = self.pull(&i)? {
                            acc = self.apply(f, vec![acc, x])?;
                        }
                        acc
                    }
                    (STD_ITER_OPS_COUNT, []) => Int(self.drain(&i)?.len() as i128),
                    (STD_ITER_OPS_ANY, [Closure(f)]) => loop {
                        match self.pull(&i)? {
                            Some(x) if truthy(self.apply(f, vec![x.clone()])?) => {
                                break Bool(true);
                            }
                            Some(_) => {}
                            None => break Bool(false),
                        }
                    },
                    (STD_ITER_OPS_ALL, [Closure(f)]) => loop {
                        match self.pull(&i)? {
                            Some(x) if !truthy(self.apply(f, vec![x.clone()])?) => {
                                break Bool(false);
                            }
                            Some(_) => {}
                            None => break Bool(true),
                        }
                    },
                    (STD_ITER_OPS_FIND, [Closure(f)]) => loop {
                        match self.pull(&i)? {
                            Some(x) if truthy(self.apply(f, vec![x.clone()])?) => {
                                break opt(Some(x));
                            }
                            Some(_) => {}
                            None => break opt(None),
                        }
                    },
                    // The first of equal extremes, as Python's.
                    (STD_ITER_OPS_MIN | STD_ITER_OPS_MAX, [key]) => {
                        let key = key.clone();
                        let mut best: Option<(Value, Value)> = None;
                        while let Some(x) = self.pull(&i)? {
                            let k = self.key(&key, &x)?;
                            let o = best.as_ref().map(|(b, _)| cmp(&k, b));
                            let wins = match o {
                                None => true,
                                Some(o) if id == STD_ITER_OPS_MIN => o.is_lt(),
                                Some(o) => o.is_gt(),
                            };
                            if wins {
                                best = Some((k, x));
                            }
                        }
                        opt(best.map(|(_, x)| x))
                    }
                    (STD_ITER_OPS_SUM, []) => sum(self.drain(&i)?)?,
                    (STD_ITER_OPS_DONE, []) => {
                        let x = self.pull(&i)?;
                        let done = x.is_none();
                        i.0.borrow_mut().cur = x;
                        Bool(done)
                    }
                    (STD_ITER_OPS_CURRENT, []) => i.0.borrow_mut().cur.take().unwrap_or_default(),
                    _ => return bad_args(id),
                }
            }
            _ => return bad_args(id),
        })
    }
}

/// `sum` in the reduction order of 9.3 (R382). Integers are summed exactly and then range
/// checked, so whether it overflows does not depend on the order.
fn sum(xs: Vec<Value>) -> R<Value> {
    if xs.iter().all(|x| matches!(x, Value::Int(_))) {
        let t: i128 = xs
            .iter()
            .map(|x| match x {
                Value::Int(n) => *n,
                _ => 0,
            })
            .sum();
        return if (i64::MIN as i128..=i64::MAX as i128).contains(&t) {
            Ok(Value::Int(t))
        } else {
            panic("overflow")
        };
    }
    let fs: Vec<f64> = xs
        .iter()
        .map(|x| match x {
            Value::Float(f) => *f,
            _ => 0.0,
        })
        .collect();
    Ok(Value::Float(yong_rt::num::sum_f64(&fs)))
}

/// The ids `iter_call` handles.
pub(crate) const IDS: &[Id] = &[
    STD_COLLECTIONS_DEQUE_ITER,
    STD_COLLECTIONS_SORTED_DICT_ITER,
    STD_COLLECTIONS_SORTED_SET_ITER,
    STD_DICT_ITER,
    STD_ITER_COUNT,
    STD_ITER_EMPTY,
    STD_ITER_ONCE,
    STD_ITER_OPS_ALL,
    STD_ITER_OPS_ANY,
    STD_ITER_OPS_CHAIN,
    STD_ITER_OPS_CHUNKS,
    STD_ITER_OPS_COLLECT,
    STD_ITER_OPS_COUNT,
    STD_ITER_OPS_CURRENT,
    STD_ITER_OPS_DONE,
    STD_ITER_OPS_DROP_WHILE,
    STD_ITER_OPS_ENUMERATE,
    STD_ITER_OPS_FILTER,
    STD_ITER_OPS_FIND,
    STD_ITER_OPS_FLAT_MAP,
    STD_ITER_OPS_FOLD,
    STD_ITER_OPS_MAP,
    STD_ITER_OPS_MAX,
    STD_ITER_OPS_MIN,
    STD_ITER_OPS_NEXT,
    STD_ITER_OPS_SKIP,
    STD_ITER_OPS_STEP_BY,
    STD_ITER_OPS_SUM,
    STD_ITER_OPS_TAKE,
    STD_ITER_OPS_TAKE_WHILE,
    STD_ITER_OPS_WINDOWS,
    STD_ITER_OPS_ZIP,
    STD_ITER_REPEAT,
    STD_LIST_ITER,
    STD_PRELUDE_DICT,
    STD_PRELUDE_SET,
    STD_SET_ITER,
    STD_TEXT_CHARS,
    STD_TEXT_ITER,
];
