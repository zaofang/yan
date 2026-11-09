//! One dataflow analysis for moves, last uses and drops (RFC 0054).
//!
//! A client lowers its own IR to a [`Graph`]: basic blocks of [`Event`]s joined by edges. Branches,
//! loop back edges, `break`, `continue`, `match` arms and fall-through are all plain edges; `return`,
//! `fail` and `?` exits are blocks with no successors. Bindings are identified by [`Binding`], assigned
//! at definition, never by name. [`Graph::solve`] computes the three facts to a fixed point.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Identity of one definition. A shadowing `let` or a loop re-bind is a new binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Binding(pub u32);

/// Field of a record, as numbered by the client.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Field(pub u32);

/// Opaque client tag carried by a read, for diagnostics (the witness of a later use).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tag(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Block(pub u32);

/// What a statement does to a binding. A statement is a sequence of events in evaluation order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// Reads the binding without consuming it.
    Read(Binding, Tag),
    /// Reads the binding and gives up ownership. The binding is then moved.
    Move(Binding, Tag),
    /// Moves one field out of the record held by the binding, leaving it owned with a hole.
    Take(Binding, Field, Tag),
    /// Introduces the binding, owned and unmoved. A re-definition clears moves and holes.
    Define(Binding),
}

/// Position before the event at `index` of `block`; `index == len` is the end of the block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    pub block: Block,
    pub index: usize,
}

/// A part of a binding: the whole value, or one field of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Place {
    Whole(Binding),
    Field(Binding, Field),
}

impl Place {
    fn binding(self) -> Binding {
        match self {
            Place::Whole(b) | Place::Field(b, _) => b,
        }
    }
}

#[derive(Default)]
struct BlockData {
    events: Vec<Event>,
    succs: Vec<Block>,
}

#[derive(Default)]
pub struct Graph {
    blocks: Vec<BlockData>,
}

impl Graph {
    /// A graph with one empty entry block, `Block(0)`.
    pub fn new() -> Self {
        let mut g = Graph::default();
        g.block();
        g
    }

    pub const ENTRY: Block = Block(0);

    pub fn block(&mut self) -> Block {
        self.blocks.push(BlockData::default());
        Block(self.blocks.len() as u32 - 1)
    }

    pub fn push(&mut self, block: Block, event: Event) {
        self.blocks[block.0 as usize].events.push(event);
    }

    pub fn edge(&mut self, from: Block, to: Block) {
        let succs = &mut self.blocks[from.0 as usize].succs;
        if !succs.contains(&to) {
            succs.push(to);
        }
    }

    pub fn len(&self, block: Block) -> usize {
        self.blocks[block.0 as usize].events.len()
    }

    /// Blocks reachable from the entry with no successors: where the function returns or fails.
    pub fn exits(&self) -> Vec<Block> {
        (0..self.blocks.len() as u32)
            .map(Block)
            .filter(|b| self.blocks[b.0 as usize].succs.is_empty())
            .collect()
    }

    fn preds(&self) -> Vec<Vec<Block>> {
        let mut preds = vec![Vec::new(); self.blocks.len()];
        for (i, b) in self.blocks.iter().enumerate() {
            for s in &b.succs {
                preds[s.0 as usize].push(Block(i as u32));
            }
        }
        preds
    }

    pub fn solve(&self) -> Solved<'_> {
        let preds = self.preds();
        let reachable = self.reachable();
        Solved {
            graph: self,
            moved_in: self.forward(&preds, &reachable, Join::Union, moved_step),
            owned_may_in: self.forward(&preds, &reachable, Join::Union, owned_step),
            owned_must_in: self.forward(&preds, &reachable, Join::Intersect, owned_step),
        }
    }

    fn reachable(&self) -> Vec<bool> {
        let mut seen = vec![false; self.blocks.len()];
        let mut stack = vec![Graph::ENTRY];
        while let Some(b) = stack.pop() {
            if std::mem::replace(&mut seen[b.0 as usize], true) {
                continue;
            }
            stack.extend(&self.blocks[b.0 as usize].succs);
        }
        seen
    }

    /// Forward worklist. `None` is the top element (no path seen yet), only meaningful for
    /// intersection joins; unreachable blocks stay `None`.
    fn forward(
        &self,
        preds: &[Vec<Block>],
        reachable: &[bool],
        join: Join,
        step: fn(&mut BTreeSet<Place>, Event),
    ) -> Vec<Option<BTreeSet<Place>>> {
        let n = self.blocks.len();
        let mut inn: Vec<Option<BTreeSet<Place>>> = vec![None; n];
        inn[0] = Some(BTreeSet::new());
        let mut work: VecDeque<usize> = (0..n).filter(|&i| reachable[i]).collect();
        while let Some(i) = work.pop_front() {
            if i != 0 {
                let mut outs = preds[i].iter().filter_map(|p| self.out_of(&inn, *p, step));
                let joined = outs.next().map(|first| {
                    outs.fold(first, |acc, o| match join {
                        Join::Union => acc.union(&o).copied().collect(),
                        Join::Intersect => acc.intersection(&o).copied().collect(),
                    })
                });
                if joined == inn[i] {
                    continue;
                }
                inn[i] = joined;
            }
            work.extend(self.blocks[i].succs.iter().map(|s| s.0 as usize));
        }
        inn
    }

    fn out_of(
        &self,
        inn: &[Option<BTreeSet<Place>>],
        block: Block,
        step: fn(&mut BTreeSet<Place>, Event),
    ) -> Option<BTreeSet<Place>> {
        let mut set = inn[block.0 as usize].clone()?;
        for e in &self.blocks[block.0 as usize].events {
            step(&mut set, *e);
        }
        Some(set)
    }
}

