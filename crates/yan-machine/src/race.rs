//! 6.7 item 1: data races over the arms of a `par`, by vector clocks (D5 of the `run --ub`
//! design). Each arm is a logical thread forked from its parent and joined back; the machine runs
//! arms one at a time, but a conflict is judged by happens-before, so it does not depend on the
//! schedule. Shadow state is kept per byte, only while an arm runs: an access outside every `par`
//! is ordered before any later arm by the fork and after any earlier one by the join.
//!
//! A `par concurrent` block is the same fork and join, but its tasks interleave: the scheduler
//! picks the task that runs next, and `switch` moves the current thread to it. Blocks are kept by
//! scheduler handle, not on the region stack, because a task that yields leaves its inner block
//! live.

use std::collections::{HashMap, VecDeque};

use yan_mir::MemOrder;

type Tid = usize;
type Epoch = (Tid, u32);

#[derive(Default)]
struct Cell {
    write: Option<Epoch>,
    reads: Vec<Epoch>,
    /// The same for atomic accesses. Atomics never race one another, only plain accesses.
    awrite: Option<Epoch>,
    areads: Vec<Epoch>,
}

/// What an atomic access does to its word.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Read,
    Write,
    /// A read-modify-write, which continues the release sequence it reads.
    Rmw,
}

/// Per-thread fence state (D5 item 3): the clocks relaxed loads have seen and a later acquire
/// fence takes, and the clock the last release fence saved for later relaxed writes to publish.
#[derive(Default)]
struct Fences {
    acquire: Vec<u32>,
    release: Vec<u32>,
}

struct Region {
    parent: Tid,
    arms: Vec<Tid>,
}

/// The release/acquire state of one channel (D5): the clock each queued message was sent under,
/// the clock each receive left under (a send into a full buffer acquires the receive that made
/// room), and the clock of the dropped senders (a receive that finds the channel closed acquires
/// it).
#[derive(Default)]
struct Link {
    cap: u64,
    sent: u64,
    msgs: VecDeque<Vec<u32>>,
    freed: VecDeque<Vec<u32>>,
    freed_base: u64,
    closed: Vec<u32>,
    /// Receivers parked on an unbuffered channel, oldest first, with the clock each arrived under.
    parked: VecDeque<(Tid, Vec<u32>)>,
}

struct Block {
    parent: Tid,
    tasks: Vec<Tid>,
}

/// An actor is a logical thread of its own: it starts under its spawner's clock, and each message
/// it runs first acquires the clock its sender left.
struct Actor {
    tid: Tid,
    mail: VecDeque<(u64, Vec<u32>)>,
}

pub struct Race {
    /// Clock of each thread; thread 0 is the program's own.
    clocks: Vec<Vec<u32>>,
    /// The `par` arm index of each thread, for reports.
    arm: Vec<usize>,
    cur: Tid,
    regions: Vec<Region>,
    blocks: HashMap<u64, Block>,
    links: HashMap<u64, Link>,
    actors: HashMap<u64, Actor>,
    /// The thread each running handler interrupted.
    resume: Vec<Tid>,
    shadow: HashMap<u64, Cell>,
    /// The release clock of each atomic word.
    sync: HashMap<u64, Vec<u32>>,
    fences: HashMap<Tid, Fences>,
    /// The one clock every sequentially consistent operation and fence joins and publishes.
    sc: Vec<u32>,
    /// The last [`HISTORY`] stores of each atomic word, oldest first (RFC 0090 D3).
    hist: HashMap<u64, History>,
    /// The newest store of a word each thread has read or written, by modification-order
    /// position: a later load by that thread may not read an older one (read-read coherence).
    floor: HashMap<(Tid, u64), u64>,
    seed: u64,
    draws: u64,
}

/// Stores a load may still choose among; older ones are forgotten.
const HISTORY: usize = 4;

struct Store {
    value: u64,
    epoch: Epoch,
    /// The release clock this store published.
    sync: Vec<u32>,
}

#[derive(Default)]
struct History {
    /// Modification-order position of `stores[0]`.
    base: u64,
    stores: Vec<Store>,
}

fn splitmix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn ordered(clock: &[u32], (t, c): Epoch) -> bool {
    clock.get(t).copied().unwrap_or(0) >= c
}

