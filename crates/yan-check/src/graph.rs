//! The call graph of a module with each function's direct capability uses, and the closures
//! over it. One representation for every check that walks calls (E602, E318).

use std::collections::{HashMap, HashSet};

use yan_core::ast::*;

pub(crate) struct Graph<'m> {
    callees: HashMap<String, HashSet<String>>,
    fns: HashMap<String, &'m FnDef>,
}

impl<'m> Graph<'m> {
    pub fn new(callees: HashMap<String, HashSet<String>>, fns: HashMap<String, &'m FnDef>) -> Self {
        Graph { callees, fns }
    }

    /// Does a path through the call graph lead from `f` back to `f`?
    pub fn recursive(&self, f: &str) -> bool {
        self.walk(
            self.callees
                .get(f)
                .into_iter()
                .flatten()
                .map(String::as_str),
            Some(f),
        )
        .1
    }

    /// `roots` and every function they call, transitively.
    pub fn reachable<'a>(&'a self, roots: impl IntoIterator<Item = &'a str>) -> HashSet<&'a str> {
        self.walk(roots, None).0
    }

    /// The functions among `roots` and their callees that take the capability bound as `cap`
    /// or of trait `ty`, in name order.
    pub fn users<'a>(
        &'a self,
        roots: impl IntoIterator<Item = &'a str>,
        cap: &str,
        ty: &str,
    ) -> Vec<&'a FnDef> {
        let mut out: Vec<&FnDef> = self
            .reachable(roots)
            .into_iter()
            .filter_map(|k| self.fns.get(k).copied())
            .filter(|f| {
                f.params.iter().any(|p| {
                    matches!(p, Param::Cap { name, ty: t }
                        if name == cap || t.path.last().is_some_and(|n| n == ty))
                })
            })
            .collect();
        out.sort_by(|a, b| a.path.cmp(&b.path));
        out
    }

    fn walk<'a>(
        &'a self,
        start: impl IntoIterator<Item = &'a str>,
        target: Option<&str>,
    ) -> (HashSet<&'a str>, bool) {
        let mut seen = HashSet::new();
        let mut stack: Vec<&str> = start.into_iter().collect();
        while let Some(g) = stack.pop() {
            if target == Some(g) {
                return (seen, true);
            }
            if seen.insert(g) {
                stack.extend(
                    self.callees
                        .get(g)
                        .into_iter()
                        .flatten()
                        .map(String::as_str),
                );
            }
        }
        (seen, false)
    }
}

#[cfg(test)]
mod tests {
    use crate::check_in;

    fn codes(src: &str, release: bool) -> Vec<(String, u32)> {
        let src = format!("module t\n{src}");
        let (m, d) = yan_core::parse("t.yir", &src);
        assert!(d.is_empty());
        check_in("t.yir", &src, &m.unwrap(), release)
            .into_iter()
            .map(|d| (d.code, d.span.line))
            .collect()
    }

    const VIA_HELPER: &str = "fn probe(cap toolchain: Toolchain) -> i64 {\n    ret 0\n}\n\
        fn mid(cap toolchain: Toolchain) -> i64 {\n    let n: i64 = probe(cap toolchain)\n    ret n\n}\n\
        fn main(cap toolchain: Toolchain) -> i64 {\n    let n: i64 = mid(cap toolchain)\n    ret n\n}\n";

    #[test]
    fn release_reaches_toolchain_through_calls() {
        let found = codes(VIA_HELPER, true);
        assert_eq!(found.len(), 3);
        assert!(found.iter().all(|(c, _)| c == "E318"));
    }

    #[test]
    fn debug_build_binds_toolchain() {
        assert_eq!(codes(VIA_HELPER, false), []);
    }

    #[test]
    fn an_unreached_use_is_not_a_release_binding() {
        let src = "fn probe(cap toolchain: Toolchain) -> i64 {\n    ret 0\n}\nfn main() -> i64 {\n    ret 0\n}\n";
        assert_eq!(codes(src, true), []);
    }
}
