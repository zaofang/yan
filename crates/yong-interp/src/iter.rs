//! std.iter (R508 to R515): lazy pipelines that pull one item at a time from their end. Adapters
//! call closures, so this stage runs inside the VM.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt;
use std::rc::Rc;

use super::native::opt;
use super::prelude::cmp;
use super::vm::{Clo, Vm};
use super::{R, Value, panic, unsupported};

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
    fn pull(&mut self, i: &It) -> R<Option<Value>> {
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

    fn drain(&mut self, i: &It) -> R<Vec<Value>> {
        let mut out = Vec::new();
        while let Some(x) = self.pull(i)? {
            out.push(x);
        }
        Ok(out)
    }

    /// The std.iter functions and the `iter()` methods of the containers; `None` for other names.
    pub(crate) fn iter_call(&mut self, name: &str, args: Vec<Value>) -> Option<R<Value>> {
        use Value::*;
        let r = (|| -> R<Option<Value>> {
            let size = |n: i128| -> R<usize> {
                if n < 1 {
                    panic("value")
                } else {
                    Ok(n as usize)
                }
            };
            Ok(Some(match (name, args.as_slice()) {
                ("std.list.iter", [List(xs)]) => items(xs.clone()),
                ("std.set.iter", [Set(xs)]) => items(xs.iter().map(|k| k.0.clone())),
                ("std.dict.iter", [Dict(d)]) => items(d.keys().map(|k| k.0.clone())),
                ("std.collections.deque.iter", [Deque(xs, _)]) => items(xs.clone()),
                ("std.collections.sorted_set.iter", [SortedSet(xs)]) => items(xs.clone()),
                ("std.collections.sorted_dict.iter", [SortedMap(kv)]) => {
                    items(kv.iter().map(|(k, _)| k.clone()))
                }
                ("std.iter.count", [Int(a), Int(s)]) => it(Pipe::Count(*a, *s)),
                ("std.iter.repeat", [x, n]) => it(Pipe::Repeat(
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
                ("std.iter.once", [x]) => items([x.clone()]),
                ("std.iter.empty", []) => items([]),
                (_, [Iter(i), rest @ ..]) => {
                    let i = i.clone();
                    match (name, rest) {
                        ("std.iter.ops.map", [Closure(f)]) => it(Pipe::Map(i, f.clone())),
                        ("std.iter.ops.filter", [Closure(f)]) => it(Pipe::Filter(i, f.clone())),
                        ("std.iter.ops.take_while", [Closure(f)]) => {
                            it(Pipe::TakeWhile(i, f.clone(), false))
                        }
                        ("std.iter.ops.drop_while", [Closure(f)]) => {
                            it(Pipe::DropWhile(i, f.clone(), true))
                        }
                        ("std.iter.ops.take", [Int(n)]) => it(Pipe::Take(i, (*n).max(0))),
                        ("std.iter.ops.skip", [Int(n)]) => it(Pipe::Skip(i, (*n).max(0))),
                        ("std.iter.ops.step_by", [Int(n)]) => {
                            it(Pipe::StepBy(i, size(*n)? as i128, true))
                        }
                        ("std.iter.ops.zip", [Iter(b)]) => it(Pipe::Zip(i, b.clone())),
                        ("std.iter.ops.enumerate", []) => it(Pipe::Enumerate(i, 0)),
                        ("std.iter.ops.flat_map", [Closure(f)]) => {
                            it(Pipe::FlatMap(i, f.clone(), VecDeque::new()))
                        }
                        ("std.iter.ops.chain", [Iter(b)]) => it(Pipe::Chain(i, b.clone())),
                        ("std.iter.ops.windows", [Int(n)]) => {
                            it(Pipe::Windows(i, size(*n)?, VecDeque::new()))
                        }
                        ("std.iter.ops.chunks", [Int(n)]) => it(Pipe::Chunks(i, size(*n)?)),
                        ("std.iter.ops.next", []) => opt(self.pull(&i)?),
                        ("std.iter.ops.collect", []) => List(self.drain(&i)?),
                        ("std.iter.ops.fold", [init, Closure(f)]) => {
                            let mut acc = init.clone();
                            while let Some(x) = self.pull(&i)? {
                                acc = self.apply(f, vec![acc, x])?;
                            }
                            acc
                        }
                        ("std.iter.ops.count", []) => Int(self.drain(&i)?.len() as i128),
                        ("std.iter.ops.any", [Closure(f)]) => loop {
                            match self.pull(&i)? {
                                Some(x) if truthy(self.apply(f, vec![x.clone()])?) => {
                                    break Bool(true);
                                }
                                Some(_) => {}
                                None => break Bool(false),
                            }
                        },
                        ("std.iter.ops.all", [Closure(f)]) => loop {
                            match self.pull(&i)? {
                                Some(x) if !truthy(self.apply(f, vec![x.clone()])?) => {
                                    break Bool(false);
                                }
                                Some(_) => {}
                                None => break Bool(true),
                            }
                        },
                        ("std.iter.ops.find", [Closure(f)]) => loop {
                            match self.pull(&i)? {
                                Some(x) if truthy(self.apply(f, vec![x.clone()])?) => {
                                    break opt(Some(x));
                                }
                                Some(_) => {}
                                None => break opt(None),
                            }
                        },
                        ("std.iter.ops.min", []) => {
                            opt(self.drain(&i)?.into_iter().min_by(cmp))
                        }
                        // The first of equal maxima, as Python's max.
                        ("std.iter.ops.max", []) => {
                            opt(self.drain(&i)?.into_iter().rev().max_by(cmp))
                        }
                        ("std.iter.ops.sum", []) => sum(self.drain(&i)?)?,
                        ("std.iter.ops.done", []) => {
                            let x = self.pull(&i)?;
                            let done = x.is_none();
                            i.0.borrow_mut().cur = x;
                            Bool(done)
                        }
                        ("std.iter.ops.current", []) => {
                            i.0.borrow_mut().cur.take().unwrap_or_default()
                        }
                        _ => return Ok(None),
                    }
                }
                _ => return Ok(None),
            }))
        })();
        r.transpose()
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
    let blocks: Vec<f64> = fs
        .chunks(4096)
        .map(|b| {
            let mut lanes = [-0.0f64; 64];
            for (i, x) in b.iter().enumerate() {
                lanes[i % 64] += x;
            }
            tree(&lanes)
        })
        .collect();
    Ok(Value::Float(if blocks.is_empty() {
        -0.0
    } else {
        tree(&blocks)
    }))
}

/// The fixed tree: split at the largest power of two below the length.
fn tree(xs: &[f64]) -> f64 {
    match xs.len() {
        1 => xs[0],
        n => {
            let k = 1 << (usize::BITS - 1 - (n - 1).leading_zeros());
            tree(&xs[..k]) + tree(&xs[k..])
        }
    }
}
