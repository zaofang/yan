//! std.collections over interpreter values (R488 to R493). The sorted types are sorted vectors
//! here; compiled code uses B-trees, which no program can tell apart.

use std::cmp::Ordering;
use std::collections::VecDeque;
use std::mem;

use super::native::opt;
use super::prelude::cmp;
use super::{R, Value, panic};

fn take(v: &mut Value) -> Value {
    mem::replace(v, Value::Unit)
}

fn pair((k, v): &(Value, Value)) -> Value {
    Value::Tuple(vec![k.clone(), v.clone()])
}

const ARITY: usize = 4;

fn sift_up(h: &mut [Value], mut i: usize) {
    while i > 0 {
        let p = (i - 1) / ARITY;
        if cmp(&h[i], &h[p]) != Ordering::Less {
            break;
        }
        h.swap(i, p);
        i = p;
    }
}

fn sift_down(h: &mut [Value], mut i: usize) {
    loop {
        let first = i * ARITY + 1;
        let Some(m) = (first..(first + ARITY).min(h.len())).min_by(|&a, &b| cmp(&h[a], &h[b]))
        else {
            return;
        };
        if cmp(&h[m], &h[i]) != Ordering::Less {
            return;
        }
        h.swap(i, m);
        i = m;
    }
}

fn heapify(mut h: Vec<Value>) -> Vec<Value> {
    for i in (0..h.len() / ARITY + 1).rev() {
        sift_down(&mut h, i);
    }
    h
}

