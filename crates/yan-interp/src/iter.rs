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

use yan_std::registry::{Id, id::*};

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
    /// Source, step (`None` adds), the running value, and whether to emit it first.
    Accumulate(It, Option<F>, Option<Value>, bool),
}

fn it(p: Pipe) -> Value {
    Value::Iter(It(Rc::new(RefCell::new(Stage { pipe: p, cur: None }))))
}

fn items(xs: impl IntoIterator<Item = Value>) -> Value {
    it(Pipe::Items(xs.into_iter().collect()))
}

/// The payload of a `T | None` argument.
fn unwrap_opt(v: &Value) -> Option<&Value> {
    match v {
        Value::Data { name, fields } if name == "None" && fields.is_empty() => None,
        Value::Data { name, fields } if name == "Some" => fields.first().map(|f| &f.1),
        v => Some(v),
    }
}

/// Python's `accumulate` default, `+` on ints and floats.
fn add(a: Value, b: Value) -> R<Value> {
    match (a, b) {
        (Value::Int(a), Value::Int(b)) => match a.checked_add(b) {
            Some(n) if (i64::MIN as i128..=i64::MAX as i128).contains(&n) => Ok(Value::Int(n)),
            _ => panic("overflow"),
        },
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
        _ => panic("value"),
    }
}

/// Ordered selections of `r` items, by position, in lexicographic index order.
fn permutations(xs: &[Value], r: i128) -> Vec<Vec<Value>> {
    let n = xs.len();
    if r < 0 || r as usize > n {
        return Vec::new();
    }
    let r = r as usize;
    let mut out = Vec::new();
    let mut used = vec![false; n];
    let mut cur = Vec::with_capacity(r);
    fn go(
        xs: &[Value],
        r: usize,
        used: &mut [bool],
        cur: &mut Vec<usize>,
        out: &mut Vec<Vec<Value>>,
    ) {
        if cur.len() == r {
            out.push(cur.iter().map(|&i| xs[i].clone()).collect());
            return;
        }
        for i in 0..xs.len() {
            if !used[i] {
                used[i] = true;
                cur.push(i);
                go(xs, r, used, cur, out);
                cur.pop();
                used[i] = false;
            }
        }
    }
    go(xs, r, &mut used, &mut cur, &mut out);
    out
}

