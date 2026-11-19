//! RFC 0094: a channel-constructor habit name (`Chan[int](n)`, `queue.Queue(n)`, `nondet.channel(n)`)
//! is E200 naming `Channel[T](capacity)`, with no E017 before it.

use super::*;

const CHANNEL_HINT: &str = "channels are built with `Channel[T](capacity)`, which returns \
                            `(Sender[T], Receiver[T])`";

const HABIT_NAMES: [&str; 7] = [
    "Chan",
    "chan",
    "channel",
    "make_chan",
    "Queue",
    "sync.Channel",
    "nondet.channel",
];

const HABIT_PATHS: [&str; 3] = ["queue.Queue", "asyncio.Queue", "queue.SimpleQueue"];

impl Lower<'_, '_, '_> {
    fn defined(&self, n: &str) -> bool {
        self.g.fns.contains_key(n)
            || self.g.records.contains_key(n)
            || self.g.variant_of.contains_key(n)
    }

    /// The habit name `f` spells, when it resolves to nothing the program or std defines.
    fn channel_habit(&self, f: &j::Expr) -> Option<String> {
        let callee = match &f.kind {
            j::ExprKind::Index(b, _) => b,
            _ => f,
        };
        let path = match &callee.kind {
            j::ExprKind::Name(n) if !self.value_bound(n) && !self.defined(n) => n.clone(),
            j::ExprKind::Attr(m, op, _) => match &m.kind {
                j::ExprKind::Name(mn) if !self.value_bound(mn) => format!("{mn}.{op}"),
                _ => return None,
            },
            _ => return None,
        };
        (HABIT_NAMES.contains(&path.as_str()) || HABIT_PATHS.contains(&path.as_str()))
            .then_some(path)
    }

    /// Reports E200 for a habit name and answers with an error value.
    pub(super) fn channel_near_miss(
        &mut self,
        f: &j::Expr,
        at: Span,
    ) -> Option<(Rhs, Ty, Option<Ty>)> {
        let n = self.channel_habit(f)?;
        self.err("E200", f.span, format!("cannot find `{n}`: {CHANNEL_HINT}"));
        Some((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err, None))
    }
}
