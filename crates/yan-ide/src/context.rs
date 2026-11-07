//! Agent context queries (21.4): `search_type` over std and `pack_context` over a package.

use std::collections::{BTreeMap, HashSet, VecDeque};

use yan_hir::{Item, ItemKind, Module, Param, TArg, Type};

use super::{header, single};

/// A std function whose type matches a query.
pub struct TypeHit {
    pub name: String,
    pub signature: String,
}

/// Std functions matching a type, best first: a whole-signature match, then a return type,
/// then a parameter type. A single capital letter in the query, and any type parameter of the
/// function, matches every type. A `Callable` query may leave off trailing defaulted parameters.
pub fn search_type(query: &str) -> Result<Vec<TypeHit>, String> {
    let q = parse_type(query)?;
    let mut hits: Vec<(u8, String, String)> = Vec::new();
    for (module, _, _, m) in yan_db::std_modules() {
        for it in &m.items {
            let ItemKind::Func(f) = &it.kind else {
                continue;
            };
            let tparams: HashSet<&str> = f
                .tparams
                .iter()
                .filter_map(|t| match t {
                    yan_hir::TParam::Type { name, .. } => Some(name.as_str()),
                    yan_hir::TParam::Value { .. } => None,
                })
                .collect();
            let params: Vec<(&Type, bool)> = f
                .params
                .iter()
                .filter_map(|p| match p {
                    Param::Value { ty, default, .. } => Some((ty, default.is_some())),
                    _ => None,
                })
                .collect();
            let ret = f.ret.as_ref().and_then(|r| r.ty.as_ref());
            let rank = match &q {
                Type::Callable {
                    params: qp,
                    ret: qr,
                    ..
                } => {
                    let fits = qp.len() <= params.len()
                        && params[qp.len()..].iter().all(|&(_, d)| d)
                        && qp
                            .iter()
                            .zip(&params)
                            .all(|(a, &(b, _))| unify(a, b, &tparams))
                        && unify(qr, ret.unwrap_or(&Type::None), &tparams);
                    fits.then_some(0)
                }
                _ => ret
                    .filter(|r| unify(&q, r, &tparams))
                    .map(|_| 1)
                    .or_else(|| {
                        params
                            .iter()
                            .any(|&(p, _)| unify(&q, p, &tparams))
                            .then_some(2)
                    }),
            };
            if let Some(rank) = rank {
                let name = format!("{module}.{}", f.name);
                hits.push((rank, name, signature(it)));
            }
        }
    }
    hits.sort();
    Ok(hits
        .into_iter()
        .map(|(_, name, signature)| TypeHit { name, signature })
        .collect())
}

fn parse_type(text: &str) -> Result<Type, String> {
    let src = format!("def q(x: {text}) -> None: ...\n");
    let (m, _) = yan_yag::parse("query.yag", &src, false);
    let f = m.and_then(|m| match m.items.into_iter().next()?.kind {
        ItemKind::Func(f) => Some(f),
        _ => None,
    });
    match f.and_then(|f| f.params.into_iter().next()) {
        Some(Param::Value { ty, .. }) => Ok(ty),
        _ => Err(format!("`{text}` is not a type")),
    }
}

fn unify(q: &Type, s: &Type, tparams: &HashSet<&str>) -> bool {
    let wild = |t: &Type, f: &dyn Fn(&str) -> bool| match t {
        Type::Named { path, args } => args.is_empty() && matches!(path.as_slice(), [n] if f(n)),
        _ => false,
    };
    if wild(q, &|n| {
        n.len() == 1 && n.starts_with(|c: char| c.is_ascii_uppercase())
    }) || wild(s, &|n| tparams.contains(n))
    {
        return true;
    }
    match (q, s) {
        (Type::None, Type::None) => true,
        (Type::Named { path: p, args: a }, Type::Named { path: r, args: b }) => {
            p == r
                && a.len() == b.len()
                && a.iter().zip(b).all(|(x, y)| match (x, y) {
                    (TArg::Type(x), TArg::Type(y)) => unify(x, y, tparams),
                    (x, y) => x == y,
                })
        }
        (
            Type::Callable {
                params: p, ret: a, ..
            },
            Type::Callable {
                params: r, ret: b, ..
            },
        ) => {
            p.len() == r.len()
                && p.iter().zip(r).all(|(x, y)| unify(x, y, tparams))
                && unify(a, b, tparams)
        }
        _ => false,
    }
}

/// The declaration line(s) of a function, without its docstring or contracts.
fn signature(it: &Item) -> String {
    let text = header(&yan_yag::print(&single(it)));
    let mut lines = Vec::new();
    for l in text.lines().filter(|l| !l.starts_with('@')) {
        lines.push(l);
        if l.trim_end().ends_with(':') {
            break;
        }
    }
    lines.join("\n")
}

/// One item of a packed context.
pub struct Packed {
    pub name: String,
    pub distance: usize,
    pub text: String,
}

