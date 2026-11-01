//! The surface kit (22.2, R560). For now it holds the trivia side table of 16.5: everything a
//! surface needs to print a module byte for byte that is not part of its item hashes.

use std::collections::{BTreeMap, HashMap};

/// A node inside an item: child indices from the item's root.
pub type NodePath = Vec<u32>;

/// Where a comment attaches (16.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attach {
    /// On its own line, to the next node.
    Leading,
    /// On the same line, to the previous node.
    Trailing,
    /// In an empty construct or before a closer.
    Dangling,
}

/// What elaboration inserted (R565).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inserted {
    Capability,
    Copy,
    Move,
    Conversion,
}

/// One trivia entry at a node. Local names, the sixth kind, are kept per item by binding order
/// (R519) rather than by node; see [`TriviaTable::set_names`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Trivia {
    /// A comment or docstring, with its text as written.
    Comment { text: String, attach: Attach },
    /// Blank lines before the node.
    BlankLines(u32),
    /// A literal's spelling when it is not the canonical one (18.5).
    Literal(String),
    /// Which of several Chang spellings produced the node (R518).
    Spelling(String),
    /// The node was inserted by elaboration, not written (R565).
    Origin(Inserted),
}

/// A module's trivia, keyed by item hash and node path. It is never hashed (16.5).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TriviaTable {
    nodes: BTreeMap<(String, NodePath), Vec<Trivia>>,
    names: HashMap<String, Vec<String>>,
}

impl TriviaTable {
    pub fn add(&mut self, item: &str, path: NodePath, t: Trivia) {
        self.nodes
            .entry((item.to_owned(), path))
            .or_default()
            .push(t);
    }

    pub fn at(&self, item: &str, path: &[u32]) -> &[Trivia] {
        self.nodes
            .get(&(item.to_owned(), path.to_vec()))
            .map_or(&[], Vec::as_slice)
    }

    /// The origin mark at a node, if elaboration inserted it.
    pub fn origin(&self, item: &str, path: &[u32]) -> Option<Inserted> {
        self.at(item, path).iter().find_map(|t| match t {
            Trivia::Origin(o) => Some(*o),
            _ => None,
        })
    }

    /// Keeps only the entries `keep` accepts. After an edit, a note whose spelling no longer
    /// matches its node is dropped and the default spelling prints (16.5).
    pub fn retain(&mut self, mut keep: impl FnMut(&str, &[u32], &Trivia) -> bool) {
        self.nodes.retain(|(item, path), ts| {
            ts.retain(|t| keep(item, path, t));
            !ts.is_empty()
        });
    }

    /// An item's name table: `names[k]` is the written name of local `%k`.
    pub fn set_names(&mut self, item: &str, names: Vec<String>) {
        self.names.insert(item.to_owned(), names);
    }

    /// The written name of local `k` of `item`.
    pub fn name(&self, item: &str, k: usize) -> Option<&str> {
        self.names.get(item)?.get(k).map(String::as_str)
    }
}

#[cfg(test)]
mod selftest {
    use super::*;

    #[test]
    fn entries_by_item_and_node() {
        let mut t = TriviaTable::default();
        t.add("h1", vec![0, 2], Trivia::Origin(Inserted::Copy));
        t.add("h1", vec![0, 2], Trivia::BlankLines(1));
        t.add("h2", vec![0, 2], Trivia::Literal("0x10".into()));
        assert_eq!(t.origin("h1", &[0, 2]), Some(Inserted::Copy));
        assert_eq!(t.origin("h2", &[0, 2]), None);
        assert_eq!(t.at("h1", &[1]), &[]);
        t.retain(|_, _, x| !matches!(x, Trivia::Literal(_)));
        assert_eq!(t.at("h2", &[0, 2]), &[]);
        assert_eq!(t.at("h1", &[0, 2]).len(), 2);
    }

    #[test]
    fn name_tables() {
        let mut t = TriviaTable::default();
        t.set_names("h1", vec!["total".into(), "i".into()]);
        assert_eq!(t.name("h1", 1), Some("i"));
        assert_eq!(t.name("h1", 2), None);
    }
}
