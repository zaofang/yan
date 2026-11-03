//! The `std.rand` generators and derived operations. Each stream is fixed per seed and versioned
//! with the module: changing an output here breaks every seeded program.

pub fn splitmix64(x: &mut u64) -> u64 {
    *x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = *x;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

fn four(seed: u64) -> [u64; 4] {
    let mut s = seed;
    [(); 4].map(|_| splitmix64(&mut s))
}

/// ChaCha with 8 rounds, a 64-bit block counter and stream 0: the words of `rand_chacha` 0.9's
/// `ChaCha8Rng::from_seed(key)`.
#[derive(Clone, Debug, PartialEq)]
pub struct ChaCha8 {
    key: [u32; 8],
    counter: u64,
    block: [u32; 16],
    index: usize,
}

impl ChaCha8 {
    pub fn new(seed: u64) -> Self {
        let mut key = [0; 8];
        for (i, w) in four(seed).into_iter().enumerate() {
            key[2 * i] = w as u32;
            key[2 * i + 1] = (w >> 32) as u32;
        }
        ChaCha8 {
            key,
            counter: 0,
            block: [0; 16],
            index: 16,
        }
    }

    fn refill(&mut self) {
        let mut input = [0u32; 16];
        input[..4].copy_from_slice(&[0x6170_7865, 0x3320_646e, 0x7962_2d32, 0x6b20_6574]);
        input[4..12].copy_from_slice(&self.key);
        input[12] = self.counter as u32;
        input[13] = (self.counter >> 32) as u32;
        let mut x = input;
        fn qr(x: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
            x[a] = x[a].wrapping_add(x[b]);
            x[d] = (x[d] ^ x[a]).rotate_left(16);
            x[c] = x[c].wrapping_add(x[d]);
            x[b] = (x[b] ^ x[c]).rotate_left(12);
            x[a] = x[a].wrapping_add(x[b]);
            x[d] = (x[d] ^ x[a]).rotate_left(8);
            x[c] = x[c].wrapping_add(x[d]);
            x[b] = (x[b] ^ x[c]).rotate_left(7);
        }
        for _ in 0..4 {
            qr(&mut x, 0, 4, 8, 12);
            qr(&mut x, 1, 5, 9, 13);
            qr(&mut x, 2, 6, 10, 14);
            qr(&mut x, 3, 7, 11, 15);
            qr(&mut x, 0, 5, 10, 15);
            qr(&mut x, 1, 6, 11, 12);
            qr(&mut x, 2, 7, 8, 13);
            qr(&mut x, 3, 4, 9, 14);
        }
        for (o, i) in x.iter_mut().zip(input) {
            *o = o.wrapping_add(i);
        }
        self.block = x;
        self.counter = self.counter.wrapping_add(1);
        self.index = 0;
    }

    pub fn next_u64(&mut self) -> u64 {
        if self.index >= 16 {
            self.refill();
        }
        let (lo, hi) = (self.block[self.index], self.block[self.index + 1]);
        self.index += 2;
        u64::from(hi) << 32 | u64::from(lo)
    }
}

/// PCG with a 128-bit LCG, the cheap multiplier and the DXSM output: `rand_pcg` 0.9's
/// `Lcg128CmDxsm64::new(state, stream)`, which is NumPy's `PCG64DXSM`.
#[derive(Clone, Debug, PartialEq)]
pub struct Pcg64Dxsm {
    state: u128,
    increment: u128,
}

const PCG_MUL: u64 = 0xda94_2042_e4dd_58b5;

impl Pcg64Dxsm {
    pub fn new(seed: u64) -> Self {
        let [a, b, c, d] = four(seed);
        let state = u128::from(a) << 64 | u128::from(b);
        let stream = u128::from(c) << 64 | u128::from(d);
        let increment = stream << 1 | 1;
        let mut p = Pcg64Dxsm {
            state: state.wrapping_add(increment),
            increment,
        };
        p.step();
        p
    }

    fn step(&mut self) {
        self.state = self
            .state
            .wrapping_mul(u128::from(PCG_MUL))
            .wrapping_add(self.increment);
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut hi = (self.state >> 64) as u64;
        let lo = self.state as u64 | 1;
        hi ^= hi >> 32;
        hi = hi.wrapping_mul(PCG_MUL);
        hi ^= hi >> 48;
        hi = hi.wrapping_mul(lo);
        self.step();
        hi
    }
}

/// `[0, 1)` from the top 53 bits.
pub fn f64_from(x: u64) -> f64 {
    (x >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
}

/// Uniform in `[0, n)` for `n > 0`, by Lemire's nearly divisionless method.
pub fn below(n: u64, mut next: impl FnMut() -> u64) -> u64 {
    let mut m = u128::from(next()) * u128::from(n);
    if (m as u64) < n {
        let t = n.wrapping_neg() % n;
        while (m as u64) < t {
            m = u128::from(next()) * u128::from(n);
        }
    }
    (m >> 64) as u64
}

/// The seed of branch `k` of a `par` under a handler seeded with `parent`.
pub fn split(parent: u64, k: u64) -> u64 {
    let mut a = k + 1;
    let mut b = parent ^ splitmix64(&mut a);
    splitmix64(&mut b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::{RngCore, SeedableRng};

    fn key(seed: u64) -> [u8; 32] {
        let mut k = [0u8; 32];
        for (i, w) in four(seed).into_iter().enumerate() {
            k[8 * i..8 * i + 8].copy_from_slice(&w.to_le_bytes());
        }
        k
    }

    #[test]
    fn chacha8_matches_rand_chacha() {
        for seed in [0, 42, u64::MAX] {
            let mut ours = ChaCha8::new(seed);
            let mut theirs = rand_chacha::ChaCha8Rng::from_seed(key(seed));
            for _ in 0..100 {
                assert_eq!(ours.next_u64(), theirs.next_u64());
            }
        }
    }

    #[test]
    fn pcg64dxsm_matches_rand_pcg() {
        for seed in [0, 42, u64::MAX] {
            let [a, b, c, d] = four(seed);
            let mut ours = Pcg64Dxsm::new(seed);
            let mut theirs = rand_pcg::Lcg128CmDxsm64::new(
                u128::from(a) << 64 | u128::from(b),
                u128::from(c) << 64 | u128::from(d),
            );
            for _ in 0..100 {
                assert_eq!(ours.next_u64(), theirs.next_u64());
            }
        }
    }

    #[test]
    fn streams_are_pinned() {
        let mut c = ChaCha8::new(0);
        let mut p = Pcg64Dxsm::new(0);
        assert_eq!(c.next_u64(), 0xbf94_d133_2d8e_e5e8);
        assert_eq!(p.next_u64(), 0xa0b6_259b_0b18_7d87);
    }

    #[test]
    fn below_is_in_range_and_rejects_bias() {
        let mut c = ChaCha8::new(7);
        for n in [1, 2, 3, 10, u64::MAX] {
            for _ in 0..50 {
                assert!(below(n, || c.next_u64()) < n);
            }
        }
        // 2**63 + 1 rejects about half the draws.
        let mut calls = 0;
        let mut c = ChaCha8::new(1);
        for _ in 0..100 {
            below((1 << 63) + 1, || {
                calls += 1;
                c.next_u64()
            });
        }
        assert!(calls > 100);
    }
}
