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
            blocks: HashMap::new(),
            links: HashMap::new(),
            actors: HashMap::new(),
            resume: Vec::new(),
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
}
