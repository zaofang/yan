//! One dataflow analysis for moves, liveness and ownership (RFC 0054).
//!
//! A client lowers its IR to a [`Cfg`] of nodes holding ordered [`Event`]s about bindings and
//! solves it once. A [`Binding`] is an id the client assigns at each definition, so a shadowing
//! `let` or a loop re-bind is a new binding. A [`Site`] is opaque to this crate; clients map it
//! to a span and give sites increasing values in source order, which `next_use` relies on.
//!
//! Facts: maybe-moved (forward, may), live (backward, may), owned (forward, may and must).

use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Binding(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Site(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Node(pub usize);

/// A field of a binding that can be moved out on its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Field(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// A (re)definition. An owned binding holds a resource that must be consumed or dropped.
    Def { binding: Binding, owned: bool },
    /// A read that leaves the value in place.
    Use { binding: Binding, site: Site },
    /// A read that consumes the whole value.
    Move { binding: Binding, site: Site },
    /// A read that takes one field out; the rest stays owned.
    MoveField {
        binding: Binding,
        field: Field,
        site: Site,
    },
    /// A drop at scope end or on a branch edge: ends ownership without reading.
    Drop { binding: Binding },
}

/// The position before event `event` of `node`; `event == len` is the end of the node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    pub node: Node,
    pub event: usize,
}

#[derive(Default)]
pub struct Cfg {
    events: Vec<Vec<Event>>,
    succs: Vec<Vec<Node>>,
}

impl Cfg {
    pub fn new() -> Self {
        Self::default()
    }

    /// The entry is node 0, the first one added.
    pub fn node(&mut self) -> Node {
        self.events.push(Vec::new());
        self.succs.push(Vec::new());
        Node(self.events.len() - 1)
    }

    /// Appends `e` to `n` and returns the point after it.
    pub fn push(&mut self, n: Node, e: Event) -> Point {
        self.events[n.0].push(e);
        Point {
            node: n,
            event: self.events[n.0].len(),
        }
    }

