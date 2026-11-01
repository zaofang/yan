//! std.collections over interpreter values (R488 to R493); the heap and search kernels are in
//! `yong_rt::collections` (R561). The sorted types are sorted vectors here; compiled code uses
//! B-trees, which no program can tell apart.

use std::cmp::Ordering;
use std::collections::VecDeque;
use std::mem;

use yong_rt::collections as c;
use yong_std::registry::{Id, id::*};

use super::bind::{Bind, bad_args};
use super::native::opt;
use super::prelude::cmp;
use super::{Value, panic};
use Value::*;

fn take(v: &mut Value) -> Value {
    mem::replace(v, Unit)
}

fn pair((k, v): &(Value, Value)) -> Value {
    Tuple(vec![k.clone(), v.clone()])
}

pub(super) fn ascending(h: &[Value]) -> Vec<Value> {
    let mut xs = h.to_vec();
    xs.sort_by(cmp);
    xs
}

fn sorted(mut xs: Vec<Value>) -> Vec<Value> {
    xs.sort_by(cmp);
    xs.dedup_by(|a, b| cmp(a, b) == Ordering::Equal);
    xs
}

fn find(xs: &[Value], x: &Value) -> Result<usize, usize> {
    xs.binary_search_by(|y| cmp(y, x))
}

fn find_key(m: &[(Value, Value)], k: &Value) -> Result<usize, usize> {
    m.binary_search_by(|(y, _)| cmp(y, k))
}

fn bounded(d: &mut VecDeque<Value>, maxlen: Option<usize>, front: bool) {
    if maxlen.is_some_and(|m| d.len() > m) {
        if front {
            d.pop_back();
        } else {
            d.pop_front();
        }
    }
}

macro_rules! f {
    ($id:ident, |$a:ident| { $($pat:pat => $e:expr),+ $(,)? }) => {
        (
            $id,
            Bind::Plain(|$a| {
                Ok(match &mut *$a {
                    $($pat => $e,)+
                    #[allow(unreachable_patterns)]
                    _ => return bad_args($id),
                })
            }),
        )
    };
}

