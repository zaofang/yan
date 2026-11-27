//! E320, E321: profile restrictions (17.1, R320, R321). A profile is a table of rules; each rule
//! names the functions in a reachable set that break it, so a new rule adds a row, not a code path.
//! `kernel` applies to each `@kernel` function and everything it calls; `embedded`, set by the
//! manifest, to every function of the module.

use std::collections::{HashMap, HashSet};

use yan_callgraph::{Cap, Edge, Graph};
use yan_hir as j;

use crate::{Cx, decorators, kernel, lower};

struct Facts<'a> {
    fns: HashMap<&'a str, &'a j::FuncDef>,
    externs: HashSet<&'a str>,
    records: HashMap<&'a str, &'a j::Record>,
    /// Functions calling an extern function that carries no `@stack(bytes)` (R321).
    unstacked: HashSet<&'a str>,
    graph: Graph,
}

struct Rule {
    code: &'static str,
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
            code: "E320",
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
            code: "E320",
            name: "@forbid(alloc)",
            broken: |f, reach| body_hits(f, reach, decorators::allocates),
        },
        Rule {
            code: "E320",
            name: "@forbid(rc)",
            broken: |f, reach| body_hits(f, reach, decorators::counts_rc),
        },
        Rule {
            code: "E320",
            name: "no recursion",
            broken: |f, reach| named(reach, |k| f.graph.recursive(k, &[])),
        },
        Rule {
            code: "E320",
            name: "no extern",
            broken: |f, reach| named(reach, |k| f.externs.contains(k)),
        },
        Rule {
            code: "E320",
            name: "no dyn",
            broken: |f, reach| {
                named(reach, |k| {
                    f.fns
                        .get(k)
                        .is_some_and(|d| sig_types(d).into_iter().any(indirect))
                })
            },
        },
        Rule {
            code: "E320",
            name: "only GPU-representable types",
            broken: |f, reach| {
                named(reach, |k| {
                    f.fns.get(k).is_some_and(|d| {
                        let tps = tparam_names(d);
                        sig_types(d)
                            .into_iter()
                            .any(|t| !f.gpu_type(t, &tps, &mut HashSet::new()))
                    })
                })
            },
        },
    ],
};

const EMBEDDED: Profile = Profile {
    name: "embedded",
    rules: &[
        Rule {
            code: "E320",
            name: "@forbid(rc)",
            broken: |f, reach| body_hits(f, reach, decorators::counts_rc),
        },
        Rule {
            code: "E321",
            name: "no recursion except through `become`",
            broken: |f, reach| named(reach, |k| f.graph.recursive(k, &[Edge::Become])),
        },
        Rule {
            code: "E321",
            name: "an extern callee needs `@stack(bytes)`",
            broken: |f, reach| named(reach, |k| f.unstacked.contains(k)),
        },
    ],
};

/// Scalars a kernel may hold, in both spellings of the 64-bit types.
const GPU_SCALARS: &[&str] = &[
    "bool", "int", "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "float", "f16", "bf16",
    "f32", "f64",
];

/// Containers and tile types that exist on the device.
const GPU_CONTAINERS: &[&str] = &["Vec", "Array", "DeviceArray", "Tile", "TileOrigin"];

fn sig_types(d: &j::FuncDef) -> Vec<&j::Type> {
    let params = d.params.iter().filter_map(|p| match p {
        j::Param::Value { ty, .. } | j::Param::Pack { ty, .. } => Some(ty),
        j::Param::Slash => None,
    });
    params
        .chain(d.ret.iter().filter_map(|r| r.ty.as_ref()))
        .collect()
}

fn tparam_names(d: &j::FuncDef) -> Vec<&str> {
    d.tparams
        .iter()
        .map(|t| match t {
            j::TParam::Type { name, .. } | j::TParam::Value { name, .. } => name.as_str(),
        })
        .collect()
}

