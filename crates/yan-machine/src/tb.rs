//! 6.7 item 5: Tree Borrows (6.6, RFC 0088). Each allocation that has been retagged keeps a tree of
//! tags with a permission per 8-byte word. A tag's parent is the tag it was retagged from; tag 1
//! is the allocation's root, and a pointer with no tag (provenance word 0) is the root. A pointer
//! value carries `(allocation + 1) << 32 | tag` in its provenance word.
//!
//! Simplification: a protected `Reserved` word does not track the "conflicted" bit, so a few
//! protected-write sequences that real Tree Borrows flags go unreported. It never reports a
//! program real Tree Borrows accepts.

use yan_mir::RetagKind;

pub type Tag = u32;
pub const ROOT: Tag = 1;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Perm {
    Reserved,
    Active,
    Frozen,
    /// Interior-mutable bytes: any access is allowed and none changes the state.
    Cell,
    Disabled,
}

struct Node {
    tag: Tag,
    parent: Option<Tag>,
    perms: Vec<Perm>,
    protectors: u32,
    site: String,
}

pub struct Tree {
    nodes: Vec<Node>,
    next: Tag,
}

pub fn pack(alloc: usize, tag: Tag) -> u64 {
    ((alloc as u64 + 1) << 32) | u64::from(tag)
}

/// `(allocation, tag)` of a provenance word; `None` for word 0.
pub fn unpack(prov: u64) -> Option<(usize, Tag)> {
    (prov >> 32 != 0).then(|| ((prov >> 32) as usize - 1, prov as u32))
}

impl Tree {
    pub fn new(words: usize) -> Tree {
        Tree {
            nodes: vec![Node {
                tag: ROOT,
                parent: None,
                perms: vec![Perm::Active; words],
                protectors: 0,
                site: "the allocation".into(),
            }],
            next: ROOT + 1,
        }
    }