pub(super) static BINDS: &[(Id, Bind)] = &[
    f!(STD_PRELUDE_DEQUE, |a| {
        [] => Deque(VecDeque::new(), None),
        [List(xs)] => Deque(mem::take(xs).into(), None),
        [List(xs), Int(m)] => {
            let m = usize::try_from(*m).unwrap_or(0);
            let skip = xs.len().saturating_sub(m);
            Deque(xs.drain(skip..).collect(), Some(m))
        }
    }),
    f!(STD_PRELUDE_HEAP, |a| {
        [] => Heap(Vec::new()),
        [List(xs)] => Heap(c::heapify(mem::take(xs), cmp)),
    }),
    f!(STD_PRELUDE_SORTED_DICT, |a| {
        [] => SortedMap(Vec::new()),
        [Dict(d)] => {
            let mut m: Vec<_> = mem::take(d).into_iter().map(|(k, v)| (k.0, v)).collect();
            m.sort_by(|a, b| cmp(&a.0, &b.0));
            SortedMap(m)
        }
    }),
    f!(STD_PRELUDE_SORTED_SET, |a| {
        [] => SortedSet(Vec::new()),
        [List(xs)] => SortedSet(sorted(mem::take(xs))),
    }),
    f!(STD_COLLECTIONS_DEQUE_APPEND, |a| {
        [Deque(d, m), x] => {
            d.push_back(take(x));
            bounded(d, *m, false);
            Unit
        }
    }),
    f!(STD_COLLECTIONS_DEQUE_APPENDLEFT, |a| {
        [Deque(d, m), x] => {
            d.push_front(take(x));
            bounded(d, *m, true);
            Unit
        }
    }),
    f!(STD_COLLECTIONS_DEQUE_POP, |a| {
        [Deque(d, _)] => match d.pop_back() {
            Some(v) => v,
            None => return panic("bounds"),
        },
    }),
    f!(STD_COLLECTIONS_DEQUE_POPLEFT, |a| {
        [Deque(d, _)] => match d.pop_front() {
            Some(v) => v,
            None => return panic("bounds"),
        },
    }),
    f!(STD_COLLECTIONS_DEQUE_FIRST, |a| {
        [Deque(d, _)] => opt(d.front().cloned()),
    }),
    f!(STD_COLLECTIONS_DEQUE_LAST, |a| {
        [Deque(d, _)] => opt(d.back().cloned()),
    }),
    f!(STD_COLLECTIONS_DEQUE_EXTEND, |a| {
        [Deque(d, m), List(xs)] => {
            for x in mem::take(xs) {
                d.push_back(x);
                bounded(d, *m, false);
            }
            Unit
        }
    }),
    f!(STD_COLLECTIONS_DEQUE_ROTATE, |a| {
        [Deque(d, _), Int(n)] => {
            if !d.is_empty() {
                let k = n.rem_euclid(d.len() as i128) as usize;
                d.rotate_right(k);
            }
            Unit
        }
    }),
    f!(STD_COLLECTIONS_DEQUE_CLEAR, |a| {
        [Deque(d, _)] => {
            d.clear();
            Unit
        }
    }),
    f!(STD_COLLECTIONS_DEQUE_MAXLEN, |a| {
        [Deque(_, m)] => opt(m.map(|m| Int(m as i128))),
    }),
    f!(STD_COLLECTIONS_DEQUE_LEN, |a| {
        [Deque(d, _)] => Int(d.len() as i128),
    }),
    f!(STD_COLLECTIONS_DEQUE_CONTAINS, |a| {
        [Deque(d, _), x] => Bool(d.contains(x)),
    }),
    f!(STD_COLLECTIONS_HEAP_PUSH, |a| {
        [Heap(h), x] => {
            h.push(take(x));
            let i = h.len() - 1;
            c::sift_up(h, i, cmp);
            Unit
        }
    }),
    f!(STD_COLLECTIONS_HEAP_POP, |a| {
        [Heap(h)] => match c::heap_pop(h, cmp) {
            Some(v) => v,
            None => return panic("bounds"),
        },
    }),
    f!(STD_COLLECTIONS_HEAP_PEEK, |a| {
        [Heap(h)] => opt(h.first().cloned()),
    }),
    f!(STD_COLLECTIONS_HEAP_PUSHPOP, |a| {
        [Heap(h), x] => match h.first() {
            Some(top) if cmp(top, x) == Ordering::Less => {
                let top = mem::replace(&mut h[0], take(x));
                c::sift_down(h, 0, cmp);
                top
            }
            _ => take(x),
        },
    }),
    f!(STD_COLLECTIONS_HEAP_INTO_SORTED, |a| {
        [Heap(h)] => List(ascending(h)),
    }),
    f!(STD_COLLECTIONS_HEAP_LEN, |a| {
        [Heap(h)] => Int(h.len() as i128),
    }),
    f!(STD_COLLECTIONS_SORTED_DICT_INSERT, |a| {
        [SortedMap(m), k, v] => {
            match find_key(m, k) {
                Ok(i) => m[i].1 = take(v),
                Err(i) => m.insert(i, (take(k), take(v))),
            }
            Unit
        }
    }),
    f!(STD_COLLECTIONS_SORTED_DICT_INDEX, |a| {
        [SortedMap(m), k] => match find_key(m, k) {
            Ok(i) => m[i].1.clone(),
            Err(_) => return panic("bounds"),
        },
    }),
    f!(STD_COLLECTIONS_SORTED_DICT_GET, |a| {
        [SortedMap(m), k] => {
            opt(find_key(m, k).ok().map(|i| m[i].1.clone()))
        }
    }),
    f!(STD_COLLECTIONS_SORTED_DICT_CONTAINS_KEY, |a| {
        [SortedMap(m), k] => {
            Bool(find_key(m, k).is_ok())
        }
    }),
    f!(STD_COLLECTIONS_SORTED_DICT_LEN, |a| {
        [SortedMap(m)] => Int(m.len() as i128),
    }),
    f!(STD_COLLECTIONS_SORTED_DICT_REMOVE, |a| {
        [SortedMap(m), k] => {
            opt(find_key(m, k).ok().map(|i| m.remove(i).1))
        }
    }),
    f!(STD_COLLECTIONS_SORTED_DICT_SETDEFAULT, |a| {
        [SortedMap(m), k, v] => match find_key(m, k) {
            Ok(i) => m[i].1.clone(),
            Err(i) => {
                m.insert(i, (take(k), v.clone()));
                take(v)
            }
        },
    }),
    f!(STD_COLLECTIONS_SORTED_DICT_ITEMS, |a| {
        [SortedMap(m)] => List(m.iter().map(pair).collect()),
    }),
    f!(STD_COLLECTIONS_SORTED_DICT_KEYS, |a| {
        [SortedMap(m)] => {
            List(m.iter().map(|(k, _)| k.clone()).collect())
        }
    }),
    f!(STD_COLLECTIONS_SORTED_DICT_VALUES, |a| {
        [SortedMap(m)] => {
            List(m.iter().map(|(_, v)| v.clone()).collect())
        }
    }),
    f!(STD_COLLECTIONS_SORTED_DICT_FIRST, |a| {
        [SortedMap(m)] => opt(m.first().map(pair)),
    }),
    f!(STD_COLLECTIONS_SORTED_DICT_LAST, |a| {
        [SortedMap(m)] => opt(m.last().map(pair)),
    }),
    f!(STD_COLLECTIONS_SORTED_DICT_POP_FIRST, |a| {
        [SortedMap(m)] => {
            opt((!m.is_empty()).then(|| pair(&m.remove(0))))
        }
    }),
    f!(STD_COLLECTIONS_SORTED_DICT_POP_LAST, |a| {
        [SortedMap(m)] => opt(m.pop().as_ref().map(pair)),
    }),
    f!(STD_COLLECTIONS_SORTED_DICT_FLOOR, |a| {
        [SortedMap(m), k] => {
            opt(c::floor(find_key(m, k)).map(|i| pair(&m[i])))
        }
    }),
    f!(STD_COLLECTIONS_SORTED_DICT_CEILING, |a| {
        [SortedMap(m), k] => {
            opt(c::ceiling(m.len(), find_key(m, k)).map(|i| pair(&m[i])))
        }
    }),
    f!(STD_COLLECTIONS_SORTED_DICT_RANGE, |a| {
        [SortedMap(m), lo, hi] => {
            let (i, j) = c::range(find_key(m, lo), find_key(m, hi));
            List(m[i..j].iter().map(pair).collect())
        }
    }),
    f!(STD_COLLECTIONS_SORTED_SET_ADD, |a| {
        [SortedSet(s), x] => {
            if let Err(i) = find(s, x) {
                s.insert(i, take(x));
            }
            Unit
        }
    }),
    f!(STD_COLLECTIONS_SORTED_SET_DISCARD, |a| {
        [SortedSet(s), x] => {
            if let Ok(i) = find(s, x) {
                s.remove(i);
            }
            Unit
        }
    }),
    f!(STD_COLLECTIONS_SORTED_SET_CONTAINS, |a| {
        [SortedSet(s), x] => Bool(find(s, x).is_ok()),
    }),
    f!(STD_COLLECTIONS_SORTED_SET_LEN, |a| {
        [SortedSet(s)] => Int(s.len() as i128),
    }),
    f!(STD_COLLECTIONS_SORTED_SET_UNION, |a| {
        [SortedSet(x), SortedSet(y)] => {
            SortedSet(sorted(x.iter().chain(y.iter()).cloned().collect()))
        }
    }),
    f!(STD_COLLECTIONS_SORTED_SET_INTERSECTION, |a| {
        [SortedSet(x), SortedSet(y)] => {
            SortedSet(x.iter().filter(|v| find(y, v).is_ok()).cloned().collect())
        }
    }),
    f!(STD_COLLECTIONS_SORTED_SET_FIRST, |a| {
        [SortedSet(s)] => opt(s.first().cloned()),
    }),
    f!(STD_COLLECTIONS_SORTED_SET_LAST, |a| {
        [SortedSet(s)] => opt(s.last().cloned()),
    }),
    f!(STD_COLLECTIONS_SORTED_SET_POP_FIRST, |a| {
        [SortedSet(s)] => {
            opt((!s.is_empty()).then(|| s.remove(0)))
        }
    }),
    f!(STD_COLLECTIONS_SORTED_SET_POP_LAST, |a| {
        [SortedSet(s)] => opt(s.pop()),
    }),
    f!(STD_COLLECTIONS_SORTED_SET_FLOOR, |a| {
        [SortedSet(s), x] => {
            opt(c::floor(find(s, x)).map(|i| s[i].clone()))
        }
    }),
    f!(STD_COLLECTIONS_SORTED_SET_CEILING, |a| {
        [SortedSet(s), x] => {
            opt(c::ceiling(s.len(), find(s, x)).map(|i| s[i].clone()))
        }
    }),
    f!(STD_COLLECTIONS_SORTED_SET_RANGE, |a| {
        [SortedSet(s), lo, hi] => {
            let (i, j) = c::range(find(s, lo), find(s, hi));
            List(s[i..j].to_vec())
        }
    }),
];