/// A trait object or function value: a call through it has no static callee (R320).
fn indirect(t: &j::Type) -> bool {
    match t {
        j::Type::Dyn { .. } | j::Type::Callable { .. } => true,
        j::Type::Named { args, .. } => args
            .iter()
            .any(|a| matches!(a, j::TArg::Type(t) if indirect(t))),
        _ => false,
    }
}

impl Facts<'_> {
    /// Whether `t` is a scalar, a `Vec`, a tile or array of those, or a record of those.
    /// Indirect types are the `no dyn` rule's, not this one's.
    fn gpu_type<'s>(&'s self, t: &'s j::Type, tps: &[&str], seen: &mut HashSet<&'s str>) -> bool {
        let j::Type::Named { path, args } = t else {
            return true;
        };
        let [name] = path.as_slice() else {
            return false;
        };
        let name = name.as_str();
        let args_ok = |seen: &mut HashSet<&'s str>| {
            args.iter()
                .all(|a| !matches!(a, j::TArg::Type(t) if !self.gpu_type(t, tps, seen)))
        };
        if tps.contains(&name) || GPU_SCALARS.contains(&name) || GPU_CONTAINERS.contains(&name) {
            return args_ok(seen);
        }
        match self.records.get(name) {
            Some(r) if seen.insert(r.name.as_str()) => {
                args_ok(seen) && r.fields.iter().all(|fl| self.gpu_type(&fl.ty, &[], seen))
            }
            Some(_) => true,
            None => false,
        }
    }
}

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

/// Positions of the calls that are the operand of a `become` statement in `f`.
fn tail_calls(f: &j::FuncDef) -> HashSet<(u32, u32)> {
    let mut out = HashSet::new();
    let mut stack: Vec<&[j::Stmt]> = f.body.iter().map(|b| b.stmts.as_slice()).collect();
    while let Some(stmts) = stack.pop() {
        for s in stmts {
            if let j::StmtKind::Become(c) = &s.kind {
                out.insert((c.span.line, c.span.col));
            }
            stack.extend(lower::sub_blocks(s));
        }
    }
    out
}