fn heap_pop(h: &mut Vec<Value>) -> Option<Value> {
    let last = h.pop()?;
    if h.is_empty() {
        return Some(last);
    }
    let top = mem::replace(&mut h[0], last);
    sift_down(h, 0);
    Some(top)
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

/// From a binary search's result, the last index at most the key.
fn floor(at: Result<usize, usize>) -> Option<usize> {
    at.map_or_else(|i| i.checked_sub(1), Some)
}

fn ceiling(n: usize, at: Result<usize, usize>) -> Option<usize> {
    let i = at.unwrap_or_else(|i| i);
    (i < n).then_some(i)
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

pub(super) fn call(name: &str, args: &mut [Value]) -> Option<R<Value>> {
    use Value::*;
    Some(Ok(match (name, &mut *args) {
        ("std.prelude.deque", []) => Deque(VecDeque::new(), None),
        ("std.prelude.deque", [List(xs)]) => Deque(mem::take(xs).into(), None),
        ("std.prelude.deque", [List(xs), Int(m)]) => {
            let m = usize::try_from(*m).unwrap_or(0);
            let skip = xs.len().saturating_sub(m);
            Deque(xs.drain(skip..).collect(), Some(m))
        }
        ("std.prelude.heap", []) => Heap(Vec::new()),
        ("std.prelude.heap", [List(xs)]) => Heap(heapify(mem::take(xs))),
        ("std.prelude.sorted_dict", []) => SortedMap(Vec::new()),
        ("std.prelude.sorted_dict", [Dict(d)]) => {
            let mut m: Vec<_> = mem::take(d).into_iter().map(|(k, v)| (k.0, v)).collect();
            m.sort_by(|a, b| cmp(&a.0, &b.0));
            SortedMap(m)
        }
        ("std.prelude.sorted_set", []) => SortedSet(Vec::new()),
        ("std.prelude.sorted_set", [List(xs)]) => SortedSet(sorted(mem::take(xs))),

        ("std.collections.deque.append", [Deque(d, m), x]) => {
            d.push_back(take(x));
            bounded(d, *m, false);
            Unit
        }
        ("std.collections.deque.appendleft", [Deque(d, m), x]) => {
            d.push_front(take(x));
            bounded(d, *m, true);
            Unit
        }
        ("std.collections.deque.pop", [Deque(d, _)]) => match d.pop_back() {
            Some(v) => v,
            None => return Some(panic("bounds")),
        },
        ("std.collections.deque.popleft", [Deque(d, _)]) => match d.pop_front() {
            Some(v) => v,
            None => return Some(panic("bounds")),
        },
        ("std.collections.deque.first", [Deque(d, _)]) => opt(d.front().cloned()),
        ("std.collections.deque.last", [Deque(d, _)]) => opt(d.back().cloned()),
        ("std.collections.deque.extend", [Deque(d, m), List(xs)]) => {
            for x in mem::take(xs) {
                d.push_back(x);
                bounded(d, *m, false);
            }
            Unit
        }
        ("std.collections.deque.rotate", [Deque(d, _), Int(n)]) => {
            if !d.is_empty() {
                let k = n.rem_euclid(d.len() as i128) as usize;
                d.rotate_right(k);
            }
            Unit
        }
        ("std.collections.deque.clear", [Deque(d, _)]) => {
            d.clear();
            Unit
        }
        ("std.collections.deque.maxlen", [Deque(_, m)]) => opt(m.map(|m| Int(m as i128))),
        ("std.collections.deque.len", [Deque(d, _)]) => Int(d.len() as i128),
        ("std.collections.deque.contains", [Deque(d, _), x]) => Bool(d.contains(x)),

        ("std.collections.heap.push", [Heap(h), x]) => {
            h.push(take(x));
            let i = h.len() - 1;
            sift_up(h, i);
            Unit
        }
        ("std.collections.heap.pop", [Heap(h)]) => match heap_pop(h) {
            Some(v) => v,
            None => return Some(panic("bounds")),
        },
        ("std.collections.heap.peek", [Heap(h)]) => opt(h.first().cloned()),
        ("std.collections.heap.pushpop", [Heap(h), x]) => match h.first() {
            Some(top) if cmp(top, x) == Ordering::Less => {
                let top = mem::replace(&mut h[0], take(x));
                sift_down(h, 0);
                top
            }
            _ => take(x),
        },
        ("std.collections.heap.into_sorted", [Heap(h)]) => List(ascending(h)),
        ("std.collections.heap.len", [Heap(h)]) => Int(h.len() as i128),

        ("std.collections.sorted_dict.insert", [SortedMap(m), k, v]) => {
            match find_key(m, k) {
                Ok(i) => m[i].1 = take(v),
                Err(i) => m.insert(i, (take(k), take(v))),
            }
            Unit
        }
        ("std.collections.sorted_dict.index", [SortedMap(m), k]) => match find_key(m, k) {
            Ok(i) => m[i].1.clone(),
            Err(_) => return Some(panic("bounds")),
        },
        ("std.collections.sorted_dict.get", [SortedMap(m), k]) => {
            opt(find_key(m, k).ok().map(|i| m[i].1.clone()))
        }
        ("std.collections.sorted_dict.contains_key", [SortedMap(m), k]) => {
            Bool(find_key(m, k).is_ok())
        }
        ("std.collections.sorted_dict.len", [SortedMap(m)]) => Int(m.len() as i128),
        ("std.collections.sorted_dict.remove", [SortedMap(m), k]) => {
            opt(find_key(m, k).ok().map(|i| m.remove(i).1))
        }
        ("std.collections.sorted_dict.setdefault", [SortedMap(m), k, v]) => match find_key(m, k) {
            Ok(i) => m[i].1.clone(),
            Err(i) => {
                m.insert(i, (take(k), v.clone()));
                take(v)
            }
        },
        ("std.collections.sorted_dict.items", [SortedMap(m)]) => List(m.iter().map(pair).collect()),
        ("std.collections.sorted_dict.keys", [SortedMap(m)]) => {
            List(m.iter().map(|(k, _)| k.clone()).collect())
        }
        ("std.collections.sorted_dict.values", [SortedMap(m)]) => {
            List(m.iter().map(|(_, v)| v.clone()).collect())
        }
        ("std.collections.sorted_dict.first", [SortedMap(m)]) => opt(m.first().map(pair)),
        ("std.collections.sorted_dict.last", [SortedMap(m)]) => opt(m.last().map(pair)),
        ("std.collections.sorted_dict.pop_first", [SortedMap(m)]) => {
            opt((!m.is_empty()).then(|| pair(&m.remove(0))))
        }
        ("std.collections.sorted_dict.pop_last", [SortedMap(m)]) => opt(m.pop().as_ref().map(pair)),
        ("std.collections.sorted_dict.floor", [SortedMap(m), k]) => {
            opt(floor(find_key(m, k)).map(|i| pair(&m[i])))
        }
        ("std.collections.sorted_dict.ceiling", [SortedMap(m), k]) => {
            opt(ceiling(m.len(), find_key(m, k)).map(|i| pair(&m[i])))
        }
        ("std.collections.sorted_dict.range", [SortedMap(m), lo, hi]) => {
            let a = find_key(m, lo).unwrap_or_else(|i| i);
            let b = find_key(m, hi).unwrap_or_else(|i| i).max(a);
            List(m[a..b].iter().map(pair).collect())
        }

        ("std.collections.sorted_set.add", [SortedSet(s), x]) => {
            if let Err(i) = find(s, x) {
                s.insert(i, take(x));
            }
            Unit
        }
        ("std.collections.sorted_set.discard", [SortedSet(s), x]) => {
            if let Ok(i) = find(s, x) {
                s.remove(i);
            }
            Unit
        }
        ("std.collections.sorted_set.contains", [SortedSet(s), x]) => Bool(find(s, x).is_ok()),
        ("std.collections.sorted_set.len", [SortedSet(s)]) => Int(s.len() as i128),
        ("std.collections.sorted_set.union", [SortedSet(a), SortedSet(b)]) => {
            SortedSet(sorted(a.iter().chain(b.iter()).cloned().collect()))
        }
        ("std.collections.sorted_set.intersection", [SortedSet(a), SortedSet(b)]) => {
            SortedSet(a.iter().filter(|x| find(b, x).is_ok()).cloned().collect())
        }
        ("std.collections.sorted_set.first", [SortedSet(s)]) => opt(s.first().cloned()),
        ("std.collections.sorted_set.last", [SortedSet(s)]) => opt(s.last().cloned()),
        ("std.collections.sorted_set.pop_first", [SortedSet(s)]) => {
            opt((!s.is_empty()).then(|| s.remove(0)))
        }
        ("std.collections.sorted_set.pop_last", [SortedSet(s)]) => opt(s.pop()),
        ("std.collections.sorted_set.floor", [SortedSet(s), x]) => {
            opt(floor(find(s, x)).map(|i| s[i].clone()))
        }
        ("std.collections.sorted_set.ceiling", [SortedSet(s), x]) => {
            opt(ceiling(s.len(), find(s, x)).map(|i| s[i].clone()))
        }
        ("std.collections.sorted_set.range", [SortedSet(s), lo, hi]) => {
            let a = find(s, lo).unwrap_or_else(|i| i);
            let b = find(s, hi).unwrap_or_else(|i| i).max(a);
            List(s[a..b].to_vec())
        }
        _ => return None,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ints(xs: &[i128]) -> Vec<Value> {
        xs.iter().map(|&i| Value::Int(i)).collect()
    }

    #[test]
    fn heap_pops_in_order() {
        let mut h = heapify(ints(&[5, 3, 9, 1, 7, 2, 8, 6, 4, 0, 11, 10]));
        let mut out = Vec::new();
        while let Some(v) = heap_pop(&mut h) {
            out.push(v);
        }
        assert_eq!(out, ints(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]));
    }

    #[test]
    fn floor_and_ceiling_at_the_edges() {
        let s = ints(&[2, 4, 6]);
        let at = |x| find(&s, &Value::Int(x));
        assert_eq!(floor(at(1)), None);
        assert_eq!(floor(at(5)), Some(1));
        assert_eq!(floor(at(6)), Some(2));
        assert_eq!(ceiling(3, at(5)), Some(2));
        assert_eq!(ceiling(3, at(7)), None);
    }
}
