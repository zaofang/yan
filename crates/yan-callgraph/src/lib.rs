//! The call graph of a program with each function's direct capability uses, and the closures
//! over it. One representation for every check that walks calls (E602, E318).

use std::collections::{HashMap, HashSet};

/// How a caller reaches a callee.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Edge {
    Call,
    /// A tail transfer: the caller's frame is replaced (6.5).
    Become,
    /// Through a trait object or a function value.
    Dyn,
    /// Into foreign code.
    Extern,
}

/// A capability a function takes: its parameter name and its type's name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cap {
    pub name: String,
    pub ty: String,
}

#[derive(Default)]
pub struct Graph {
    edges: HashMap<String, HashSet<(String, Edge)>>,
    uses: HashMap<String, Vec<Cap>>,
}

impl Graph {
    pub fn add_fn(&mut self, f: &str, edges: HashSet<(String, Edge)>, uses: Vec<Cap>) {
        self.edges.insert(f.to_owned(), edges);
        self.uses.insert(f.to_owned(), uses);
    }

    /// Does a path through the graph lead from `f` back to `f`? `skip` edge kinds are not
    /// followed.
    pub fn recursive(&self, f: &str, skip: &[Edge]) -> bool {
        self.walk(self.next(f, skip), Some(f), skip).1
    }

    /// `roots` and every function they reach, transitively.
    pub fn reachable<'a>(&'a self, roots: impl IntoIterator<Item = &'a str>) -> HashSet<&'a str> {
        self.walk(roots, None, &[]).0
    }

    /// The functions among `roots` and their callees with a capability satisfying `pred`, in name
    /// order.
    pub fn users<'a>(
        &'a self,
        roots: impl IntoIterator<Item = &'a str>,
        pred: impl Fn(&Cap) -> bool,
    ) -> Vec<&'a str> {
        let mut out: Vec<&str> = self
            .reachable(roots)
            .into_iter()
            .filter(|k| self.uses.get(*k).is_some_and(|u| u.iter().any(&pred)))
            .collect();
        out.sort_unstable();
        out
    }

    fn next<'a>(&'a self, f: &str, skip: &'a [Edge]) -> impl Iterator<Item = &'a str> + 'a {
        self.edges
            .get(f)
            .into_iter()
            .flatten()
            .filter(|(_, k)| !skip.contains(k))
            .map(|(g, _)| g.as_str())
    }

    fn walk<'a>(
        &'a self,
        start: impl IntoIterator<Item = &'a str>,
        target: Option<&str>,
        skip: &'a [Edge],
    ) -> (HashSet<&'a str>, bool) {
        let mut seen = HashSet::new();
        let mut stack: Vec<&str> = start.into_iter().collect();
        while let Some(g) = stack.pop() {
            if target == Some(g) {
                return (seen, true);
            }
            if seen.insert(g) {
                stack.extend(self.next(g, skip));
            }
        }
        (seen, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Fns<'a> = &'a [(&'a str, &'a [(&'a str, Edge)], &'a [&'a str])];

    fn graph(fns: Fns) -> Graph {
        let mut g = Graph::default();
        for (f, es, caps) in fns {
            let edges = es.iter().map(|(c, k)| ((*c).to_owned(), *k)).collect();
            let uses = caps
                .iter()
                .map(|c| Cap {
                    name: (*c).to_owned(),
                    ty: (*c).to_owned(),
                })
                .collect();
            g.add_fn(f, edges, uses);
        }
        g
    }

    #[test]
    fn a_cycle_is_recursive_unless_its_edge_is_skipped() {
        let g = graph(&[
            ("a", &[("b", Edge::Call)], &[]),
            ("b", &[("a", Edge::Become)], &[]),
        ]);
        assert!(g.recursive("a", &[]));
        assert!(!g.recursive("a", &[Edge::Become]));
    }

    #[test]
    fn users_follow_calls_from_the_roots_only() {
        let g = graph(&[
            ("main", &[("mid", Edge::Call)], &[]),
            ("mid", &[("leaf", Edge::Dyn)], &["io"]),
            ("leaf", &[], &["io"]),
            ("other", &[], &["io"]),
        ]);
        assert_eq!(g.users(["main"], |c| c.name == "io"), ["leaf", "mid"]);
    }
}
