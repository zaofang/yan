//! `std.itertools` selections for native code: the index tuples of `permutations`,
//! `combinations` and `combinations_with_replacement`, in Python's order.

use crate::abi::{List, leak};

pub const PERMUTATIONS: i64 = 0;
pub const COMBINATIONS: i64 = 1;
pub const WITH_REPLACEMENT: i64 = 2;

fn go(kind: i64, n: usize, r: usize, from: usize, cur: &mut Vec<usize>, out: &mut Vec<u64>) {
    if cur.len() == r {
        out.extend(cur.iter().map(|&i| i as u64));
        return;
    }
    for i in from..n {
        if kind == PERMUTATIONS && cur.contains(&i) {
            continue;
        }
        cur.push(i);
        let next = match kind {
            PERMUTATIONS => 0,
            COMBINATIONS => i + 1,
            _ => i,
        };
        go(kind, n, r, next, cur, out);
        cur.pop();
    }
}

/// A one-word-element list: the number of selections, then each selection's `r` indices.
/// A negative `r`, or one past `n` for the kinds without replacement, selects nothing.
#[unsafe(no_mangle)]
pub extern "C" fn yan_rt_select(kind: i64, n: i64, r: i64) -> *mut List {
    let mut idx = Vec::new();
    if r >= 0 && (kind == WITH_REPLACEMENT || r <= n) {
        go(kind, n as usize, r as usize, 0, &mut Vec::new(), &mut idx);
    }
    let count = match r {
        0 => 1,
        1.. => idx.len() / r as usize,
        _ => 0,
    };
    let mut data = Vec::with_capacity(idx.len() + 1);
    data.push(count as u64);
    data.extend(idx);
    leak(List {
        words: 1,
        data,
        limit: -1,
    })
}