    pub fn edge(&mut self, from: Node, to: Node) {
        if !self.succs[from.0].contains(&to) {
            self.succs[from.0].push(to);
        }
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// The earliest read of `b` reachable from `p` before a redefinition: the witness for
    /// "used again".
    pub fn next_use(&self, b: Binding, p: Point) -> Option<Site> {
        let mut found: Option<Site> = None;
        let mut seen = vec![false; self.len()];
        let mut stack = Vec::new();
        let scan = |node: Node, from: usize, found: &mut Option<Site>| -> bool {
            for e in &self.events[node.0][from..] {
                match *e {
                    Event::Def { binding, .. } if binding == b => return false,
                    Event::Use { binding, site }
                    | Event::Move { binding, site }
                    | Event::MoveField { binding, site, .. }
                        if binding == b =>
                    {
                        *found = Some(found.map_or(site, |f| f.min(site)));
                        return false;
                    }
                    _ => {}
                }
            }
            true
        };
        if scan(p.node, p.event, &mut found) {
            stack.extend(self.succs[p.node.0].iter().copied());
        }
        while let Some(n) = stack.pop() {
            if std::mem::replace(&mut seen[n.0], true) {
                continue;
            }
            if scan(n, 0, &mut found) {
                stack.extend(self.succs[n.0].iter().copied());
            }
        }
        found
    }

    fn preds(&self) -> Vec<Vec<Node>> {
        let mut p = vec![Vec::new(); self.len()];
        for (i, ss) in self.succs.iter().enumerate() {
            for s in ss {
                p[s.0].push(Node(i));
            }
        }
        p
    }

    pub fn solve(&self) -> Solution<'_> {
        let preds = self.preds();
        let n = self.len();
        let mut moved_in: Vec<Moved> = vec![Moved::new(); n];
        let mut own_in: Vec<Option<Owned>> = vec![None; n];
        if n > 0 {
            own_in[0] = Some(Owned::default());
        }
        let mut live_out: Vec<BTreeSet<Binding>> = vec![BTreeSet::new(); n];

        loop {
            let mut changed = false;
            for i in 0..n {
                let mut m = Moved::new();
                let mut o: Option<Owned> = if i == 0 { Some(Owned::default()) } else { None };
                for p in &preds[i] {
                    let mut pm = moved_in[p.0].clone();
                    for e in &self.events[p.0] {
                        step_moved(&mut pm, e);
                    }
                    m.extend(pm);
                    if let Some(mut po) = own_in[p.0].clone() {
                        for e in &self.events[p.0] {
                            step_owned(&mut po, e);
                        }
                        o = Some(match o {
                            None => po,
                            Some(acc) => acc.join(&po),
                        });
                    }
                }
                if m != moved_in[i] {
                    moved_in[i] = m;
                    changed = true;
                }
                if o != own_in[i] {
                    own_in[i] = o;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        loop {
            let mut changed = false;
            for i in (0..n).rev() {
                let mut out = BTreeSet::new();
                for s in &self.succs[i] {
                    out.extend(live_before(&live_out[s.0], &self.events[s.0], 0));
                }
                if out != live_out[i] {
                    live_out[i] = out;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        Solution {
            cfg: self,
            moved_in,
            own_in,
            live_out,
        }
    }
}

/// A move and where it happened; `None` is the whole value.
type Moved = BTreeSet<(Binding, Option<Field>, Site)>;

/// May and must sets of owned bindings. A binding in `may` but not `must` is owned on some
/// paths only: dropping it needs a drop flag or an edge drop.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Owned {
    pub may: BTreeSet<Binding>,
    pub must: BTreeSet<Binding>,
    /// Fields taken out of a binding on every path on which it is still owned.
    pub holes: BTreeSet<(Binding, Field)>,
}

impl Owned {
    fn join(&self, o: &Owned) -> Owned {
        // A path that no longer owns the binding has no field left to drop: every field is a
        // hole there.
        let hole = |s: &Owned, h: &(Binding, Field)| s.holes.contains(h) || !s.may.contains(&h.0);
        Owned {
            may: self.may.union(&o.may).copied().collect(),
            must: self.must.intersection(&o.must).copied().collect(),
            holes: self
                .holes
                .union(&o.holes)
                .filter(|h| hole(self, h) && hole(o, h))
                .copied()
                .collect(),
        }
    }
}

fn step_moved(s: &mut Moved, e: &Event) {
    match *e {
        Event::Def { binding, .. } => s.retain(|(b, ..)| *b != binding),
        Event::Move { binding, site } => {
            s.insert((binding, None, site));
        }
        Event::MoveField {
            binding,
            field,
            site,
        } => {
            s.insert((binding, Some(field), site));
        }
        Event::Use { .. } | Event::Drop { .. } => {}
    }
}

fn step_owned(s: &mut Owned, e: &Event) {
    if let Event::Def { binding, .. } | Event::Move { binding, .. } | Event::Drop { binding } = *e {
        s.holes.retain(|(b, _)| *b != binding);
    }
    match *e {
        Event::Def { binding, owned } => {
            if owned {
                s.may.insert(binding);
                s.must.insert(binding);
            } else {
                s.may.remove(&binding);
                s.must.remove(&binding);
            }
        }
        Event::Move { binding, .. } | Event::Drop { binding } => {
            s.may.remove(&binding);
            s.must.remove(&binding);
        }
        Event::MoveField { binding, field, .. } => {
            s.holes.insert((binding, field));
        }
        Event::Use { .. } => {}
    }
}

/// Live bindings before event `from` of a node whose live-out is `out`.
fn live_before(out: &BTreeSet<Binding>, events: &[Event], from: usize) -> BTreeSet<Binding> {
    let mut live = out.clone();
    for e in events[from..].iter().rev() {
        match *e {
            Event::Def { binding, .. } => {
                live.remove(&binding);
            }
            Event::Use { binding, .. }
            | Event::Move { binding, .. }
            | Event::MoveField { binding, .. } => {
                live.insert(binding);
            }
            Event::Drop { .. } => {}
        }
    }
    live
}

pub struct Solution<'a> {
    cfg: &'a Cfg,
    moved_in: Vec<Moved>,
    own_in: Vec<Option<Owned>>,
    live_out: Vec<BTreeSet<Binding>>,
}

impl Solution<'_> {
    /// Whether `b` (or, for `Some(f)`, its field) may have been moved at `p`. A whole-value
    /// move also counts for a field.
    pub fn maybe_moved(&self, b: Binding, field: Option<Field>, p: Point) -> bool {
        self.moved_at(p)
            .iter()
            .any(|&(x, f, _)| x == b && (f.is_none() || f == field))
    }

    /// The sites of the whole-value moves of `b` that may reach `p`, in order.
    pub fn moves(&self, b: Binding, p: Point) -> Vec<Site> {
        let s: BTreeSet<Site> = self
            .moved_at(p)
            .iter()
            .filter(|&&(x, f, _)| x == b && f.is_none())
            .map(|&(.., site)| site)
            .collect();
        s.into_iter().collect()
    }

    /// Fields of `b` that may have been taken out at `p`.
    pub fn moved_fields(&self, b: Binding, p: Point) -> Vec<Field> {
        let s: BTreeSet<Field> = self
            .moved_at(p)
            .iter()
            .filter_map(|&(x, f, _)| if x == b { f } else { None })
            .collect();
        s.into_iter().collect()
    }

    fn moved_at(&self, p: Point) -> Moved {
        let mut s = self.moved_in[p.node.0].clone();
        for e in &self.cfg.events[p.node.0][..p.event] {
            step_moved(&mut s, e);
        }
        s
    }

    /// Whether a read of `b` is reachable from `p` before a redefinition.
    pub fn live_at(&self, b: Binding, p: Point) -> bool {
        let ev = &self.cfg.events[p.node.0];
        live_before(&self.live_out[p.node.0], ev, p.event).contains(&b)
    }

    /// Ownership at `p`; `None` when `p` is unreachable from the entry.
    pub fn owned_at(&self, p: Point) -> Option<Owned> {
        let mut s = self.own_in[p.node.0].clone()?;
        for e in &self.cfg.events[p.node.0][..p.event] {
            step_owned(&mut s, e);
        }
        Some(s)
    }

    /// The earliest read of `b` reachable from `p` before a redefinition: the witness for
    /// "used again".
    pub fn next_use(&self, b: Binding, p: Point) -> Option<Site> {
        self.cfg.next_use(b, p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const X: Binding = Binding(0);
    const Y: Binding = Binding(1);

    fn at(node: Node, event: usize) -> Point {
        Point { node, event }
    }

    fn mv(b: Binding, s: u32) -> Event {
        Event::Move {
            binding: b,
            site: Site(s),
        }
    }

    fn rd(b: Binding, s: u32) -> Event {
        Event::Use {
            binding: b,
            site: Site(s),
        }
    }

    fn def(b: Binding) -> Event {
        Event::Def {
            binding: b,
            owned: true,
        }
    }

    /// entry -> then | else -> join
    fn diamond() -> (Cfg, [Node; 4]) {
        let mut g = Cfg::new();
        let n = [g.node(), g.node(), g.node(), g.node()];
        g.edge(n[0], n[1]);
        g.edge(n[0], n[2]);
        g.edge(n[1], n[3]);
        g.edge(n[2], n[3]);
        (g, n)
    }

    #[test]
    fn moved_on_one_branch_is_maybe_moved_at_join() {
        let (mut g, n) = diamond();
        g.push(n[0], def(X));
        g.push(n[1], mv(X, 1));
        g.push(n[3], rd(X, 2));
        let s = g.solve();
        assert!(s.maybe_moved(X, None, at(n[3], 0)));
        assert!(!s.maybe_moved(X, None, at(n[2], 0)));
    }

    #[test]
    fn redefinition_clears_moved() {
        let mut g = Cfg::new();
        let a = g.node();
        g.push(a, def(X));
        g.push(a, mv(X, 1));
        g.push(a, def(X));
        let s = g.solve();
        assert!(s.maybe_moved(X, None, at(a, 2)));
        assert!(!s.maybe_moved(X, None, at(a, 3)));
    }

    #[test]
    fn use_in_other_arm_is_not_a_next_use() {
        let (mut g, n) = diamond();
        g.push(n[0], def(X));
        g.push(n[1], mv(X, 1));
        g.push(n[2], rd(X, 2));
        let s = g.solve();
        assert_eq!(s.next_use(X, at(n[1], 1)), None);
        assert!(!s.live_at(X, at(n[1], 1)));
        assert_eq!(s.next_use(X, at(n[0], 1)), Some(Site(1)));
    }

    #[test]
    fn loop_back_edge_makes_an_earlier_read_a_next_use() {
        let mut g = Cfg::new();
        let (head, body, exit) = (g.node(), g.node(), g.node());
        g.push(head, def(X));
        g.edge(head, body);
        g.push(body, rd(X, 1));
        g.push(body, mv(X, 2));
        g.edge(body, body);
        g.edge(body, exit);
        let s = g.solve();
        assert_eq!(s.next_use(X, at(body, 2)), Some(Site(1)));
        assert!(s.live_at(X, at(body, 2)));
    }

    #[test]
    fn redefinition_in_loop_ends_the_live_range() {
        let mut g = Cfg::new();
        let (head, body) = (g.node(), g.node());
        g.push(head, def(X));
        g.edge(head, body);
        g.push(body, def(X));
        g.push(body, rd(X, 1));
        g.push(body, mv(X, 2));
        g.edge(body, body);
        let s = g.solve();
        assert_eq!(s.next_use(X, at(body, 3)), None);
    }

    #[test]
    fn ownership_may_and_must_differ_after_a_one_arm_move() {
        let (mut g, n) = diamond();
        g.push(n[0], def(X));
        g.push(n[0], def(Y));
        g.push(n[1], mv(X, 1));
        let s = g.solve();
        let o = s.owned_at(at(n[3], 0)).unwrap();
        assert!(o.may.contains(&X) && !o.must.contains(&X));
        assert!(o.may.contains(&Y) && o.must.contains(&Y));
    }

    #[test]
    fn drop_ends_ownership_without_a_move() {
        let mut g = Cfg::new();
        let a = g.node();
        g.push(a, def(X));
        g.push(a, Event::Drop { binding: X });
        let s = g.solve();
        assert!(s.owned_at(at(a, 2)).unwrap().may.is_empty());
        assert!(!s.maybe_moved(X, None, at(a, 2)));
    }

    #[test]
    fn taking_a_field_leaves_the_record_owned_with_a_hole() {
        let mut g = Cfg::new();
        let a = g.node();
        g.push(a, def(X));
        g.push(
            a,
            Event::MoveField {
                binding: X,
                field: Field(7),
                site: Site(1),
            },
        );
        let s = g.solve();
        assert!(s.maybe_moved(X, Some(Field(7)), at(a, 2)));
        assert!(!s.maybe_moved(X, Some(Field(8)), at(a, 2)));
        assert!(!s.maybe_moved(X, None, at(a, 2)));
        assert_eq!(s.moved_fields(X, at(a, 2)), vec![Field(7)]);
        assert!(s.owned_at(at(a, 2)).unwrap().must.contains(&X));
    }

    #[test]
    fn a_field_is_a_hole_at_a_join_only_if_taken_on_every_owning_path() {
        let take = |f: u32, s: u32| Event::MoveField {
            binding: X,
            field: Field(f),
            site: Site(s),
        };
        let (mut g, n) = diamond();
        g.push(n[0], def(X));
        g.push(n[1], take(0, 1));
        g.push(n[1], take(1, 2));
        g.push(n[2], take(0, 3));
        let s = g.solve();
        let o = s.owned_at(at(n[3], 0)).unwrap();
        assert_eq!(o.holes, BTreeSet::from([(X, Field(0))]));

        let (mut g, n) = diamond();
        g.push(n[0], def(X));
        g.push(n[1], take(0, 1));
        g.push(n[2], mv(X, 2));
        let s = g.solve();
        let o = s.owned_at(at(n[3], 0)).unwrap();
        assert_eq!(o.holes, BTreeSet::from([(X, Field(0))]));
        assert!(o.may.contains(&X) && !o.must.contains(&X));
    }

    #[test]
    fn moves_name_every_site_that_may_reach() {
        let (mut g, n) = diamond();
        g.push(n[0], def(X));
        g.push(n[1], mv(X, 1));
        g.push(n[2], mv(X, 2));
        let s = g.solve();
        assert_eq!(s.moves(X, at(n[3], 0)), vec![Site(1), Site(2)]);
    }

    #[test]
    fn unreachable_node_has_no_ownership() {
        let mut g = Cfg::new();
        let _a = g.node();
        let b = g.node();
        assert!(g.solve().owned_at(at(b, 0)).is_none());
    }

    #[test]
    fn next_use_picks_the_earliest_site_across_paths() {
        let (mut g, n) = diamond();
        g.push(n[0], def(X));
        g.push(n[1], rd(X, 5));
        g.push(n[2], rd(X, 3));
        let s = g.solve();
        assert_eq!(s.next_use(X, at(n[0], 1)), Some(Site(3)));
    }
}