/// Sorted-position selections of `r` items, with repeats allowed when `repeat`.
fn combinations(xs: &[Value], r: i128, repeat: bool) -> Vec<Vec<Value>> {
    if r < 0 {
        return Vec::new();
    }
    fn go(
        xs: &[Value],
        r: usize,
        from: usize,
        repeat: bool,
        cur: &mut Vec<Value>,
        out: &mut Vec<Vec<Value>>,
    ) {
        if cur.len() == r {
            out.push(cur.clone());
            return;
        }
        for i in from..xs.len() {
            cur.push(xs[i].clone());
            go(xs, r, if repeat { i } else { i + 1 }, repeat, cur, out);
            cur.pop();
        }
    }
    let mut out = Vec::new();
    go(xs, r as usize, 0, repeat, &mut Vec::new(), &mut out);
    out
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
            Pipe::Accumulate(_, _, acc, first @ true) => {
                *first = false;
                acc.clone()
            }
            Pipe::Accumulate(src, f, acc, _) => match self.pull(src)? {
                Some(x) => {
                    let next = match (acc.take(), &*f) {
                        (None, _) => x,
                        (Some(a), Some(f)) => self.apply(f, vec![a, x])?,
                        (Some(a), None) => add(a, x)?,
                    };
                    *acc = Some(next.clone());
                    Some(next)
                }
                None => None,
            },
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
            (STD_ITERTOOLS_PERMUTATIONS, [List(xs), r]) => {
                let r = match unwrap_opt(r) {
                    Some(Int(r)) => *r,
                    _ => xs.len() as i128,
                };
                items(permutations(xs, r).into_iter().map(List))
            }
            (STD_ITERTOOLS_COMBINATIONS, [List(xs), Int(r)]) => {
                items(combinations(xs, *r, false).into_iter().map(List))
            }
            (STD_ITERTOOLS_COMBINATIONS_WITH_REPLACEMENT, [List(xs), Int(r)]) => {
                items(combinations(xs, *r, true).into_iter().map(List))
            }
            (STD_ITERTOOLS_PRODUCT, [List(a), List(b)]) => items(
                a.iter()
                    .flat_map(|x| b.iter().map(move |y| Tuple(vec![x.clone(), y.clone()]))),
            ),
            (STD_ITERTOOLS_PAIRWISE, [List(xs)]) => items(xs.windows(2).map(|w| Tuple(w.to_vec()))),
            (STD_ITERTOOLS_ACCUMULATE, [List(xs), f, init]) => {
                let f = match unwrap_opt(f) {
                    Some(Closure(f)) => Some(f.clone()),
                    _ => None,
                };
                let init = unwrap_opt(init).cloned();
                let first = init.is_some();
                it(Pipe::Accumulate(
                    It(Rc::new(RefCell::new(Stage {
                        pipe: Pipe::Items(xs.iter().cloned().collect()),
                        cur: None,
                    }))),
                    f,
                    init,
                    first,
                ))
            }
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
                    (STD_ITER_OPS_CONTAINS, [x]) => loop {
                        match self.pull(&i)? {
                            Some(y) if y == *x => break Bool(true),
                            Some(_) => {}
                            None => break Bool(false),
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
                    (
                        STD_ITER_OPS_MIN
                        | STD_ITER_OPS_MAX
                        | STD_ITER_OPS_MIN_NONEMPTY
                        | STD_ITER_OPS_MAX_NONEMPTY,
                        [key],
                    ) => {
                        let key = key.clone();
                        let mut best: Option<(Value, Value)> = None;
                        while let Some(x) = self.pull(&i)? {
                            let k = self.key(&key, &x)?;
                            let o = best.as_ref().map(|(b, _)| cmp(&k, b));
                            let wins = match o {
                                None => true,
                                Some(o)
                                    if matches!(
                                        id,
                                        STD_ITER_OPS_MIN | STD_ITER_OPS_MIN_NONEMPTY
                                    ) =>
                                {
                                    o.is_lt()
                                }
                                Some(o) => o.is_gt(),
                            };
                            if wins {
                                best = Some((k, x));
                            }
                        }
                        match best {
                            // R658: an empty iterable with no `default`, as Python's `ValueError`.
                            None if matches!(
                                id,
                                STD_ITER_OPS_MIN_NONEMPTY | STD_ITER_OPS_MAX_NONEMPTY
                            ) =>
                            {
                                return panic("value");
                            }
                            Some((_, x))
                                if matches!(
                                    id,
                                    STD_ITER_OPS_MIN_NONEMPTY | STD_ITER_OPS_MAX_NONEMPTY
                                ) =>
                            {
                                x
                            }
                            b => opt(b.map(|(_, x)| x)),
                        }
                    }
                    (STD_ITER_OPS_SUM, []) => sum(self.drain(&i)?)?,
                    (STD_ITER_OPS_SUM_FLOAT, []) => sum_float(self.drain(&i)?),
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
    Ok(sum_float(xs))
}

/// The float `sum`; unlike [`sum`] it knows from the element type that an empty input is a float.
pub(crate) fn sum_float(xs: Vec<Value>) -> Value {
    let fs: Vec<f64> = xs
        .iter()
        .map(|x| match x {
            Value::Float(f) => *f,
            _ => 0.0,
        })
        .collect();
    Value::Float(yan_rt::num::sum_f64(&fs))
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
    STD_ITER_OPS_CONTAINS,
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
    STD_ITER_OPS_MAX_NONEMPTY,
    STD_ITER_OPS_MIN,
    STD_ITER_OPS_MIN_NONEMPTY,
    STD_ITER_OPS_NEXT,
    STD_ITER_OPS_SKIP,
    STD_ITER_OPS_STEP_BY,
    STD_ITER_OPS_SUM,
    STD_ITER_OPS_SUM_FLOAT,
    STD_ITER_OPS_TAKE,
    STD_ITER_OPS_TAKE_WHILE,
    STD_ITER_OPS_WINDOWS,
    STD_ITER_OPS_ZIP,
    STD_ITER_REPEAT,
    STD_ITERTOOLS_ACCUMULATE,
    STD_ITERTOOLS_COMBINATIONS,
    STD_ITERTOOLS_COMBINATIONS_WITH_REPLACEMENT,
    STD_ITERTOOLS_PAIRWISE,
    STD_ITERTOOLS_PERMUTATIONS,
    STD_ITERTOOLS_PRODUCT,
    STD_LIST_ITER,
    STD_PRELUDE_DICT,
    STD_PRELUDE_SET,
    STD_SET_ITER,
    STD_TEXT_CHARS,
    STD_TEXT_ITER,
];
