//! Spelling marks (RFC 0034 D1, D4). Lowering knows a node by its HIR span and kind, not yet by its
//! final path, because later passes rewrite statements. It records a [`Pending`] mark, and
//! [`place`] moves each one, once the item is built, to the first node in walk order with that
//! span and kind. The span is only this join key inside one elaboration; it is never stored.

use yan_core::ast::{Arg, Atom, Ctor, Item};
use yan_core::path::{Node, walk};
use yan_diag::Span;
use yan_surface::{Inserted, Mark, Trivia, TriviaTable};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The item itself, joined by its span.
    Item,
    Stmt,
    /// An `assert` statement.
    Assert,
    Call,
    /// A name atom that was `out`, joined by the span of the atom.
    Out,
    Arg,
    /// Field `k` of a constructor, joined by the span of its atom.
    Field(usize),
    /// A literal, joined by its span.
    Lit,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pending {
    pub span: Span,
    pub kind: Kind,
    /// For an argument, the span of its call: a helper call elaboration builds around the same
    /// expression can reuse the argument's span.
    pub call: Option<Span>,
    pub mark: Mark,
}

/// The pending marks of `item`'s elaboration written into `table` under `key`. Returns the
/// marks no node claimed.
pub fn place(
    key: &str,
    item: &Item,
    pending: &[Pending],
    lits: &[(Span, String)],
    origins: &[(Span, Inserted)],
    table: &mut TriviaTable,
) -> Vec<Pending> {
    let mut claimed = vec![false; pending.len()];
    let mut lit_claimed = vec![false; lits.len()];
    let mut origin_claimed = vec![false; origins.len()];
    let mut call = None;
    walk(item, &mut |path, node| {
        let mut cands = Vec::new();
        match node {
            Node::Stmt(s) if matches!(s.kind, yan_core::ast::StmtKind::Assert(_)) => {
                cands.push((Kind::Assert, s.span))
            }
            Node::Stmt(s) => cands.push((Kind::Stmt, s.span)),
            Node::Field(Ctor::Named { fields, .. }, k) => {
                let (Atom::Name(_, sp) | Atom::Lit(_, sp)) = &fields[k].1;
                cands.push((Kind::Field(k), *sp));
                if matches!(fields[k].1, Atom::Name(..)) {
                    cands.push((Kind::Out, *sp));
                }
            }
            Node::Literal(_, span) => {
                if let Some(i) = (0..lits.len()).find(|&i| !lit_claimed[i] && lits[i].0 == span) {
                    lit_claimed[i] = true;
                    table.add(key, path.to_vec(), Trivia::Literal(lits[i].1.clone()));
                }
                cands.push((Kind::Lit, span));
            }
            Node::Field(..) => return,
            Node::Item(i) => cands.push((Kind::Item, i.span)),
            Node::Call(c) => {
                call = Some(c.span);
                if let Some(i) =
                    (0..origins.len()).find(|&i| !origin_claimed[i] && origins[i].0 == c.span)
                {
                    origin_claimed[i] = true;
                    table.add(key, path.to_vec(), Trivia::Origin(origins[i].1));
                }
                cands.push((Kind::Call, c.span))
            }
            Node::Arg(a) => {
                let (Arg::Value(_, _, s) | Arg::Cap(_, s)) = a;
                cands.push((Kind::Arg, *s));
                if let Arg::Value(_, Atom::Name(_, sp), _) = a {
                    cands.push((Kind::Out, *sp));
                }
            }
            _ => return,
        };
        // A call lowered twice leaves two nodes with one span and the same marks: each node
        // takes one copy of a mark, so the later copy reaches the later node.
        let mut here: Vec<&Mark> = Vec::new();
        for (kind, span) in cands {
            for (i, p) in pending.iter().enumerate() {
                if !claimed[i]
                    && p.kind == kind
                    && p.span == span
                    && (kind != Kind::Arg || p.call == call)
                    && !here.contains(&&p.mark)
                {
                    claimed[i] = true;
                    here.push(&p.mark);
                    table.add(key, path.to_vec(), Trivia::Mark(p.mark.clone()));
                }
            }
        }
    });
    pending
        .iter()
        .zip(claimed)
        .filter(|(_, c)| !c)
        .map(|(p, _)| p.clone())
        .collect()
}

#[cfg(test)]
mod selftest {
    use super::*;

    const SRC: &str = "module m\n\nfn f(let a: i64) -> i64 {\n    let x: i64 = add_checked_i64(let a, let 1)\n    ret x\n}\n";

    #[test]
    fn marks_sharing_a_span_and_kind_land_on_the_first_node() {
        let m = yan_core::parse("m.yir", SRC).0.expect("parses");
        let item = &m.items[0];
        let mut call_span = None;
        walk(item, &mut |_, n| {
            if let Node::Call(c) = n {
                call_span = Some(c.span);
            }
        });
        // A statement and its call may share a span; kinds keep them apart.
        let span = call_span.unwrap();
        let pending = [
            Pending {
                span,
                kind: Kind::Call,
                call: None,
                mark: Mark::Method,
            },
            Pending {
                span,
                kind: Kind::Call,
                call: None,
                mark: Mark::Function,
            },
            Pending {
                span,
                kind: Kind::Stmt,
                call: None,
                mark: Mark::Annotated,
            },
        ];
        let mut t = TriviaTable::default();
        let left = place("h", item, &pending, &[], &[], &mut t);
        let at = t.at("h", &[2, 0, 0]);
        // Both call marks share a span and kind, so both land on the first such node.
        assert_eq!(at.len(), 2);
        assert!(left.len() <= 1);
    }

    #[test]
    fn unclaimed_marks_are_returned() {
        let m = yan_core::parse("m.yir", SRC).0.expect("parses");
        let pending = [Pending {
            span: Span::default(),
            kind: Kind::Arg,
            call: None,
            mark: Mark::Keyword,
        }];
        let mut t = TriviaTable::default();
        assert_eq!(place("h", &m.items[0], &pending, &[], &[], &mut t).len(), 1);
        assert_eq!(t.entries().count(), 0);
    }
}