/// The earlier access `me` is not ordered after that conflicts with a read or write: a plain
/// one, and for a plain access also an atomic one. Atomic accesses do not conflict.
fn conflict(cell: &Cell, me: &[u32], write: bool, atomic: bool) -> Option<(&'static str, Tid)> {
    let unordered = |e: &&Epoch| !ordered(me, **e);
    let find = |w: &Option<Epoch>, rs: &[Epoch], names: [&'static str; 2]| {
        w.iter()
            .find(unordered)
            .map(|e| (names[0], e.0))
            .or_else(|| {
                rs.iter()
                    .find(|r| write && unordered(r))
                    .map(|r| (names[1], r.0))
            })
    };
    find(&cell.write, &cell.reads, ["write", "read"]).or_else(|| {
        (!atomic)
            .then(|| find(&cell.awrite, &cell.areads, ["atomic write", "atomic read"]))
            .flatten()
    })
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
    #[cfg(test)]
    pub fn new() -> Race {
        Race::seeded(0)
    }

    /// The seed fixes which store each load that may read an old one reads (R1101).
    pub fn seeded(seed: u64) -> Race {
        Race {
            hist: HashMap::new(),
            floor: HashMap::new(),
            seed,
            draws: 0,
            clocks: vec![vec![1]],
            arm: vec![0],
            cur: 0,
            regions: Vec::new(),
            blocks: HashMap::new(),
            links: HashMap::new(),
            actors: HashMap::new(),
            resume: Vec::new(),
            shadow: HashMap::new(),
            sync: HashMap::new(),
            fences: HashMap::new(),
            sc: Vec::new(),
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
        self.merge(r.parent, &r.arms);
    }

    fn merge(&mut self, parent: Tid, tasks: &[Tid]) {
        let mut c = std::mem::take(&mut self.clocks[parent]);
        for t in tasks {
            join(&mut c, &self.clocks[*t]);
        }
        self.clocks[parent] = c;
        self.cur = parent;
        self.tick(parent);
        if self.idle() {
            self.shadow.clear();
            self.sync.clear();
            self.fences.clear();
            self.sc.clear();
        }
    }

    pub fn chan_new(&mut self, ch: u64, cap: u64) {
        self.links.insert(
            ch,
            Link {
                cap,
                ..Link::default()
            },
        );
    }

    /// This thread's receive found nothing and waits. A send on an unbuffered channel only goes
    /// through once a receiver waits, so it follows the receiver's arrival; a receive that
    /// begins before its send is therefore ordered before what the sender does next.
    pub fn chan_recv_park(&mut self, ch: u64) {
        let Some(l) = self.links.get_mut(&ch) else {
            return;
        };
        l.parked.retain(|(t, _)| *t != self.cur);
        l.parked
            .push_back((self.cur, self.clocks[self.cur].clone()));
    }

    /// This thread no longer waits on `ch`.
    pub fn chan_unpark(&mut self, ch: u64) {
        if let Some(l) = self.links.get_mut(&ch) {
            l.parked.retain(|(t, _)| *t != self.cur);
        }
    }

    /// A send went through: it releases this thread's clock, and first acquires the receive that
    /// made room for it when the buffer had been full.
    pub fn chan_send(&mut self, ch: u64) {
        let Some(l) = self.links.get_mut(&ch) else {
            return;
        };
        if l.cap == 0
            && let Some((_, c)) = l.parked.pop_front()
        {
            join(&mut self.clocks[self.cur], &c);
        }
        let n = l.sent;
        l.sent += 1;
        let room = (n >= l.cap).then(|| {
            let i = n - l.cap;
            while l.freed_base < i {
                l.freed.pop_front();
                l.freed_base += 1;
            }
            l.freed.front().cloned()
        });
        if let Some(Some(c)) = room {
            join(&mut self.clocks[self.cur], &c);
        }
        l.msgs.push_back(self.clocks[self.cur].clone());
        self.tick(self.cur);
    }

    /// A receive took a message, or found the channel closed.
    pub fn chan_recv(&mut self, ch: u64, closed: bool) {
        let Some(l) = self.links.get_mut(&ch) else {
            return;
        };
        l.parked.retain(|(t, _)| *t != self.cur);
        if closed {
            join(&mut self.clocks[self.cur], &l.closed);
            return;
        }
        if let Some(c) = l.msgs.pop_front() {
            join(&mut self.clocks[self.cur], &c);
        }
        l.freed.push_back(self.clocks[self.cur].clone());
        self.tick(self.cur);
    }

    pub fn chan_sender_dropped(&mut self, ch: u64) {
        if let Some(l) = self.links.get_mut(&ch) {
            join(&mut l.closed, &self.clocks[self.cur]);
            self.tick(self.cur);
        }
    }

    pub fn actor_spawn(&mut self, id: u64) {
        let t = self.clocks.len();
        let mut c = self.clocks[self.cur].clone();
        c.resize(t + 1, 0);
        c[t] = 1;
        self.clocks.push(c);
        self.arm.push(usize::MAX);
        self.actors.insert(
            id,
            Actor {
                tid: t,
                mail: VecDeque::new(),
            },
        );
        self.tick(self.cur);
    }

    /// A message with arguments `key` was queued for actor `id`: it releases this thread's clock.
    pub fn actor_send(&mut self, id: u64, key: u64) {
        let c = self.clocks[self.cur].clone();
        if let Some(a) = self.actors.get_mut(&id) {
            a.mail.push_back((key, c));
            self.tick(self.cur);
        }
    }

    /// Actor `id` starts the message with arguments `key`; what it runs belongs to the actor's
    /// thread until `actor_end`.
    pub fn actor_begin(&mut self, id: u64, key: Option<u64>) {
        let Some(a) = self.actors.get_mut(&id) else {
            return;
        };
        let sent = key.and_then(|k| {
            let at = a.mail.iter().position(|(m, _)| *m == k)?;
            // Messages before it were dropped by the mailbox's overflow policy.
            a.mail.drain(..at);
            a.mail.pop_front().map(|(_, c)| c)
        });
        let t = a.tid;
        if let Some(c) = sent {
            join(&mut self.clocks[t], &c);
        }
        self.resume.push(self.cur);
        self.cur = t;
    }

    pub fn actor_end(&mut self) {
        if let Some(t) = self.resume.pop() {
            self.tick(self.cur);
            self.cur = t;
        }
    }

    fn idle(&self) -> bool {
        self.regions.is_empty() && self.blocks.is_empty()
    }

    /// `par concurrent` block `h` of `n` tasks begins: each task is forked from the current thread.
    pub fn block_begin(&mut self, h: u64, n: usize) {
        self.fork();
        let r = self.regions.pop().expect("fork pushed a region");
        let mut tasks = Vec::with_capacity(n);
        for k in 0..n {
            self.regions.push(Region {
                parent: r.parent,
                arms: Vec::new(),
            });
            self.enter(k as u32);
            tasks.push(self.cur);
            self.regions.pop();
        }
        self.cur = r.parent;
        self.blocks.insert(
            h,
            Block {
                parent: r.parent,
                tasks,
            },
        );
    }

    /// The scheduler picked task `i` of block `h`; any index past the tasks hands control back to
    /// the block's parent (all done, a yield, or an idle actor).
    pub fn block_switch(&mut self, h: u64, i: usize) {
        if let Some(b) = self.blocks.get(&h) {
            self.cur = b.tasks.get(i).copied().unwrap_or(b.parent);
        }
    }

    pub fn block_end(&mut self, h: u64) {
        if let Some(b) = self.blocks.remove(&h) {
            self.merge(b.parent, &b.tasks);
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
        } else if self.arm[t] == usize::MAX {
            "an actor".into()
        } else {
            format!("`par` arm {}", self.arm[t])
        }
    }

    /// Records an 8-byte access at `addr`; the message of the race it makes, if any.
    pub fn access(&mut self, addr: u64, write: bool) -> Option<String> {
        if self.idle() {
            return None;
        }
        let me = &self.clocks[self.cur];
        let now = (self.cur, me[self.cur]);
        for a in addr..addr + 8 {
            let cell = self.shadow.entry(a).or_default();
            if let Some((kind, t)) = conflict(cell, me, write, false) {
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
                cell.awrite = None;
                cell.areads.clear();
            } else {
                cell.reads.retain(|r| r.0 != self.cur);
                cell.reads.push(now);
            }
        }
        None
    }
}

impl Race {
    fn clock_of(&mut self, t: Tid) -> &mut Vec<u32> {
        &mut self.clocks[t]
    }

    /// An atomic access of the 8-byte word at `addr` (D5): the message of the race it makes
    /// with an unordered plain access, if any, and otherwise its release/acquire edges. A
    /// failed compare-exchange is a `Read` under its failure ordering.
    #[cfg(test)]
    pub fn atomic(&mut self, addr: u64, kind: Kind, order: MemOrder) -> Option<String> {
        self.atomic_value(addr, kind, order, false, 0, None).err()
    }

    /// [`Race::atomic`] on a word holding `old` that the access leaves as `new`, if it writes
    /// one. It returns the word the access reads: for a plain `load` (`stale_ok`) any store
    /// coherence and happens-before allow, drawn by seed, otherwise `old` (RFC 0090 D3).
    pub fn atomic_value(
        &mut self,
        addr: u64,
        kind: Kind,
        order: MemOrder,
        stale_ok: bool,
        old: u64,
        new: Option<u64>,
    ) -> Result<u64, String> {
        if self.idle() {
            if new.is_some() {
                self.hist.remove(&addr);
            }
            return Ok(old);
        }
        let cur = self.cur;
        let write = kind != Kind::Read;
        let me = &self.clocks[cur];
        for a in addr..addr + 8 {
            if let Some(cell) = self.shadow.get(&a)
                && let Some((prior, t)) = conflict(cell, me, write, true)
            {
                return Err(format!(
                    "atomic {} of 8 bytes at {addr:#x} by {} races with an earlier plain {prior} by {}",
                    if write { "write" } else { "read" },
                    self.who(cur),
                    self.who(t),
                ));
            }
        }
        let acquire = !matches!(order, MemOrder::Relaxed | MemOrder::Release);
        let release = !matches!(order, MemOrder::Relaxed | MemOrder::Acquire);
        let seq = order == MemOrder::SeqCst;
        if seq {
            let sc = self.sc.clone();
            join(self.clock_of(cur), &sc);
        }
        if new.is_some() && !self.hist.contains_key(&addr) {
            let sync = self.sync.get(&addr).cloned().unwrap_or_default();
            let initial = Store {
                value: old,
                epoch: (0, 0),
                sync,
            };
            self.hist.insert(
                addr,
                History {
                    base: 0,
                    stores: vec![initial],
                },
            );
        }
        let chosen = (stale_ok && kind == Kind::Read && order != MemOrder::SeqCst)
            .then(|| self.choose(addr))
            .flatten();
        let mut value = old;
        let seen = match &chosen {
            Some((v, sync)) => {
                value = *v;
                sync.clone()
            }
            None => self.sync.get(&addr).cloned().unwrap_or_default(),
        };
        if kind != Kind::Write {
            if acquire {
                join(self.clock_of(cur), &seen);
            } else {
                join(&mut self.fences.entry(cur).or_default().acquire, &seen);
            }
        }
        let now = (cur, self.clocks[cur][cur]);
        for a in addr..addr + 8 {
            let cell = self.shadow.entry(a).or_default();
            if write {
                cell.awrite = Some(now);
                cell.areads.clear();
            } else {
                cell.areads.retain(|r| r.0 != cur);
                cell.areads.push(now);
            }
        }
        if write {
            let mut out = if release {
                self.clocks[cur].clone()
            } else {
                self.fences.entry(cur).or_default().release.clone()
            };
            // A store starts a new release sequence; a read-modify-write continues the one it read.
            if kind == Kind::Rmw {
                join(&mut out, &seen);
            }
            if let (Some(w), Some(h)) = (new, self.hist.get_mut(&addr)) {
                h.stores.push(Store {
                    value: w,
                    epoch: now,
                    sync: out.clone(),
                });
                if h.stores.len() > HISTORY {
                    h.stores.remove(0);
                    h.base += 1;
                }
                self.floor
                    .insert((cur, addr), h.base + h.stores.len() as u64 - 1);
            }
            self.sync.insert(addr, out);
        } else if chosen.is_none()
            && let Some(h) = self.hist.get(&addr)
        {
            self.floor
                .insert((cur, addr), h.base + h.stores.len() as u64 - 1);
        }
        if seq {
            let c = self.clocks[cur].clone();
            join(&mut self.sc, &c);
        }
        if write {
            self.tick(cur);
        }
        Ok(value)
    }

    /// The store a load by the current thread reads: any in the word's history that is not
    /// older than one the thread has read, and not hidden by a later store that happens before
    /// the load. `None` when the word has no history, or under seed 0, so the load reads the newest.
    fn choose(&mut self, addr: u64) -> Option<(u64, Vec<u32>)> {
        if self.seed == 0 {
            return None;
        }
        let cur = self.cur;
        let h = self.hist.get(&addr)?;
        let me = &self.clocks[cur];
        let floor = self.floor.get(&(cur, addr)).copied().unwrap_or(0);
        let visible: Vec<usize> = (0..h.stores.len())
            .filter(|&i| h.base + i as u64 >= floor)
            .filter(|&i| !h.stores[i + 1..].iter().any(|s| ordered(me, s.epoch)))
            .collect();
        self.draws += 1;
        let pick =
            visible[(splitmix(self.seed ^ splitmix(self.draws)) % visible.len() as u64) as usize];
        let s = &h.stores[pick];
        let out = (s.value, s.sync.clone());
        self.floor.insert((cur, addr), h.base + pick as u64);
        Some(out)
    }

    /// `fence(order)`: an acquire fence takes the clocks the thread's relaxed loads saw, a
    /// release fence saves the thread's clock for its later relaxed writes, and a sequentially
    /// consistent one also passes through the global clock.
    pub fn fence(&mut self, order: MemOrder) {
        if self.idle() {
            return;
        }
        let cur = self.cur;
        let acquire = !matches!(order, MemOrder::Relaxed | MemOrder::Release);
        let release = !matches!(order, MemOrder::Relaxed | MemOrder::Acquire);
        if acquire {
            let seen = self.fences.entry(cur).or_default().acquire.clone();
            join(self.clock_of(cur), &seen);
        }
        if order == MemOrder::SeqCst {
            let sc = self.sc.clone();
            join(self.clock_of(cur), &sc);
            let c = self.clocks[cur].clone();
            join(&mut self.sc, &c);
        }
        if release {
            self.fences.entry(cur).or_default().release = self.clocks[cur].clone();
            self.tick(cur);
        }
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
    fn tasks_race_in_any_interleaving_and_join_orders() {
        let mut r = Race::new();
        r.block_begin(0, 2);
        r.block_switch(0, 0);
        assert!(r.access(0x10, true).is_none());
        r.block_switch(0, 1);
        assert!(r.access(0x10, true).is_some());
        r.block_switch(0, 2);
        r.block_end(0);
        r.block_begin(0, 2);
        r.block_switch(0, 1);
        assert!(r.access(0x10, true).is_none());
        r.block_switch(0, 0);
        assert!(r.access(0x20, true).is_none());
        r.block_switch(0, 1);
        assert!(r.access(0x10, false).is_none());
    }

    #[test]
    fn a_channel_orders_send_before_receive_and_close_before_closed() {
        let mut r = Race::new();
        r.chan_new(1, 1);
        r.block_begin(0, 2);
        r.block_switch(0, 0);
        assert!(r.access(0x10, true).is_none());
        r.chan_send(1);
        r.chan_sender_dropped(1);
        r.block_switch(0, 1);
        r.chan_recv(1, false);
        assert!(r.access(0x10, false).is_none());
        r.chan_recv(1, true);
        assert!(r.access(0x10, true).is_none());
        let mut r = Race::new();
        r.chan_new(1, 1);
        r.block_begin(0, 2);
        r.block_switch(0, 0);
        assert!(r.access(0x10, true).is_none());
        r.block_switch(0, 1);
        assert!(r.access(0x10, false).is_some());
    }

    #[test]
    fn a_message_orders_the_sender_before_the_handler_and_actors_run_serially() {
        let mut r = Race::new();
        r.actor_spawn(7);
        r.block_begin(0, 2);
        r.block_switch(0, 0);
        assert!(r.access(0x10, true).is_none());
        r.actor_send(7, 100);
        r.block_switch(0, 1);
        r.actor_begin(7, Some(100));
        assert!(r.access(0x10, true).is_none());
        r.actor_end();
        r.actor_send(7, 0);
        r.actor_begin(7, Some(0));
        assert!(r.access(0x10, false).is_none());
        r.actor_end();
        // An access by another task that never received from the actor races with it.
        assert!(r.access(0x10, true).is_some());
    }

    #[test]
    fn an_unbuffered_send_follows_the_receiver_that_was_waiting() {
        let mut r = Race::new();
        r.chan_new(1, 0);
        r.block_begin(0, 2);
        r.block_switch(0, 0);
        assert!(r.access(0x10, true).is_none());
        r.chan_recv_park(1);
        r.block_switch(0, 1);
        r.chan_send(1);
        assert!(r.access(0x10, true).is_none());
        // Without the wait the sender is unordered with the receiver's earlier write.
        let mut r = Race::new();
        r.chan_new(1, 0);
        r.block_begin(0, 2);
        r.block_switch(0, 0);
        assert!(r.access(0x10, true).is_none());
        r.block_switch(0, 1);
        r.chan_send(1);
        assert!(r.access(0x10, true).is_some());
    }

    #[test]
    fn a_send_into_a_full_buffer_follows_the_receive_that_made_room() {
        let mut r = Race::new();
        r.chan_new(1, 1);
        r.block_begin(0, 2);
        r.block_switch(0, 0);
        r.chan_send(1);
        r.block_switch(0, 1);
        assert!(r.access(0x10, true).is_none());
        r.chan_recv(1, false);
        r.block_switch(0, 0);
        r.chan_send(1);
        assert!(r.access(0x10, true).is_none());
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

    use MemOrder::*;

    /// Task 0 writes plain data then `publish`es the flag; task 1 does `observe` on the flag and
    /// reads the data. The message of the race on the data read, if any.
    fn handoff(publish: impl FnOnce(&mut Race), observe: impl FnOnce(&mut Race)) -> Option<String> {
        let mut r = Race::new();
        r.fork();
        assert!(
            arm(&mut r, 0, |r| {
                assert!(r.access(0x10, true).is_none());
                publish(r);
                None
            })
            .is_none()
        );
        arm(&mut r, 1, |r| {
            observe(r);
            r.access(0x10, false)
        })
    }

    #[test]
    fn release_acquire_orders_plain_accesses() {
        let out = handoff(
            |r| assert!(r.atomic(0x20, Kind::Write, Release).is_none()),
            |r| assert!(r.atomic(0x20, Kind::Read, Acquire).is_none()),
        );
        assert!(out.is_none());
    }

    #[test]
    fn relaxed_on_either_side_leaves_the_plain_access_unordered() {
        assert!(
            handoff(
                |r| assert!(r.atomic(0x20, Kind::Write, Relaxed).is_none()),
                |r| assert!(r.atomic(0x20, Kind::Read, Acquire).is_none()),
            )
            .is_some()
        );
        assert!(
            handoff(
                |r| assert!(r.atomic(0x20, Kind::Write, Release).is_none()),
                |r| assert!(r.atomic(0x20, Kind::Read, Relaxed).is_none()),
            )
            .is_some()
        );
    }

    #[test]
    fn a_relaxed_read_modify_write_continues_the_release_sequence_but_a_store_ends_it() {
        let rmw_between = |kind| {
            let mut r = Race::new();
            r.fork();
            arm(&mut r, 0, |r| {
                r.access(0x10, true);
                r.atomic(0x20, Kind::Write, Release);
                None
            });
            arm(&mut r, 1, |r| r.atomic(0x20, kind, Relaxed));
            arm(&mut r, 2, |r| {
                assert!(r.atomic(0x20, Kind::Read, Acquire).is_none());
                r.access(0x10, false)
            })
        };
        assert!(rmw_between(Kind::Rmw).is_none());
        assert!(rmw_between(Kind::Write).is_some());
    }

    #[test]
    fn fences_make_relaxed_accesses_synchronize() {
        let out = handoff(
            |r| {
                r.fence(Release);
                assert!(r.atomic(0x20, Kind::Write, Relaxed).is_none());
            },
            |r| {
                assert!(r.atomic(0x20, Kind::Read, Relaxed).is_none());
                r.fence(Acquire);
            },
        );
        assert!(out.is_none());
        let missing = handoff(
            |r| {
                r.fence(Release);
                assert!(r.atomic(0x20, Kind::Write, Relaxed).is_none());
            },
            |r| assert!(r.atomic(0x20, Kind::Read, Relaxed).is_none()),
        );
        assert!(missing.is_some());
    }

    #[test]
    fn sequentially_consistent_operations_order_through_one_clock() {
        let out = handoff(
            |r| assert!(r.atomic(0x20, Kind::Write, SeqCst).is_none()),
            |r| assert!(r.atomic(0x30, Kind::Write, SeqCst).is_none()),
        );
        assert!(out.is_none());
    }

    #[test]
    fn atomics_race_with_unordered_plain_accesses_not_with_each_other() {
        let mut r = Race::new();
        r.fork();
        assert!(arm(&mut r, 0, |r| r.atomic(0x20, Kind::Write, Relaxed)).is_none());
        assert!(arm(&mut r, 1, |r| r.atomic(0x20, Kind::Rmw, Relaxed)).is_none());
        assert!(arm(&mut r, 2, |r| r.atomic(0x20, Kind::Read, Relaxed)).is_none());
        let plain_read = arm(&mut r, 3, |r| r.access(0x20, false)).expect("a race");
        assert!(plain_read.contains("atomic write"), "{plain_read}");
        let plain_write = arm(&mut r, 4, |r| r.access(0x20, true));
        assert!(plain_write.is_some());
        let mut r = Race::new();
        r.fork();
        assert!(arm(&mut r, 0, |r| r.access(0x20, true)).is_none());
        let out = arm(&mut r, 1, |r| r.atomic(0x20, Kind::Read, Relaxed)).expect("a race");
        assert!(out.contains("plain write"), "{out}");
    }

    /// What a relaxed load in the second arm reads after the first stored 1 over 0.
    fn read_after_store(seed: u64, store: MemOrder, load: MemOrder) -> u64 {
        let mut r = Race::seeded(seed);
        r.fork();
        r.enter(0);
        r.atomic_value(0x20, Kind::Write, store, false, 0, Some(1))
            .unwrap();
        r.exit();
        r.enter(1);
        let v = r
            .atomic_value(0x20, Kind::Read, load, true, 1, None)
            .unwrap();
        r.exit();
        v
    }

    #[test]
    fn a_seeded_relaxed_load_may_read_the_older_store() {
        use MemOrder::*;
        let seen: Vec<u64> = (1..=16)
            .map(|s| read_after_store(s, Relaxed, Relaxed))
            .collect();
        assert!(seen.contains(&0) && seen.contains(&1), "{seen:?}");
        assert_eq!(
            read_after_store(0, Relaxed, Relaxed),
            1,
            "seed 0 reads the newest"
        );
        assert_eq!(
            read_after_store(3, Relaxed, Relaxed),
            read_after_store(3, Relaxed, Relaxed)
        );
    }

    #[test]
    fn a_sequentially_consistent_load_reads_the_newest_store() {
        use MemOrder::*;
        assert!((1..=16).all(|s| read_after_store(s, SeqCst, SeqCst) == 1));
    }

    #[test]
    fn a_load_never_reads_older_than_one_the_thread_has_read() {
        use MemOrder::*;
        for seed in 1..=16 {
            let mut r = Race::seeded(seed);
            r.fork();
            r.enter(0);
            r.atomic_value(0x20, Kind::Write, Relaxed, false, 0, Some(1))
                .unwrap();
            r.atomic_value(0x20, Kind::Write, Relaxed, false, 1, Some(2))
                .unwrap();
            r.exit();
            r.enter(1);
            let mut last = 0;
            for _ in 0..8 {
                let v = r
                    .atomic_value(0x20, Kind::Read, Relaxed, true, 2, None)
                    .unwrap();
                assert!(v >= last, "seed {seed}: {v} after {last}");
                last = v;
            }
            r.exit();
        }
    }
}