    pub fn has(&self, tag: Tag) -> bool {
        self.index(tag).is_some()
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    fn index(&self, tag: Tag) -> Option<usize> {
        self.nodes.iter().position(|n| n.tag == tag)
    }

    /// Whether `anc` is `tag` or one of its ancestors.
    fn covers(&self, anc: Tag, tag: Tag) -> bool {
        let mut at = Some(tag);
        while let Some(t) = at {
            if t == anc {
                return true;
            }
            at = self.index(t).and_then(|i| self.nodes[i].parent);
        }
        false
    }

    /// A read or write of words `lo..hi` through `tag`, in every node's view.
    pub fn access(&mut self, tag: Tag, lo: usize, hi: usize, write: bool) -> Result<(), String> {
        if self.index(tag).is_none() {
            return Err(format!("access through tag {tag}, which is not live"));
        }
        for i in 0..self.nodes.len() {
            let child = self.covers(self.nodes[i].tag, tag);
            let n = &mut self.nodes[i];
            for w in lo..hi.min(n.perms.len()) {
                let p = n.perms[w];
                let protected = n.protectors > 0;
                let next = match (child, write, p) {
                    (_, _, Perm::Cell) => Perm::Cell,
                    (true, _, Perm::Disabled) => {
                        return Err(format!(
                            "access through tag {tag} outside its range or after it was invalidated (retagged at {})",
                            n.site
                        ));
                    }
                    (true, false, p) => p,
                    (true, true, Perm::Reserved | Perm::Active) => Perm::Active,
                    (true, true, Perm::Frozen) => {
                        return Err(format!(
                            "write through frozen tag {} (retagged at {})",
                            n.tag, n.site
                        ));
                    }
                    (false, _, Perm::Disabled) => Perm::Disabled,
                    (false, false, Perm::Active) if protected => {
                        return Err(format!(
                            "read of a word while protected tag {} holds it uniquely (retagged at {})",
                            n.tag, n.site
                        ));
                    }
                    (false, false, Perm::Active) => Perm::Frozen,
                    (false, false, p) => p,
                    (false, true, _) if protected => {
                        return Err(format!(
                            "write to a word while protected tag {} holds it (retagged at {})",
                            n.tag, n.site
                        ));
                    }
                    (false, true, _) => Perm::Disabled,
                };
                n.perms[w] = next;
            }
        }
        Ok(())
    }

    /// A new tag under `parent` over words `lo..hi`; its other words are unusable through it.
    pub fn retag(
        &mut self,
        parent: Tag,
        lo: usize,
        hi: usize,
        kind: RetagKind,
        protected: bool,
        site: String,
    ) -> Result<Tag, String> {
        if kind != RetagKind::Cell {
            self.access(parent, lo, hi, false)?;
        }
        let words = self.nodes[0].perms.len();
        let init = match kind {
            RetagKind::Unique => Perm::Reserved,
            RetagKind::Frozen => Perm::Frozen,
            RetagKind::Cell => Perm::Cell,
        };
        let mut perms = vec![Perm::Disabled; words];
        perms[lo..hi.min(words)].fill(init);
        let tag = self.next;
        self.next += 1;
        self.nodes.push(Node {
            tag,
            parent: Some(parent),
            perms,
            protectors: u32::from(protected),
            site,
        });
        Ok(tag)
    }

    pub fn unprotect(&mut self, tag: Tag) {
        if let Some(i) = self.index(tag) {
            self.nodes[i].protectors = self.nodes[i].protectors.saturating_sub(1);
        }
    }

    /// Drops every unprotected leaf that `live` does not name, repeatedly; the root stays.
    pub fn collect(&mut self, live: impl Fn(Tag) -> bool) {
        loop {
            let parents: Vec<Tag> = self.nodes.iter().filter_map(|n| n.parent).collect();
            let before = self.nodes.len();
            self.nodes.retain(|n| {
                n.tag == ROOT || n.protectors > 0 || live(n.tag) || parents.contains(&n.tag)
            });
            if self.nodes.len() == before {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Tree {
        Tree::new(2)
    }

    #[test]
    fn unique_child_survives_own_accesses_and_loses_to_a_foreign_write() {
        let mut t = tree();
        let u = t
            .retag(ROOT, 0, 1, RetagKind::Unique, false, "a".into())
            .unwrap();
        t.access(u, 0, 1, true).unwrap();
        t.access(ROOT, 0, 1, true).unwrap();
        assert!(t.access(u, 0, 1, false).is_err());
    }

    #[test]
    fn frozen_child_rejects_a_write_and_tolerates_foreign_reads() {
        let mut t = tree();
        let f = t
            .retag(ROOT, 0, 1, RetagKind::Frozen, false, "a".into())
            .unwrap();
        t.access(ROOT, 0, 1, false).unwrap();
        t.access(f, 0, 1, false).unwrap();
        assert!(t.access(f, 0, 1, true).is_err());
    }

    #[test]
    fn outside_its_range_a_tag_is_unusable() {
        let mut t = tree();
        let u = t
            .retag(ROOT, 0, 1, RetagKind::Unique, false, "a".into())
            .unwrap();
        assert!(t.access(u, 1, 2, false).is_err());
    }

    #[test]
    fn a_protected_tag_forbids_foreign_access_after_it_was_written() {
        let mut t = tree();
        let u = t
            .retag(ROOT, 0, 1, RetagKind::Unique, true, "a".into())
            .unwrap();
        t.access(u, 0, 1, true).unwrap();
        assert!(t.access(ROOT, 0, 1, false).is_err());
    }

    #[test]
    fn cell_allows_everything() {
        let mut t = tree();
        let c = t
            .retag(ROOT, 0, 1, RetagKind::Cell, false, "a".into())
            .unwrap();
        t.access(ROOT, 0, 1, true).unwrap();
        t.access(c, 0, 1, true).unwrap();
    }

    #[test]
    fn collect_keeps_ancestors_of_live_tags_and_protected_ones() {
        let mut t = tree();
        let a = t
            .retag(ROOT, 0, 2, RetagKind::Unique, false, "a".into())
            .unwrap();
        let b = t
            .retag(a, 0, 1, RetagKind::Unique, false, "b".into())
            .unwrap();
        let _p = t
            .retag(ROOT, 0, 1, RetagKind::Frozen, true, "p".into())
            .unwrap();
        let _dead = t
            .retag(ROOT, 1, 2, RetagKind::Frozen, false, "d".into())
            .unwrap();
        t.collect(|x| x == b);
        assert_eq!(t.len(), 4);
        t.collect(|_| false);
        assert_eq!(t.len(), 2);
    }
}