fn facts(items: &[j::Item]) -> Facts<'_> {
    let fns: HashMap<&str, &j::FuncDef> = items
        .iter()
        .filter_map(|it| match &it.kind {
            j::ItemKind::Func(f) => Some((f.name.as_str(), f)),
            _ => None,
        })
        .collect();
    let extern_fns: Vec<(&str, bool)> = items
        .iter()
        .filter_map(|it| match &it.kind {
            j::ItemKind::Extern(x) => Some(x),
            _ => None,
        })
        .flat_map(|x| &x.items)
        .filter_map(|x| match x {
            j::XItem::Fn { name, attrs, .. } => {
                Some((name.as_str(), attrs.iter().any(|d| d.path == ["stack"])))
            }
            _ => None,
        })
        .collect();
    let externs: HashSet<&str> = extern_fns.iter().map(|(n, _)| *n).collect();
    let stacked: HashSet<&str> = extern_fns
        .iter()
        .filter(|(_, s)| *s)
        .map(|(n, _)| *n)
        .collect();
    let mut unstacked = HashSet::new();
    let mut graph = Graph::default();
    for (name, f) in &fns {
        let tails = tail_calls(f);
        let edges = decorators::find(f, |x| matches!(x.kind, j::ExprKind::Call(..)))
            .into_iter()
            .filter_map(|x| match &x.kind {
                j::ExprKind::Call(c, _) => match &c.kind {
                    j::ExprKind::Name(n) if fns.contains_key(n.as_str()) => {
                        let kind = if tails.contains(&(x.span.line, x.span.col)) {
                            Edge::Become
                        } else {
                            Edge::Call
                        };
                        Some((n.clone(), kind))
                    }
                    j::ExprKind::Name(n) if externs.contains(n.as_str()) => {
                        if !stacked.contains(n.as_str()) {
                            unstacked.insert(*name);
                        }
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
    let records = items
        .iter()
        .filter_map(|it| match &it.kind {
            j::ItemKind::Record(r) => Some((r.name.as_str(), r)),
            _ => None,
        })
        .collect();
    Facts {
        fns,
        externs,
        records,
        unstacked,
        graph,
    }
}

pub(crate) fn check(items: &[j::Item], profile: Option<&str>, cx: &mut Cx) {
    let roots: Vec<&j::Item> = items.iter().filter(|it| kernel::is_kernel(it)).collect();
    let embedded = profile == Some(EMBEDDED.name);
    if roots.is_empty() && !embedded {
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
                    rule.code,
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
    if embedded {
        let reach: HashSet<&str> = facts.fns.keys().copied().collect();
        for rule in EMBEDDED.rules {
            for by in (rule.broken)(&facts, &reach) {
                let Some(it) = items
                    .iter()
                    .find(|it| matches!(it.kind, j::ItemKind::Func(_)) && it.kind.name() == by)
                else {
                    continue;
                };
                cx.err(
                    rule.code,
                    it.name_span,
                    &by,
                    format!(
                        "`{by}` breaks the `{}` profile rule: {}",
                        EMBEDDED.name, rule.name
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Options, elaborate, elaborate_pkg};

    /// E320 messages for a program with one `@kernel` function calling `helper`.
    fn e320(helper: &str, call: &str) -> Vec<String> {
        let src = format!(
            "{helper}\n\n@kernel(tile: 8, 8)\n@access(a: elementwise, out: elementwise)\n\
             scale(at: TileOrigin, a: Array[64, 64, float]) -> Array[64, 64, float] =\n\
             \x20 for i in range(0, 8)\n    for j in range(0, 8)\n      out[at.i0 + i, at.i1 + j] = {call}\n\nmain() = 0\n"
        );
        let (ast, d) = yan_yn::parse("t.yn", &src);
        assert!(d.is_empty(), "{d:?}");
        let (h, d) = yan_yn::lower("t.yn", &src, &ast).expect("lowers");
        assert!(d.is_empty(), "{d:?}");
        let opts = Options {
            yn: true,
            ..Options::default()
        };
        let (_, d) = elaborate("t.yn", &src, &h, opts).expect("supported");
        d.into_iter()
            .filter(|d| d.code == "E320")
            .map(|d| d.message)
            .collect()
    }

    #[test]
    fn a_callee_that_recurses_breaks_the_kernel_profile() {
        let m = e320(
            "spin(x: float) -> float =\n  spin(x)",
            "spin(a[at.i0 + i, at.i1 + j])",
        );
        assert_eq!(m.len(), 1, "{m:?}");
        assert!(
            m[0].contains("`kernel` profile rule: no recursion"),
            "{m:?}"
        );
    }

    #[test]
    fn a_callee_that_allocates_breaks_the_kernel_profile() {
        let m = e320(
            "grow(x: float) -> float =\n  xs = [x]\n  x",
            "grow(a[at.i0 + i, at.i1 + j])",
        );
        assert!(m.iter().any(|s| s.contains("@forbid(alloc)")), "{m:?}");
    }

    #[test]
    fn a_pure_callee_passes() {
        let m = e320(
            "twice(x: float) -> float =\n  x + x",
            "twice(a[at.i0 + i, at.i1 + j])",
        );
        assert!(m.is_empty(), "{m:?}");
    }

    /// `elaborate` refuses `dyn` before diagnostics are returned, so the rule is checked on HIR.
    #[test]
    fn a_dyn_signature_is_indirect() {
        let src = "trait Op\n  run(self, x: float) -> float\n\n\
                   viaop(o: dyn Op, x: float) -> float =\n  x\n\nplain(x: float) -> float =\n  x\n";
        let (ast, d) = yan_yn::parse("t.yn", src);
        assert!(d.is_empty(), "{d:?}");
        let (h, _) = yan_yn::lower("t.yn", src, &ast).expect("lowers");
        let indirect_fns: Vec<&str> = h
            .items
            .iter()
            .filter_map(|it| match &it.kind {
                yan_hir::ItemKind::Func(f)
                    if super::sig_types(f).into_iter().any(super::indirect) =>
                {
                    Some(f.name.as_str())
                }
                _ => None,
            })
            .collect();
        assert_eq!(indirect_fns, ["viaop"]);
    }

    #[test]
    fn a_function_value_breaks_the_kernel_profile() {
        let m = e320(
            "apply(f: (float) -> float, x: float) -> float =\n  f(x)",
            "apply(sin, a[at.i0 + i, at.i1 + j])",
        );
        assert!(m.iter().any(|s| s.contains("no dyn")), "{m:?}");
    }

    #[test]
    fn a_non_gpu_type_breaks_the_kernel_profile() {
        let m = e320(
            "name(x: float, s: str) -> float =\n  x",
            "name(a[at.i0 + i, at.i1 + j], \"k\")",
        );
        assert!(
            m.iter().any(|s| s.contains("only GPU-representable types")),
            "{m:?}"
        );
    }

    #[test]
    fn a_record_of_scalars_is_gpu_representable() {
        let m = e320(
            "type P = { x: float, y: f32 }\n\nlen(p: P) -> float =\n  p.x",
            "len(P(x: a[at.i0 + i, at.i1 + j], y: 1.0))",
        );
        assert!(m.is_empty(), "{m:?}");
    }

    use crate::Manifest;

    /// `(code, message)` of every E320 and E321 for `src` under `profile = "embedded"`.
    fn embedded(src: &str, ffi: bool) -> Vec<(String, String)> {
        let (ast, d) = yan_yn::parse("t.yn", src);
        assert!(d.is_empty(), "{d:?}");
        let (h, d) = yan_yn::lower("t.yn", src, &ast).expect("lowers");
        assert!(d.is_empty(), "{d:?}");
        let opts = Options {
            yn: true,
            ffi,
            ..Options::default()
        };
        let manifest = Manifest {
            profile: Some("embedded".into()),
            ..Manifest::default()
        };
        let (_, _, d) =
            elaborate_pkg("t.yn", src, &h, opts, Some(manifest), None).expect("supported");
        d.into_iter()
            .filter(|d| d.code == "E320" || d.code == "E321")
            .map(|d| (d.code.to_string(), d.message))
            .collect()
    }

    #[test]
    fn a_cycle_breaks_the_embedded_stack_bound() {
        let m = embedded(
            "ping(n: int) -> int = pong(n)\n\npong(n: int) -> int = ping(n)\n\nmain() = 0\n",
            false,
        );
        assert_eq!(m.len(), 2, "{m:?}");
        assert!(
            m.iter()
                .all(|(c, s)| c == "E321" && s.contains("no recursion except")),
            "{m:?}"
        );
    }

    #[test]
    fn a_become_cycle_is_a_loop() {
        let m = embedded(
            "count(n: int, acc: int) -> int =\n  if n == 0\n    return acc\n  become count(n - 1, acc + 1)\n\nmain() = 0\n",
            false,
        );
        assert!(m.is_empty(), "{m:?}");
    }

    #[test]
    fn an_acyclic_program_passes() {
        let m = embedded("leaf(n: int) -> int = n\n\nmain() = leaf(1)\n", false);
        assert!(m.is_empty(), "{m:?}");
    }

    #[test]
    fn an_extern_callee_needs_a_stack_claim() {
        let m = embedded(
            "unsafe extern \"C\" lib m link \"m\"\n  safe cos(x: f64) -> f64\n\nf(x: f64) -> f64 uses m = cos(x)\n\nmain() = 0\n",
            true,
        );
        assert!(
            m.iter()
                .any(|(c, s)| c == "E321" && s.contains("`@stack(bytes)`")),
            "{m:?}"
        );
    }

    #[test]
    fn a_stack_claim_on_the_extern_item_passes() {
        let m = embedded(
            "unsafe extern \"C\" lib m link \"m\"\n  @stack(64)\n  safe cos(x: f64) -> f64\n\nf(x: f64) -> f64 uses m = cos(x)\n\nmain() = 0\n",
            true,
        );
        assert!(m.is_empty(), "{m:?}");
    }
}
