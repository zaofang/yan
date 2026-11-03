//! `sort_unstable` for primitive keys (17, R494). With primitive keys equal elements cannot be
//! told apart, so every kernel gives the same output and the choice of kernel, made once per
//! process like `@multiversion` (11.3), is not observable.

use std::sync::OnceLock;

/// Sort `xs` ascending.
pub fn sort_unstable_i64(xs: &mut [i64]) {
    static KERNEL: OnceLock<fn(&mut [i64])> = OnceLock::new();
    let k = KERNEL.get_or_init(|| {
        #[cfg(target_arch = "x86_64")]
        if std::arch::is_x86_feature_detected!("avx512f") {
            return avx512::sort;
        }
        <[i64]>::sort_unstable
    });
    k(xs)
}

/// Vectorized quicksort after vqsort (Blacher, Giesen, Sanders and Wassenberg, 2022): an
/// in-place partition that permutes each vector into its two sides and stores it at both
/// ends, a bitonic network in registers for small ranges, and ipnsort when recursion gets too
/// deep.
#[cfg(target_arch = "x86_64")]
#[allow(unsafe_code)]
mod avx512 {
    use std::arch::x86_64::*;

    const LANES: usize = 8;
    /// Ranges up to this length are sorted in registers.
    const SMALL: usize = 64;

    pub fn sort(xs: &mut [i64]) {
        let depth = 2 * (usize::BITS - xs.len().leading_zeros());
        // SAFETY: `sort_unstable_i64` selects this kernel only when the CPU has AVX-512F.
        unsafe { quicksort(xs, depth, None) }
    }

    /// `bound`, when set, is a pivot no greater than any element of `xs` (pdqsort's
    /// predecessor): if the pivot equals it, every copy of the pivot is split off as done.
    #[target_feature(enable = "avx512f")]
    fn quicksort(mut xs: &mut [i64], mut depth: u32, mut bound: Option<i64>) {
        while xs.len() > SMALL {
            if depth == 0 {
                xs.sort_unstable();
                return;
            }
            depth -= 1;
            let p = pivot(xs);
            if bound == Some(p) {
                let eq = partition(xs, p, true);
                xs = &mut xs[eq..];
                continue;
            }
            let (lo, hi) = {
                let lt = partition(xs, p, false);
                xs.split_at_mut(lt)
            };
            // Recurse into the smaller side, loop on the larger.
            if lo.len() < hi.len() {
                quicksort(lo, depth, bound);
                (xs, bound) = (hi, Some(p));
            } else {
                quicksort(hi, depth, Some(p));
                xs = lo;
            }
        }
        small_sort(xs);
    }

    fn median3(a: i64, b: i64, c: i64) -> i64 {
        a.max(b).min(a.min(b).max(c))
    }

    /// Tukey's ninther over nine evenly spaced samples.
    fn pivot(xs: &[i64]) -> i64 {
        let step = xs.len() / 9;
        let s = |i: usize| xs[i * step];
        median3(
            median3(s(0), s(1), s(2)),
            median3(s(3), s(4), s(5)),
            median3(s(6), s(7), s(8)),
        )
    }

    /// The lanes of `v` that go left: `<= p`, or `< p` when not `le`.
    #[inline]
    #[target_feature(enable = "avx512f")]
    fn goes_left(v: __m512i, pv: __m512i, le: bool) -> __mmask8 {
        if le {
            _mm512_cmple_epi64_mask(v, pv)
        } else {
            _mm512_cmplt_epi64_mask(v, pv)
        }
    }

    /// For each mask, the lane order that puts the masked lanes first, in order, then the rest.
    static ORDER: [[i64; LANES]; 256] = {
        let mut t = [[0; LANES]; 256];
        let mut m = 0;
        while m < 256 {
            let mut at = 0;
            let mut pass = 0;
            while pass < 2 {
                let mut lane = 0;
                while lane < LANES {
                    if ((m >> lane) & 1 == 1) == (pass == 0) {
                        t[m][at] = lane as i64;
                        at += 1;
                    }
                    lane += 1;
                }
                pass += 1;
            }
            m += 1;
        }
        t
    };