/// The signatures, contracts and examples of the items a task names and of the items they
/// depend on, nearest first, within `budget` tokens (four bytes each). A task that names no
/// item gets every item in module order. Docstrings are fenced as untrusted and characters
/// that hide text are removed (21.4).
pub fn pack_context(modules: &[(String, Module)], task: &str, budget: usize) -> Vec<Packed> {
    struct Node {
        name: String,
        bare: String,
        module: String,
        text: String,
        idents: HashSet<String>,
    }
    let mut nodes: Vec<Node> = Vec::new();
    for (module, m) in modules {
        for it in &m.items {
            if matches!(it.kind, ItemKind::Impl(_) | ItemKind::Verbatim(_)) {
                continue;
            }
            let full = yan_yag::print(&single(it));
            let bare = it.kind.name().to_owned();
            let text = match &it.kind {
                ItemKind::Func(_) | ItemKind::Lemma(_) => header(&full),
                _ => full.trim_end().to_owned(),
            };
            nodes.push(Node {
                name: format!("{module}.{bare}"),
                bare,
                module: module.clone(),
                text: fence(&strip_hidden(&text)),
                idents: idents(&full),
            });
        }
    }

    let task_idents = idents(task);
    let mut dist: BTreeMap<usize, usize> = BTreeMap::new();
    let mut queue = VecDeque::new();
    for (i, n) in nodes.iter().enumerate() {
        let dotted = format!("{}.{}", n.module, n.bare);
        if task_idents.contains(&n.bare) || task.contains(&dotted) {
            dist.insert(i, 0);
            queue.push_back(i);
        }
    }
    if dist.is_empty() {
        dist.extend((0..nodes.len()).map(|i| (i, 0)));
    }
    while let Some(i) = queue.pop_front() {
        let d = dist[&i];
        for (j, n) in nodes.iter().enumerate() {
            if !dist.contains_key(&j) && nodes[i].idents.contains(&n.bare) {
                dist.insert(j, d + 1);
                queue.push_back(j);
            }
        }
    }

    let mut order: Vec<(usize, usize)> = dist.into_iter().map(|(i, d)| (d, i)).collect();
    order.sort();
    let mut used = 0;
    let mut out = Vec::new();
    for (distance, i) in order {
        let cost = nodes[i].text.len() / 4 + 1;
        if used + cost > budget {
            continue;
        }
        used += cost;
        out.push(Packed {
            name: nodes[i].name.clone(),
            distance,
            text: nodes[i].text.clone(),
        });
    }
    out
}

fn idents(text: &str) -> HashSet<String> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Zero-width, bidirectional-control and tag characters.
fn strip_hidden(text: &str) -> String {
    text.chars()
        .filter(|&c| {
            !matches!(c,
                '\u{200B}'..='\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{2069}'
                | '\u{FEFF}'
                | '\u{E0000}'..='\u{E007F}')
        })
        .collect()
}

/// Wraps each docstring in `<untrusted>` markers; it is repository text, not an instruction.
fn fence(text: &str) -> String {
    let mut out = Vec::new();
    let mut in_doc = false;
    for l in text.lines() {
        let t = l.trim_start();
        let opens = !in_doc && t.starts_with("\"\"\"");
        if opens {
            out.push("<untrusted>".to_owned());
        }
        out.push(l.to_owned());
        if opens || in_doc {
            let closes = t.ends_with("\"\"\"") && (!opens || t.len() >= 6);
            in_doc = !closes;
            if closes {
                out.push("</untrusted>".to_owned());
            }
        }
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_callable_query_finds_list_sort() {
        let hits = search_type("Callable[[list[T]], None]").unwrap();
        assert!(hits.iter().any(|h| h.name == "list.sort"), "{}", hits.len());
        let hits = search_type("Callable[[list[T], Callable[[T], K] | None, bool], None]").unwrap();
        assert_eq!(hits[0].name, "list.sort");
        assert!(hits[0].signature.starts_with("pub def sort"));
    }

    #[test]
    fn a_bad_query_is_an_error() {
        assert!(search_type("(((").is_err());
    }

    fn module(src: &str) -> Module {
        yan_yag::parse("m.yag", src, false).0.expect("parses")
    }

    #[test]
    fn pack_context_follows_dependencies_within_budget() {
        let mut src = String::new();
        for i in 0..30 {
            let dep = if i == 0 {
                "f1()".to_owned()
            } else {
                format!("f{}()", i + 1)
            };
            src += &format!("def f{i}() -> int:\n    \"\"\"doc {i}\"\"\"\n    return {dep}\n\n");
        }
        let m = vec![("m".to_owned(), module(&src))];
        let packed = pack_context(&m, "change f0", 40);
        assert_eq!(packed[0].name, "m.f0");
        assert_eq!(packed[1].name, "m.f1");
        let cost: usize = packed.iter().map(|p| p.text.len() / 4 + 1).sum();
        assert!(cost <= 40 && packed.len() < 30, "{cost}");
        assert!(packed[0].text.contains("<untrusted>"));
    }

    #[test]
    fn hidden_characters_are_removed() {
        assert_eq!(strip_hidden("a\u{200B}b\u{202E}c\u{E0041}"), "abc");
    }
}
