//! PCT scheduling (RFC 0065 D6, R999), shared by the interpreter and native code.

/// Probabilistic concurrency testing (Burckhardt et al., ASPLOS 2010): tasks get distinct random
/// priorities, the highest runnable one runs, and at each of `depth - 1` change points drawn from
/// the seed the task about to run drops below every initial priority.
pub struct Pct {
    state: u64,
    depth: i64,
    changes: Vec<u64>,
    step: u64,
}

impl Pct {
    pub fn new(seed: u64, depth: i64, steps: i64) -> Pct {
        let mut p = Pct {
            state: seed,
            depth,
            changes: Vec::new(),
            step: 0,
        };
        for _ in 1..depth {
            let at = 1 + p.next() % steps as u64;
            p.changes.push(at);
        }
        p
    }

    /// SplitMix64.
    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Distinct priorities for `n` new tasks, all above the change-point priorities.
    pub fn priorities(&mut self, n: usize) -> Vec<i64> {
        let mut p: Vec<i64> = (0..n as i64).map(|i| self.depth + i).collect();
        for i in (1..n).rev() {
            let j = (self.next() % (i as u64 + 1)) as usize;
            p.swap(i, j);
        }
        p
    }

    /// The index among `prios` of the task to run, `None` when nothing is runnable. A change point
    /// demotes the task chosen at that step to the priority of the change, then chooses again.
    pub fn pick(&mut self, prios: &mut [i64], runnable: &[bool]) -> Option<usize> {
        let best = |prios: &[i64]| {
            (0..prios.len())
                .filter(|i| runnable[*i])
                .max_by_key(|i| prios[*i])
        };
        let mut i = best(prios)?;
        self.step += 1;
        if let Some(k) = self.changes.iter().position(|c| *c == self.step) {
            prios[i] = k as i64;
            i = best(prios)?;
        }
        Some(i)
    }
}

#[cfg(test)]
mod tests {
    use super::Pct;

    fn run(seed: u64) -> Vec<Option<usize>> {
        let mut p = Pct::new(seed, 3, 8);
        let mut prios = p.priorities(3);
        (0..8).map(|_| p.pick(&mut prios, &[true; 3])).collect()
    }

    #[test]
    fn replays_from_the_seed() {
        assert_eq!(run(5), run(5));
        assert!((0..16).any(|s| run(s) != run(5)));
    }

    #[test]
    fn priorities_are_distinct_and_above_changes() {
        let mut p = Pct::new(1, 4, 10);
        let mut ps = p.priorities(5);
        ps.sort();
        assert_eq!(ps, vec![4, 5, 6, 7, 8]);
        assert_eq!(p.changes.len(), 3);
    }

    #[test]
    fn nothing_runnable_picks_nothing() {
        let mut p = Pct::new(1, 2, 10);
        assert_eq!(p.pick(&mut [2, 3], &[false, false]), None);
    }
}