    /// `v` with its left lanes (`m`) first and its right lanes last.
    #[inline]
    #[target_feature(enable = "avx512f")]
    fn split(v: __m512i, m: __mmask8) -> __m512i {
        // SAFETY: `ORDER[m]` is 64 readable bytes.
        let idx = unsafe { _mm512_loadu_epi64(ORDER[m as usize].as_ptr()) };
        _mm512_permutexvar_epi64(idx, v)
    }

    /// Write `v`'s left lanes at `wl` and its right lanes just below `wr`. Both sides are
    /// written as whole vectors of [`split`] (a compress to memory is microcoded on Intel), so
    /// both need a vector of free space.
    ///
    /// # Safety
    ///
    /// `base + wl .. base + wl + LANES` and `base + wr - LANES .. base + wr` are writable and
    /// hold nothing still to be read.
    #[inline]
    #[target_feature(enable = "avx512f")]
    unsafe fn store(base: *mut i64, v: __m512i, m: __mmask8, wl: &mut usize, wr: &mut usize) {
        let k = m.count_ones() as usize;
        let s = split(v, m);
        // SAFETY: the caller's contract.
        unsafe {
            _mm512_storeu_epi64(base.add(*wl), s);
            _mm512_storeu_epi64(base.add(*wr - LANES), s);
        }
        *wl += k;
        *wr -= LANES - k;
    }

    /// Sort `xs` (at most `SMALL` elements) with a bitonic network over `N` registers,
    /// padded with `i64::MAX`.
    #[target_feature(enable = "avx512f")]
    fn small_sort(xs: &mut [i64]) {
        match xs.len().div_ceil(LANES) {
            0 | 1 => network::<1>(xs),
            2 => network::<2>(xs),
            3 | 4 => network::<4>(xs),
            5..=8 => network::<8>(xs),
            _ => network::<16>(xs),
        }
    }

    #[inline]
    #[target_feature(enable = "avx512f")]
    fn network<const N: usize>(xs: &mut [i64]) {
        let n = xs.len();
        debug_assert!(n <= N * LANES);
        let base = xs.as_mut_ptr();
        let pad = _mm512_set1_epi64(i64::MAX);
        let mask = |v: usize| -> __mmask8 {
            let have = n.saturating_sub(v * LANES).min(LANES);
            ((1u16 << have) - 1) as u8
        };
        // SAFETY: masked loads and stores touch only lanes below `n`.
        let mut a: [__m512i; N] = std::array::from_fn(|v| unsafe {
            _mm512_mask_loadu_epi64(pad, mask(v), base.add(v * LANES))
        });
        let swap = [
            _mm512_set_epi64(6, 7, 4, 5, 2, 3, 0, 1),
            _mm512_set_epi64(5, 4, 7, 6, 1, 0, 3, 2),
            _mm512_set_epi64(3, 2, 1, 0, 7, 6, 5, 4),
        ];
        // Lanes `i` with `i & j == 0`, for j = 1, 2, 4.
        const LOW: [u8; 3] = [0x55, 0x33, 0x0f];
        let mut k = 2;
        while k <= N * LANES {
            let mut j = k / 2;
            while j > 0 {
                if j >= LANES {
                    let jv = j / LANES;
                    for v in 0..N {
                        let w = v ^ jv;
                        if w > v {
                            let (lo, hi) =
                                (_mm512_min_epi64(a[v], a[w]), _mm512_max_epi64(a[v], a[w]));
                            let asc = (v * LANES) & k == 0;
                            (a[v], a[w]) = if asc { (lo, hi) } else { (hi, lo) };
                        }
                    }
                } else {
                    let t = j.trailing_zeros() as usize;
                    for (v, x) in a.iter_mut().enumerate() {
                        // Descending lanes: `i & k != 0` for element index `i`.
                        let desc: u8 = match k {
                            2 => 0xcc,
                            4 => 0xf0,
                            _ if (v * LANES) & k != 0 => 0xff,
                            _ => 0,
                        };
                        let p = _mm512_permutexvar_epi64(swap[t], *x);
                        let (lo, hi) = (_mm512_min_epi64(*x, p), _mm512_max_epi64(*x, p));
                        *x = _mm512_mask_blend_epi64(LOW[t] ^ desc, hi, lo);
                    }
                }
                j /= 2;
            }
            k *= 2;
        }
        for (v, x) in a.into_iter().enumerate() {
            // SAFETY: as for the loads.
            unsafe { _mm512_mask_storeu_epi64(base.add(v * LANES), mask(v), x) };
        }
    }

