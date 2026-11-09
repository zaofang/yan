use super::*;

const A: Binding = Binding(0);
const B: Binding = Binding(1);
const T: Tag = Tag(0);

fn at(block: Block, index: usize) -> Point {
    Point { block, index }
}

#[test]
fn move_then_read_is_use_after_move() {
    let mut g = Graph::new();
    let e = Graph::ENTRY;
    g.push(e, Event::Define(A));
    g.push(e, Event::Move(A, T));
    g.push(e, Event::Read(A, T));
    let s = g.solve();
    assert!(!s.maybe_moved(at(e, 1), Place::Whole(A)));
    assert!(s.maybe_moved(at(e, 2), Place::Whole(A)));
}

#[test]
fn redefinition_is_a_new_value() {
    let mut g = Graph::new();
    let e = Graph::ENTRY;
    g.push(e, Event::Define(A));
    g.push(e, Event::Move(A, T));
    g.push(e, Event::Define(A));
    g.push(e, Event::Read(A, T));
    let s = g.solve();
    assert!(!s.maybe_moved(at(e, 3), Place::Whole(A)));
}

#[test]
fn moved_on_one_branch_is_maybe_moved_at_join() {
    let mut g = Graph::new();
    let (e, l, r, j) = (Graph::ENTRY, g.block(), g.block(), g.block());
    g.push(e, Event::Define(A));
    g.push(l, Event::Move(A, T));
    for (f, t) in [(e, l), (e, r), (l, j), (r, j)] {
        g.edge(f, t);
    }
    let s = g.solve();
    assert!(s.maybe_moved(at(j, 0), Place::Whole(A)));
    assert!(s.maybe_owned(at(j, 0), A));
    assert!(!s.must_owned(at(j, 0), A));
    assert!(s.needs_flag(at(j, 0), A));
}

#[test]
fn moved_on_every_branch_needs_no_flag() {
    let mut g = Graph::new();
    let (e, l, r, j) = (Graph::ENTRY, g.block(), g.block(), g.block());
    g.push(e, Event::Define(A));
    g.push(l, Event::Move(A, T));
    g.push(r, Event::Move(A, T));
    for (f, t) in [(e, l), (e, r), (l, j), (r, j)] {
        g.edge(f, t);
    }
    let s = g.solve();
    assert!(!s.maybe_owned(at(j, 0), A));
    assert!(!s.needs_flag(at(j, 0), A));
}

#[test]
fn read_before_back_edge_is_live_after_a_loop_move() {
    // e: def A; head: move A; back edge to head. The move is followed by itself around the loop.
    let mut g = Graph::new();
    let (e, head, exit) = (Graph::ENTRY, g.block(), g.block());
    g.push(e, Event::Define(A));
    g.push(head, Event::Move(A, Tag(7)));
    for (f, t) in [(e, head), (head, head), (head, exit)] {
        g.edge(f, t);
    }
    let s = g.solve();
    assert_eq!(s.witness_after(at(head, 0), A), Some(Tag(7)));
    assert!(s.maybe_moved(at(head, 0), Place::Whole(A)));
}

#[test]
fn loop_redefinition_kills_liveness() {
    let mut g = Graph::new();
    let (e, head, exit) = (Graph::ENTRY, g.block(), g.block());
    g.push(head, Event::Define(A));
    g.push(head, Event::Move(A, T));
    for (f, t) in [(e, head), (head, head), (head, exit)] {
        g.edge(f, t);
    }
    let s = g.solve();
    assert!(!s.live_after(at(head, 1), A));
}

#[test]
fn last_use_is_not_live_after() {
    let mut g = Graph::new();
    let e = Graph::ENTRY;
    g.push(e, Event::Define(A));
    g.push(e, Event::Read(A, T));
    g.push(e, Event::Move(A, T));
    let s = g.solve();
    assert!(s.live_after(at(e, 1), A));
    assert!(!s.live_after(at(e, 2), A));
}

#[test]
fn later_use_only_in_other_arm_is_not_live() {
    let mut g = Graph::new();
    let (e, l, r) = (Graph::ENTRY, g.block(), g.block());
    g.push(e, Event::Define(A));
    g.push(l, Event::Move(A, T));
    g.push(r, Event::Read(A, T));
    g.edge(e, l);
    g.edge(e, r);
    let s = g.solve();
    assert!(!s.live_after(at(l, 0), A));
    assert!(s.live_after(at(e, 0), A));
}

#[test]
fn take_leaves_hole_and_owner() {
    let mut g = Graph::new();
    let e = Graph::ENTRY;
    g.push(e, Event::Define(A));
    g.push(e, Event::Take(A, Field(1), T));
    let s = g.solve();
    let end = at(e, 2);
    assert_eq!(s.holes(end, A), vec![Field(1)]);
    assert!(s.maybe_owned(end, A));
    assert!(s.maybe_moved(end, Place::Field(A, Field(1))));
    assert!(!s.maybe_moved(end, Place::Field(A, Field(2))));
    assert!(s.maybe_moved(end, Place::Whole(A)));
}

#[test]
fn distinct_bindings_do_not_interfere() {
    let mut g = Graph::new();
    let e = Graph::ENTRY;
    g.push(e, Event::Define(A));
    g.push(e, Event::Define(B));
    g.push(e, Event::Move(A, T));
    let s = g.solve();
    assert!(!s.maybe_moved(at(e, 3), Place::Whole(B)));
    assert!(s.maybe_owned(at(e, 3), B));
}

#[test]
fn owned_at_exits_lists_unconsumed_bindings() {
    let mut g = Graph::new();
    let (e, a, b) = (Graph::ENTRY, g.block(), g.block());
    g.push(e, Event::Define(A));
    g.push(a, Event::Move(A, T));
    g.edge(e, a);
    g.edge(e, b);
    let s = g.solve();
    let exits = s.owned_at_exits();
    assert_eq!(exits[&a], Vec::<Binding>::new());
    assert_eq!(exits[&b], vec![A]);
}

#[test]
fn unreachable_block_has_no_facts() {
    let mut g = Graph::new();
    let dead = g.block();
    g.push(Graph::ENTRY, Event::Define(A));
    g.push(dead, Event::Move(A, T));
    let s = g.solve();
    assert!(!s.maybe_moved(at(dead, 1), Place::Whole(A)));
}
