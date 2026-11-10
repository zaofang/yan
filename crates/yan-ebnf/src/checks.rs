//! Structural checks every surface's grammar must pass; used from each surface's tests.

use crate::*;
use std::collections::{HashMap, HashSet};

pub fn no_left_recursion(g: &Grammar) {
    fn nullable(e: &Expr, n: &HashSet<&str>) -> bool {
        match e {
            Expr::Lit(_) => false,
            Expr::Term(t) => matches!(t, Term::Indent | Term::Dedent | Term::Bol),
            Expr::Ref(r) => n.contains(r.as_str()),
            Expr::Seq(v) => v.iter().all(|e| nullable(e, n)),
            Expr::Alt(v) => v.iter().any(|e| nullable(e, n)),
            Expr::Opt(_) | Expr::Rep(_) => true,
        }
    }
    let mut null = HashSet::new();
    while let Some(r) = g
        .rules
        .iter()
        .find(|(r, e)| !null.contains(r.as_str()) && nullable(e, &null))
    {
        null.insert(r.0.as_str());
    }
    fn left<'a>(e: &'a Expr, n: &HashSet<&str>, out: &mut Vec<&'a str>) {
        match e {
            Expr::Ref(r) => out.push(r),
            Expr::Seq(v) => {
                for e in v {
                    left(e, n, out);
                    if !nullable(e, n) {
                        break;
                    }
                }
            }
            Expr::Alt(v) => v.iter().for_each(|e| left(e, n, out)),
            Expr::Opt(e) | Expr::Rep(e) => left(e, n, out),
            _ => {}
        }
    }
    let edges: HashMap<&str, Vec<&str>> = g
        .rules
        .iter()
        .map(|(r, e)| {
            let mut v = Vec::new();
            left(e, &null, &mut v);
            (r.as_str(), v)
        })
        .collect();
    for start in edges.keys() {
        let mut seen = HashSet::new();
        let mut stack = edges[start].clone();
        while let Some(r) = stack.pop() {
            assert_ne!(r, *start, "left recursion through {start}");
            if seen.insert(r) {
                stack.extend(edges[r].iter().copied());
            }
        }
    }
}

pub fn gbnf_is_closed_and_reachable(g: &Grammar) {
    let text = g.gbnf();
    let defs: HashSet<&str> = text
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once(" ::= ").map(|x| x.0))
        .collect();
    let mut used = HashSet::new();
    for l in text.lines().filter(|l| !l.starts_with('#')) {
        let Some((_, body)) = l.split_once(" ::= ") else {
            continue;
        };
        let mut in_str = false;
        let mut in_class = false;
        let mut esc = false;
        let mut word = String::new();
        for c in body.chars().chain([' ']) {
            if esc {
                esc = false;
                continue;
            }
            match c {
                '\\' => esc = true,
                '"' if !in_class => in_str = !in_str,
                '[' if !in_str => in_class = true,
                ']' if !in_str => in_class = false,
                c if !in_str && !in_class && (c.is_ascii_alphanumeric() || c == '-') => {
                    word.push(c)
                }
                _ => {
                    if !word.is_empty() {
                        used.insert(std::mem::take(&mut word));
                    }
                }
            }
        }
    }
    let undefined: Vec<_> = used.iter().filter(|u| !defs.contains(u.as_str())).collect();
    assert!(undefined.is_empty(), "{undefined:?}");
    assert!(defs.contains("module-d0"));
    assert!(defs.contains("stmt-d8") && !defs.contains("stmt-d9"));
}

fn lark_names(text: &str) -> (HashSet<String>, HashSet<String>) {
    let (mut defs, mut used) = (HashSet::new(), HashSet::new());
    for l in text.lines().filter(|l| !l.starts_with("//")) {
        let Some((name, body)) = l.split_once(": ") else {
            continue;
        };
        defs.insert(name.to_owned());
        let (mut quote, mut esc, mut word) = (None, false, String::new());
        for c in body.chars().chain([' ']) {
            if esc {
                esc = false;
            } else if c == '\\' && quote.is_some() {
                esc = true;
            } else if let Some(q) = quote {
                if c == q {
                    quote = None;
                }
            } else if c == '"' || c == '/' {
                quote = Some(c);
            } else if c.is_ascii_alphanumeric() || c == '_' {
                word.push(c);
            } else if !word.is_empty() {
                used.insert(std::mem::take(&mut word));
            }
        }
    }
    (defs, used)
}

pub fn llguidance_is_closed_and_reachable(g: &Grammar) {
    let text = g.llguidance();
    let (defs, mut used) = lark_names(&text);
    used.insert("module_d0".into());
    let undefined: Vec<_> = used.iter().filter(|u| !defs.contains(*u)).collect();
    assert!(undefined.is_empty(), "{undefined:?}");
    assert!(defs.contains("start") && defs.contains("module_d0"));
    assert!(defs.contains("stmt_d8") && !defs.contains("stmt_d9"));
    assert!(!text.contains("::=") && !text.contains("-d"));
}

pub fn xgrammar_differs_from_gbnf_only_in_header_and_dot(g: &Grammar) {
    let strip = |t: String| -> Vec<String> {
        t.lines()
            .filter(|l| !l.starts_with('#'))
            .map(str::to_owned)
            .collect()
    };
    let (a, b) = (strip(g.gbnf()), strip(g.xgrammar()));
    let diff: Vec<_> = a.iter().zip(&b).filter(|(x, y)| x != y).collect();
    assert_eq!(diff.len(), 1, "{diff:?}");
    assert!(diff[0].1.starts_with("tqbody"));
    assert!(!g.xgrammar().contains(" . "));
}

pub fn output_is_byte_stable(a: &Grammar, b: &Grammar) {
    assert_eq!(a.gbnf(), b.gbnf());
    assert_eq!(a.llguidance(), b.llguidance());
    assert_eq!(a.xgrammar(), b.xgrammar());
}
