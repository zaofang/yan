//! 6.7 item 1: data races over the arms of a `par`, by vector clocks (D5 of the `run --ub`
//! design). Each arm is a logical thread forked from its parent and joined back; the machine runs
//! arms one at a time, but a conflict is judged by happens-before, so it does not depend on the
//! schedule. Shadow state is kept per byte, only while an arm runs: an access outside every `par`
//! is ordered before any later arm by the fork and after any earlier one by the join.

use std::collections::HashMap;

type Tid = usize;
type Epoch = (Tid, u32);

#[derive(Default)]
struct Cell {
    write: Option<Epoch>,
    reads: Vec<Epoch>,
}

struct Region {
    parent: Tid,
    arms: Vec<Tid>,
}

pub struct Race {
    /// Clock of each thread; thread 0 is the program's own.
    clocks: Vec<Vec<u32>>,
    /// The `par` arm index of each thread, for reports.
    arm: Vec<usize>,
    cur: Tid,
    regions: Vec<Region>,
    shadow: HashMap<u64, Cell>,
}

fn ordered(clock: &[u32], (t, c): Epoch) -> bool {
    clock.get(t).copied().unwrap_or(0) >= c
}

fn join(into: &mut Vec<u32>, from: &[u32]) {
    if into.len() < from.len() {
        into.resize(from.len(), 0);
    }
    for (d, s) in into.iter_mut().zip(from) {
        *d = (*d).max(*s);
    }
}

impl Race {
    pub fn new() -> Race {
        Race {
            clocks: vec![vec![1]],
            arm: vec![0],
            cur: 0,
            regions: Vec::new(),
            shadow: HashMap::new(),
        }
    }

    pub fn depth(&self) -> usize {
        self.regions.len()
    }

    fn tick(&mut self, t: Tid) {
        let c = &mut self.clocks[t];
        if c.len() <= t {
            c.resize(t + 1, 0);
        }
        c[t] += 1;
    }

    pub fn fork(&mut self) {
        self.tick(self.cur);
        self.regions.push(Region {
            parent: self.cur,
            arms: Vec::new(),
        });
    }

    pub fn enter(&mut self, k: u32) {
        let Some(r) = self.regions.last_mut() else {
            return;
        };
        let t = self.clocks.len();
        let mut c = self.clocks[r.parent].clone();
        c.resize(t + 1, 0);
        c[t] = 1;
        self.clocks.push(c);
        self.arm.push(k as usize);
        r.arms.push(t);
        self.cur = t;
    }

    pub fn exit(&mut self) {
        if let Some(r) = self.regions.last() {
            self.cur = r.parent;
        }
    }

    pub fn join(&mut self) {
        let Some(r) = self.regions.pop() else {
            return;
        };
        let mut c = std::mem::take(&mut self.clocks[r.parent]);
        for t in &r.arms {
            join(&mut c, &self.clocks[*t]);
        }
        self.clocks[r.parent] = c;
        self.cur = r.parent;
        self.tick(r.parent);
        if self.regions.is_empty() {
            self.shadow.clear();
        }
    }

    /// Joins the regions above `depth`, as a frame leaving them early does.
    pub fn unwind(&mut self, depth: usize) {
        while self.regions.len() > depth {
            self.join();
        }
    }

    fn who(&self, t: Tid) -> String {
        if t == 0 {
            "the program".into()
        } else {
            format!("`par` arm {}", self.arm[t])
        }
    }

    /// Records an 8-byte access at `addr`; the message of the race it makes, if any.
    pub fn access(&mut self, addr: u64, write: bool) -> Option<String> {
        if self.regions.is_empty() {
            return None;
        }
        let me = &self.clocks[self.cur];
        let now = (self.cur, me[self.cur]);
        for a in addr..addr + 8 {
            let cell = self.shadow.entry(a).or_default();
            let prior = cell
                .write
                .filter(|w| !ordered(me, *w))
                .map(|w| ("write", w))
                .or_else(|| {
                    cell.reads
                        .iter()
                        .find(|r| write && !ordered(me, **r))
                        .map(|r| ("read", *r))
                });
            if let Some((kind, (t, _))) = prior {
                return Some(format!(
                    "{} of 8 bytes at {addr:#x} by {} races with an earlier {kind} by {}",
                    if write { "write" } else { "read" },
                    self.who(self.cur),
                    self.who(t),
                ));
            }
            if write {
                cell.write = Some(now);
                cell.reads.clear();
            } else {
                cell.reads.retain(|r| r.0 != self.cur);
                cell.reads.push(now);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arm(r: &mut Race, k: u32, body: impl FnOnce(&mut Race) -> Option<String>) -> Option<String> {
        r.enter(k);
        let out = body(r);
        r.exit();
        out
    }

    #[test]
    fn siblings_race_on_writes_and_read_write() {
        let mut r = Race::new();
        r.fork();
        assert!(arm(&mut r, 0, |r| r.access(0x10, true)).is_none());
        assert!(arm(&mut r, 1, |r| r.access(0x14, true)).is_some());
        let mut r = Race::new();
        r.fork();
        assert!(arm(&mut r, 0, |r| r.access(0x10, false)).is_none());
        assert!(arm(&mut r, 1, |r| r.access(0x10, true)).is_some());
    }

    #[test]
    fn reads_and_disjoint_bytes_do_not_race() {
        let mut r = Race::new();
        r.fork();
        assert!(arm(&mut r, 0, |r| r.access(0x10, false)).is_none());
        assert!(arm(&mut r, 1, |r| r.access(0x10, false)).is_none());
        let mut r = Race::new();
        r.fork();
        assert!(arm(&mut r, 0, |r| r.access(0x10, true)).is_none());
        assert!(arm(&mut r, 1, |r| r.access(0x18, true)).is_none());
    }

    #[test]
    fn fork_and_join_order_accesses() {
        let mut r = Race::new();
        r.fork();
        arm(&mut r, 0, |r| r.access(0x10, true));
        r.join();
        r.fork();
        assert!(arm(&mut r, 0, |r| r.access(0x10, true)).is_none());
        assert!(arm(&mut r, 1, |r| r.access(0x10, true)).is_some());
    }

    #[test]
    fn nested_arms_inherit_the_parent_arm() {
        let mut r = Race::new();
        r.fork();
        r.enter(0);
        assert!(r.access(0x10, true).is_none());
        r.fork();
        assert!(arm(&mut r, 0, |r| r.access(0x10, true)).is_none());
        r.join();
        r.exit();
        assert!(arm(&mut r, 1, |r| r.access(0x10, true)).is_some());
    }
}
