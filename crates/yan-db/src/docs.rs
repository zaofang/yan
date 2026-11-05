//! Doc lints W831 to W833 (R317, 18.9) on function docstrings, read from the source text.

use std::collections::HashSet;

use yan_diag::{Diagnostic, Severity, Span};
use yan_hir as j;
use yan_names::Surface;

pub(crate) fn lint(file: &str, src: &str, m: &j::Module, surface: Surface) -> Vec<Diagnostic> {
    let names = item_names(m);
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    for it in &m.items {
        let j::ItemKind::Func(f) = &it.kind else {
            continue;
        };
        if f.body.as_ref().is_none_or(|b| b.doc.is_none()) {
            continue;
        }
        let Some((first, last)) = doc_lines(&lines, f.name_span.line as usize) else {
            continue;
        };
        let mut warn = |code: &str, line: usize, col: usize, len: usize, msg: String| {
            let span = Span {
                line: line as u32 + 1,
                col: col as u32 + 1,
                end_line: line as u32 + 1,
                end_col: (col + len) as u32 + 1,
            };
            let quote = &lines[line][col..col + len];
            let mut d = Diagnostic::error(code, file, span, quote, msg).with_item(&f.name);
            d.severity = Severity::Warning;
            out.push(d);
        };
        let mut fence: Option<(usize, usize, Vec<&str>)> = None;
        for (k, line) in lines.iter().enumerate().take(last + 1).skip(first) {
            let t = line.trim_start();
            let indent = line.len() - t.len();
            if let Some(tag) = t.strip_prefix("```") {
                match fence.take() {
                    Some((at, col, body)) if is_code(&body.join("\n"), surface) => warn(
                        "W833",
                        at,
                        col,
                        4,
                        "this fence is tagged `text` but parses as code; retag it".into(),
                    ),
                    Some(_) => {}
                    None if tag.trim() == "text" => fence = Some((k, indent + 3, Vec::new())),
                    None => fence = Some((k, usize::MAX, Vec::new())),
                }
                continue;
            }
            if let Some((_, col, body)) = &mut fence {
                if *col != usize::MAX {
                    body.push(t);
                }
                continue;
            }
            for (col, name) in links(line) {
                if !names.contains(name) {
                    warn(
                        "W831",
                        k,
                        col,
                        name.len(),
                        format!("`[{name}]` links to nothing"),
                    );
                }
            }
            for (section, written) in [("Uses:", uses(f)), ("Fails:", fails(f))] {
                if let Some(rest) = t.strip_prefix(section) {
                    let said: HashSet<&str> = rest
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .collect();
                    let sig: HashSet<&str> = written.iter().map(String::as_str).collect();
                    if said != sig {
                        let mut sig: Vec<&str> = sig.into_iter().collect();
                        sig.sort_unstable();
                        warn(
                            "W832",
                            k,
                            indent,
                            section.len(),
                            format!(
                                "the `{section}` section disagrees with the signature: {}",
                                sig.join(", ")
                            ),
                        );
                    }
                }
            }
        }
    }
    out
}

/// The source lines of the docstring of the function declared on (1-based) line `def`.
fn doc_lines(lines: &[&str], def: usize) -> Option<(usize, usize)> {
    let first = (def..lines.len()).find(|&k| lines[k].trim_start().starts_with("\"\"\""))?;
    let rest = &lines[first].trim_start()[3..];
    if rest.contains("\"\"\"") {
        return Some((first, first));
    }
    let last = (first + 1..lines.len()).find(|&k| lines[k].contains("\"\"\""))?;
    Some((first, last))
}

/// `[Name]` links in a doc line, by byte column of the name.
fn links(line: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let b = line.as_bytes();
    for (i, _) in line.match_indices('[') {
        let word_before = i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_');
        let Some(len) = line[i + 1..].find(']') else {
            continue;
        };
        let name = &line[i + 1..i + 1 + len];
        let ident = name.split('.').all(|p| {
            p.chars()
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_')
                && p.chars().all(|c| c.is_alphanumeric() || c == '_')
        });
        if !word_before && ident && line.as_bytes().get(i + 2 + len) != Some(&b'(') {
            out.push((i + 1, name));
        }
    }
    out
}

fn item_names(m: &j::Module) -> HashSet<String> {
    let mut out = HashSet::new();
    for it in &m.items {
        match &it.kind {
            j::ItemKind::Func(f) | j::ItemKind::Lemma(f) => {
                out.insert(f.name.clone());
            }
            j::ItemKind::Record(r) => {
                out.insert(r.name.clone());
            }
            j::ItemKind::Type(t) => {
                out.insert(t.name.clone());
                if let j::TypeRhs::Sum(vs) = &t.rhs {
                    out.extend(vs.iter().map(|v| v.name.clone()));
                }
            }
            j::ItemKind::Trait(t) => {
                out.insert(t.name.clone());
                out.extend(t.sigs.iter().map(|(_, f)| format!("{}.{}", t.name, f.name)));
            }
            j::ItemKind::Const { name, .. } => {
                out.insert(name.clone());
            }
            j::ItemKind::Impl(_) | j::ItemKind::Extern(_) | j::ItemKind::Verbatim(_) => {}
        }
    }
    out
}

fn uses(f: &j::FuncDef) -> Vec<String> {
    match &f.uses {
        Some(j::Uses::Caps(cs)) => cs
            .iter()
            .map(|c| c.alias.clone().unwrap_or_else(|| c.path.join(".")))
            .collect(),
        _ => Vec::new(),
    }
}

fn fails(f: &j::FuncDef) -> Vec<String> {
    f.ret
        .iter()
        .flat_map(|r| r.fail.iter().map(|p| p.join(".")))
        .collect()
}

/// Whether a `text` fence's body parses as statements of the file's surface.
fn is_code(body: &str, surface: Surface) -> bool {
    if body.trim().is_empty() {
        return false;
    }
    let indented: String = body.lines().map(|l| format!("    {l}\n")).collect();
    match surface {
        Surface::Yag => {
            let src = format!("def doc_probe():\n{indented}");
            let (m, d) = yan_yag::parse("doc", &src, true);
            m.is_some() && d.iter().all(|d| d.severity != Severity::Error)
        }
        _ => false,
    }
}
