//! The closure check (RFC 0079 D1 to D3): every package's ceiling lists every extern library in its
//! normal-dependency closure, itself included. Another package's library is spelled `@a/b:zlib`.

use std::collections::{HashMap, VecDeque};

use crate::Package;

pub type Key = (String, String);

pub fn identity(name: &str, version: &str) -> Key {
    let mut parts = version.split('.');
    let major = parts.next().unwrap_or("");
    let key = match major {
        "0" => format!("0.{}", parts.next().unwrap_or("0")),
        m => m.to_owned(),
    };
    (name.to_owned(), key)
}

pub fn requirement(req: &str) -> Option<Key> {
    let (name, version) = req.split_once(' ')?;
    Some(identity(name, version.trim()))
}

pub struct Node {
    pub name: String,
    pub key: Option<Key>,
    /// `None` when the manifest names no ceiling; nothing to check then.
    pub ceiling: Option<Vec<String>>,
    pub libs: Vec<String>,
    /// Requirements of `dependencies` alone; `dev_dependencies` are outside the closure (R636).
    pub deps: Vec<Key>,
}

pub fn deps_of(reqs: &[String]) -> Vec<Key> {
    reqs.iter().filter_map(|r| requirement(r)).collect()
}

pub fn from_package(p: &Package) -> Node {
    Node {
        name: p.name.clone().unwrap_or_else(|| ".".into()),
        key: p
            .name
            .as_deref()
            .zip(p.manifest.version.as_deref())
            .map(|(n, v)| identity(n, v)),
        ceiling: p.manifest.capabilities.clone(),
        libs: p.foreign.libs.iter().map(|l| l.name.clone()).collect(),
        deps: deps_of(&p.deps),
    }
}

fn index(nodes: &[Node]) -> HashMap<&Key, usize> {
    nodes
        .iter()
        .enumerate()
        .filter_map(|(i, n)| Some((n.key.as_ref()?, i)))
        .collect()
}

/// The shortest dependency path from `start` to every package in its normal-dependency closure,
/// itself included, in breadth-first order.
pub fn reach(nodes: &[Node], start: usize) -> Vec<Vec<usize>> {
    let index = index(nodes);
    let mut seen = vec![false; nodes.len()];
    seen[start] = true;
    let mut queue = VecDeque::from([vec![start]]);
    let mut out = Vec::new();
    while let Some(path) = queue.pop_front() {
        let at = *path.last().unwrap_or(&start);
        for d in &nodes[at].deps {
            if let Some(&i) = index.get(d)
                && !seen[i]
            {
                seen[i] = true;
                let mut next = path.clone();
                next.push(i);
                queue.push_back(next);
            }
        }
        out.push(path);
    }
    out
}

/// The library entry a package's ceiling spells: bare for its own, `@a/b:zlib` for another's.
pub fn entry(owner: &Node, lib: &str, own: bool) -> String {
    if own {
        lib.to_owned()
    } else {
        format!("{}:{lib}", owner.name)
    }
}

/// E314 for each package whose ceiling misses a library of its closure: the index of the package and
/// the message, with the shortest dependency path from that package to the declaring one.
pub fn violations(nodes: &[Node]) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (start, p) in nodes.iter().enumerate() {
        let Some(ceiling) = &p.ceiling else {
            continue;
        };
        let missing = reach(nodes, start).into_iter().find_map(|path| {
            let at = *path.last()?;
            let q = &nodes[at];
            q.libs.iter().find_map(|lib| {
                let e = entry(q, lib, at == start);
                (!ceiling.contains(&e)).then(|| (e, path.clone()))
            })
        });
        if let Some((e, path)) = missing {
            let route: Vec<&str> = path.iter().map(|&i| nodes[i].name.as_str()).collect();
            out.push((
                start,
                format!(
                    "library `{e}` is outside the ceiling of `{}` ({})",
                    p.name,
                    route.join(" -> ")
                ),
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str, ceiling: Option<&[&str]>, libs: &[&str], deps: &[&str]) -> Node {
        let owned = |s: &[&str]| s.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>();
        Node {
            name: name.into(),
            key: Some(identity(name, "1.0")),
            ceiling: ceiling.map(owned),
            libs: owned(libs),
            deps: deps.iter().map(|d| identity(d, "1.0")).collect(),
        }
    }

    fn message(nodes: &[Node]) -> String {
        violations(nodes).swap_remove(0).1
    }

    #[test]
    fn a_root_must_list_its_own_and_its_dependencies_libraries() {
        let ok = [
            node("@a/root", Some(&["zlib", "@a/b:ssl"]), &["zlib"], &["@a/b"]),
            node("@a/b", Some(&["ssl"]), &["ssl"], &[]),
        ];
        assert!(violations(&ok).is_empty());
        let bad = [
            node("@a/root", Some(&["zlib"]), &["zlib"], &["@a/b"]),
            node("@a/b", Some(&["ssl"]), &["ssl"], &[]),
        ];
        let m = message(&bad);
        assert!(
            m.contains("`@a/b:ssl`") && m.contains("@a/root -> @a/b"),
            "{m}"
        );
    }

    #[test]
    fn an_intermediate_ceiling_is_checked_with_the_full_path() {
        let nodes = [
            node("@a/root", Some(&["@a/c:z", "@a/b:z"]), &[], &["@a/b"]),
            node("@a/b", Some(&[]), &[], &["@a/c"]),
            node("@a/c", Some(&["z"]), &["z"], &[]),
        ];
        let m = message(&nodes);
        assert!(
            m.contains("`@a/c:z`") && m.contains("ceiling of `@a/b`"),
            "{m}"
        );
        assert!(m.contains("@a/b -> @a/c"), "{m}");
    }

    #[test]
    fn a_uses_none_wrapper_still_needs_the_library() {
        let nodes = [
            node("@a/root", Some(&[]), &[], &["@a/w"]),
            node("@a/w", Some(&["@a/f:z"]), &[], &["@a/f"]),
            node("@a/f", Some(&["z"]), &["z"], &[]),
        ];
        assert!(message(&nodes).contains("ceiling of `@a/root`"));
    }

    #[test]
    fn the_same_bare_name_in_two_packages_is_two_entries() {
        let nodes = [
            node("@a/root", Some(&["@a/b:z"]), &[], &["@a/b", "@a/c"]),
            node("@a/b", Some(&["z"]), &["z"], &[]),
            node("@a/c", Some(&["z"]), &["z"], &[]),
        ];
        assert!(message(&nodes).contains("`@a/c:z`"));
    }

    #[test]
    fn no_ceiling_and_unresolved_dependencies_are_skipped() {
        let nodes = [node("@a/root", None, &["z"], &["@a/dev"])];
        assert!(violations(&nodes).is_empty());
    }
}