    /// Vectors read per side choice: one unpredictable branch per `UNROLL` vectors.
    const UNROLL: usize = 4;
    const BLOCK: usize = UNROLL * LANES;

    /// Reorder `xs` so the elements that go left (`<= p`, or `< p` when not `le`) come first,
    /// and return how many there are. `xs` holds at least `2 * BLOCK` elements.
    #[target_feature(enable = "avx512f")]
    fn partition(xs: &mut [i64], p: i64, le: bool) -> usize {
        let n = xs.len();
        assert!(n >= 2 * BLOCK);
        let base = xs.as_mut_ptr();
        let pv = _mm512_set1_epi64(p);
        // SAFETY: every index below stays in `0..n`. Reads come from `l..r`, which shrinks;
        // writes go to `wl..` and `..wr`, which stay outside it. The first and last blocks are
        // held in registers, so there are `2 * BLOCK` free slots; each read takes from the side
        // with less room, which leaves both sides room for everything it writes. Once all is
        // read, the free slots are one gap `wl..wr` that the held vectors fill exactly.
        unsafe {
            let load = |at: usize| _mm512_loadu_epi64(base.add(at));
            let held: [__m512i; 2 * UNROLL] = std::array::from_fn(|i| {
                load(if i < UNROLL {
                    i * LANES
                } else {
                    n - BLOCK + (i - UNROLL) * LANES
                })
            });
            let (mut l, mut r) = (BLOCK, n - BLOCK);
            let (mut wl, mut wr) = (0, n);
            while r - l >= BLOCK {
                let at = if l - wl <= wr - r {
                    l += BLOCK;
                    l - BLOCK
                } else {
                    r -= BLOCK;
                    r
                };
                let vs: [__m512i; UNROLL] = std::array::from_fn(|i| load(at + i * LANES));
                for v in vs {
                    store(base, v, goes_left(v, pv, le), &mut wl, &mut wr);
                }
            }
            while r - l >= LANES {
                let at = if l - wl <= wr - r {
                    l += LANES;
                    l - LANES
                } else {
                    r -= LANES;
                    r
                };
                let v = load(at);
                store(base, v, goes_left(v, pv, le), &mut wl, &mut wr);
            }
            while l < r {
                let x = if l - wl <= wr - r {
                    l += 1;
                    *base.add(l - 1)
                } else {
                    r -= 1;
                    *base.add(r)
                };
                if if le { x <= p } else { x < p } {
                    *base.add(wl) = x;
                    wl += 1;
                } else {
                    wr -= 1;
                    *base.add(wr) = x;
                }
            }
            for v in held {
                store(base, v, goes_left(v, pv, le), &mut wl, &mut wr);
            }
            debug_assert_eq!(wl, wr);
            wl
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic xorshift stream.
    fn stream(seed: u64) -> impl FnMut() -> u64 {
        let mut s = seed;
        move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        }
    }

    #[test]
    fn matches_std_on_random_sorted_reversed_and_duplicate_inputs() {
        let mut next = stream(0x9e37_79b9_7f4a_7c15);
        for n in (0..300).chain([1000, 4096, 65_537, 300_000]) {
            let inputs: [Vec<i64>; 5] = [
                (0..n).map(|_| next() as i64).collect(),
                (0..n).map(|_| (next() % 4) as i64 - 2).collect(),
                (0..n as i64).collect(),
                (0..n as i64).rev().collect(),
                (0..n)
                    .map(|i| if i % 2 == 0 { i64::MIN } else { i64::MAX })
                    .collect(),
            ];
            for mut xs in inputs {
                let mut want = xs.clone();
                want.sort_unstable();
                sort_unstable_i64(&mut xs);
                assert_eq!(xs, want, "n = {n}");
            }
        }
    }
}