#[derive(Clone, Copy)]
enum Join {
    Union,
    Intersect,
}

/// Maybe-moved: moved wholes and taken fields.
fn moved_step(set: &mut BTreeSet<Place>, e: Event) {
    match e {
        Event::Move(b, _) => {
            set.insert(Place::Whole(b));
        }
        Event::Take(b, f, _) => {
            set.insert(Place::Field(b, f));
        }
        Event::Define(b) => set.retain(|p| p.binding() != b),
        Event::Read(..) => {}
    }
}

/// Owned: bindings introduced and not moved whole. A taken field leaves the binding owned.
fn owned_step(set: &mut BTreeSet<Place>, e: Event) {
    match e {
        Event::Define(b) => {
            set.insert(Place::Whole(b));
        }
        Event::Move(b, _) => {
            set.remove(&Place::Whole(b));
        }
        Event::Read(..) | Event::Take(..) => {}
    }
}

pub struct Solved<'g> {
    graph: &'g Graph,
    moved_in: Vec<Option<BTreeSet<Place>>>,
    owned_may_in: Vec<Option<BTreeSet<Place>>>,
    owned_must_in: Vec<Option<BTreeSet<Place>>>,
}

impl Solved<'_> {
    fn at(
        &self,
        facts: &[Option<BTreeSet<Place>>],
        p: Point,
        step: fn(&mut BTreeSet<Place>, Event),
    ) -> Option<BTreeSet<Place>> {
        let mut set = facts[p.block.0 as usize].clone()?;
        for e in &self.graph.blocks[p.block.0 as usize].events[..p.index] {
            step(&mut set, *e);
        }
        Some(set)
    }

    /// Is `place` moved on some path reaching `at`? A whole binding counts as moved when any of its
    /// fields was taken; a field counts as moved when the whole was.
    pub fn maybe_moved(&self, at: Point, place: Place) -> bool {
        let Some(set) = self.at(&self.moved_in, at, moved_step) else {
            return false;
        };
        let b = place.binding();
        match place {
            Place::Whole(_) => set.iter().any(|p| p.binding() == b),
            Place::Field(_, _) => set.contains(&Place::Whole(b)) || set.contains(&place),
        }
    }

    /// Fields of `b` taken out on some path reaching `at`.
    pub fn holes(&self, at: Point, b: Binding) -> Vec<Field> {
        let set = self.at(&self.moved_in, at, moved_step).unwrap_or_default();
        set.iter()
            .filter_map(|p| match p {
                Place::Field(x, f) if *x == b => Some(*f),
                _ => None,
            })
            .collect()
    }

    /// Is `b` read or consumed on some path after the event at `at`, before a redefinition? A last-use
    /// move is a `Move` at which this is false.
    pub fn live_after(&self, at: Point, b: Binding) -> bool {
        self.witness_after(at, b).is_some()
    }

    /// The first read of `b` found by a breadth-first search after the event at `at`.
    pub fn witness_after(&self, at: Point, b: Binding) -> Option<Tag> {
        let events = &self.graph.blocks[at.block.0 as usize].events;
        let mut seen = BTreeSet::new();
        let mut queue = VecDeque::new();
        match scan(&events[(at.index + 1).min(events.len())..], b) {
            Scan::Found(t) => return Some(t),
            Scan::Killed => return None,
            Scan::Open => queue.push_back(at.block),
        }
        while let Some(blk) = queue.pop_front() {
            for s in &self.graph.blocks[blk.0 as usize].succs {
                if !seen.insert(*s) {
                    continue;
                }
                match scan(&self.graph.blocks[s.0 as usize].events, b) {
                    Scan::Found(t) => return Some(t),
                    Scan::Killed => {}
                    Scan::Open => queue.push_back(*s),
                }
            }
        }
        None
    }

    pub fn maybe_owned(&self, at: Point, b: Binding) -> bool {
        self.at(&self.owned_may_in, at, owned_step)
            .is_some_and(|s| s.contains(&Place::Whole(b)))
    }

    pub fn must_owned(&self, at: Point, b: Binding) -> bool {
        self.at(&self.owned_must_in, at, owned_step)
            .is_some_and(|s| s.contains(&Place::Whole(b)))
    }

    /// A drop flag is needed where ownership depends on the path.
    pub fn needs_flag(&self, at: Point, b: Binding) -> bool {
        self.maybe_owned(at, b) && !self.must_owned(at, b)
    }

    /// Bindings owned on some path at each exit, for `@linear` consumption (E412).
    pub fn owned_at_exits(&self) -> BTreeMap<Block, Vec<Binding>> {
        self.graph
            .exits()
            .into_iter()
            .filter_map(|b| {
                let end = Point {
                    block: b,
                    index: self.graph.len(b),
                };
                let set = self.at(&self.owned_may_in, end, owned_step)?;
                Some((b, set.iter().map(|p| p.binding()).collect()))
            })
            .collect()
    }
}

enum Scan {
    Found(Tag),
    Killed,
    Open,
}

fn scan(events: &[Event], b: Binding) -> Scan {
    for e in events {
        match *e {
            Event::Read(x, t) | Event::Move(x, t) | Event::Take(x, _, t) if x == b => {
                return Scan::Found(t);
            }
            Event::Define(x) if x == b => return Scan::Killed,
            _ => {}
        }
    }
    Scan::Open
}

#[cfg(test)]
mod tests;
