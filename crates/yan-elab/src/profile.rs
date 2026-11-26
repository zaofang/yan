//! E320: profile restrictions (17.1, R320). A profile is a table of rules; each rule names the
//! functions in a reachable set that break it, so a new rule adds a row, not a code path. `kernel`
//! applies to each `@kernel` function and everything it calls.

use std::collections::{HashMap, HashSet};

use yan_callgraph::{Cap, Edge, Graph};
use yan_hir as j;

use crate::{Cx, decorators, kernel};

struct Facts<'a> {
    fns: HashMap<&'a str, &'a j::FuncDef>,
    externs: HashSet<&'a str>,
    graph: Graph,
}

struct Rule {
    name: &'static str,
    /// The functions of `reach` that break the rule, in name order.
    broken: fn(&Facts, &HashSet<&str>) -> Vec<String>,
}

struct Profile {
    name: &'static str,
    rules: &'static [Rule],
}

/// `kernel::prepare` gives every `@kernel` function `uses gpu`, the tile intrinsics' capability.
const KERNEL: Profile = Profile {
    name: "kernel",
    rules: &[
        Rule {
            name: "no capability but gpu",
            broken: |f, reach| {
                named(reach, |k| {
                    f.fns
                        .get(k)
                        .is_some_and(|d| matches!(&d.uses, Some(j::Uses::Caps(c)) if c.iter().any(|r| r.path != ["gpu"])))
                })
            },
        },
        Rule {
            name: "@forbid(alloc)",
            broken: |f, reach| body_hits(f, reach, decorators::allocates),
        },
        Rule {
            name: "@forbid(rc)",
            broken: |f, reach| body_hits(f, reach, decorators::counts_rc),
        },
        Rule {
            name: "no recursion",
            broken: |f, reach| named(reach, |k| f.graph.recursive(k, &[])),
        },
        Rule {
            name: "no extern",
            broken: |f, reach| named(reach, |k| f.externs.contains(k)),
        },
    ],
};

fn named(reach: &HashSet<&str>, pred: impl Fn(&str) -> bool) -> Vec<String> {
    let mut out: Vec<String> = reach
        .iter()
        .filter(|k| pred(k))
        .map(|k| (*k).to_owned())
        .collect();
    out.sort_unstable();
    out
}

fn body_hits(f: &Facts, reach: &HashSet<&str>, hit: fn(&j::Expr) -> bool) -> Vec<String> {
    named(reach, |k| {
        f.fns
            .get(k)
            .is_some_and(|d| !decorators::find(d, hit).is_empty())
    })
}

fn facts(items: &[j::Item]) -> Facts<'_> {
    let fns: HashMap<&str, &j::FuncDef> = items
        .iter()
        .filter_map(|it| match &it.kind {
            j::ItemKind::Func(f) => Some((f.name.as_str(), f)),
            _ => None,
        })
        .collect();
    let externs: HashSet<&str> = items
        .iter()
        .filter_map(|it| match &it.kind {
            j::ItemKind::Extern(x) => Some(x),
            _ => None,
        })
        .flat_map(|x| &x.items)
        .filter_map(|x| match x {
            j::XItem::Fn { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    let mut graph = Graph::default();
    for (name, f) in &fns {
        let edges = decorators::find(f, |x| matches!(x.kind, j::ExprKind::Call(..)))
            .into_iter()
            .filter_map(|x| match &x.kind {
                j::ExprKind::Call(c, _) => match &c.kind {
                    j::ExprKind::Name(n) if fns.contains_key(n.as_str()) => {
                        Some((n.clone(), Edge::Call))
                    }
                    j::ExprKind::Name(n) if externs.contains(n.as_str()) => {
                        Some((n.clone(), Edge::Extern))
                    }
                    _ => None,
                },
                _ => None,
            })
            .collect();
        let uses = match &f.uses {
            Some(j::Uses::Caps(c)) => c
                .iter()
                .map(|r| {
                    let ty = r.path.last().cloned().unwrap_or_default();
                    Cap {
                        name: r.alias.clone().unwrap_or_else(|| ty.clone()),
                        ty,
                    }
                })
                .collect(),
            _ => Vec::new(),
        };
        graph.add_fn(name, edges, uses);
    }
    Facts {
        fns,
        externs,
        graph,
    }
}

pub(crate) fn check(items: &[j::Item], cx: &mut Cx) {
    let roots: Vec<&j::Item> = items.iter().filter(|it| kernel::is_kernel(it)).collect();
    if roots.is_empty() {
        return;
    }
    let facts = facts(items);
    for it in roots {
        let name = it.kind.name();
        let reach = facts.graph.reachable([name]);
        for rule in KERNEL.rules {
            for by in (rule.broken)(&facts, &reach) {
                let at = if by == name {
                    String::new()
                } else {
                    format!(" (in `{by}`, which it calls)")
                };
                cx.err(
                    "E320",
                    it.name_span,
                    name,
                    format!(
                        "`{name}` is `@kernel` and breaks the `{}` profile rule: {}{at}",
                        KERNEL.name, rule.name
                    ),
                );
            }
        }
    }
}
