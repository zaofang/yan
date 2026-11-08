//! Doc lints W831 to W833 (R317, 18.9) on function docstrings, read from the source text.

use std::collections::HashSet;

use yan_diag::{Diagnostic, Severity, Span};
use yan_hir as j;
use yan_names::Surface;

/// `sub_items` enables W861, the opt-in missing sub-item doc lint (`yan.toml`, R848).
pub(crate) fn lint(
    file: &str,
    src: &str,
    m: &j::Module,
    surface: Surface,
    sub_items: bool,
) -> Vec<Diagnostic> {
    let names = item_names(m);
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    for it in &m.items {
        let j::ItemKind::Func(f) = &it.kind else {
            continue;
        };
        let has_doc = f.body.as_ref().is_some_and(|b| b.doc.is_some());
        let doc = if has_doc {
            doc_lines(&lines, f.name_span.line as usize)
        } else {
            None
        };
        if surface == Surface::Yag {
            out.extend(param_docs(file, &lines, it.is_pub, sub_items, f, doc));
        }
        let Some((first, last)) = doc else {
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

struct ArgsEntry<'a> {
    name: &'a str,
    ty: Option<&'a str>,
    line: usize,
    col: usize,
}

/// The `name: text` and `name (type): text` entries of the `Args:` section in the docstring.
fn args_entries<'a>(lines: &[&'a str], (first, last): (usize, usize)) -> Vec<ArgsEntry<'a>> {
    let mut out = Vec::new();
    let mut header: Option<usize> = None;
    let mut entry_indent = 0;
    for (k, line) in lines.iter().enumerate().take(last + 1).skip(first) {
        let t = line.trim_start();
        let indent = line.len() - t.len();
        if t.trim_end() == "Args:" {
            header = Some(indent);
            entry_indent = usize::MAX;
            continue;
        }
        let Some(base) = header else { continue };
        if t.trim().is_empty() {
            continue;
        }
        if indent <= base {
            header = None;
            continue;
        }
        entry_indent = entry_indent.min(indent);
        if indent != entry_indent {
            continue;
        }
        let Some((head, _)) = t.split_once(':') else {
            continue;
        };
        let (name, ty) = match head.split_once(" (") {
            Some((n, rest)) => (n, rest.strip_suffix(')')),
            None => (head, None),
        };
        if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            out.push(ArgsEntry {
                name,
                ty,
                line: k,
                col: indent,
            });
        }
    }
    out
}

/// E280, E281, W860 and W861 for the parameter docs of a function (R846 to R849).
fn param_docs(
    file: &str,
    lines: &[&str],
    is_pub: bool,
    sub_items: bool,
    f: &j::FuncDef,
    doc: Option<(usize, usize)>,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let mut report = |code: &str, line: usize, col: usize, len: usize, msg: String| {
        let span = Span {
            line: line as u32 + 1,
            col: col as u32 + 1,
            end_line: line as u32 + 1,
            end_col: (col + len) as u32 + 1,
        };
        let mut d = Diagnostic::error(code, file, span, &lines[line][col..col + len], msg)
            .with_item(&f.name);
        if code.starts_with('W') {
            d.severity = Severity::Warning;
        }
        out.push(d);
    };
    let params: Vec<(&str, &j::Type)> = f
        .params
        .iter()
        .filter_map(|p| match p {
            j::Param::Value { name, ty, .. } | j::Param::Pack { name, ty } => {
                Some((name.as_str(), ty))
            }
            j::Param::Slash => None,
        })
        .collect();
    let sig_end = doc.map_or(lines.len(), |(first, _)| first);
    let above: HashSet<&str> = (f.name_span.line as usize..sig_end)
        .filter(|&k| k > 0 && lines[k - 1].trim_start().starts_with("#:"))
        .filter_map(|k| lines[k].trim_start().split_once(':').map(|(n, _)| n.trim()))
        .collect();
    let entries = doc.map(|d| args_entries(lines, d)).unwrap_or_default();
    for e in &entries {
        let Some((_, ty)) = params.iter().find(|(n, _)| *n == e.name) else {
            report(
                "W860",
                e.line,
                e.col,
                e.name.len(),
                format!("`Args:` documents `{}`, which is not a parameter", e.name),
            );
            continue;
        };
        if above.contains(e.name) {
            report(
                "E280",
                e.line,
                e.col,
                e.name.len(),
                format!("`{}` is documented here and above the parameter", e.name),
            );
        }
        if let Some(written) = e.ty {
            let sig = yan_yag::print_type(ty);
            let squash = |s: &str| s.split_whitespace().collect::<String>();
            if squash(written) != squash(&sig) {
                report(
                    "E281",
                    e.line,
                    e.col,
                    e.name.len(),
                    format!(
                        "`{}` is written as `{written}`, the signature says `{sig}`",
                        e.name
                    ),
                );
            }
        }
    }
    if is_pub && sub_items {
        let documented: HashSet<&str> = entries.iter().map(|e| e.name).collect();
        for (name, _) in params.iter().filter(|(n, _)| *n != "self") {
            if documented.contains(name) || above.contains(name) {
                continue;
            }
            let line = f.name_span.line as usize - 1;
            let at = (line..sig_end.min(lines.len()))
                .find_map(|k| {
                    let t = lines[k].trim_start();
                    let ind = lines[k].len() - t.len();
                    let from = if k == line {
                        f.name_span.col as usize
                    } else {
                        ind
                    };
                    lines[k][from.min(lines[k].len())..]
                        .find(name)
                        .map(|c| (k, from + c))
                })
                .unwrap_or((line, f.name_span.col as usize - 1));
            if lines[at.0].len() >= at.1 + name.len() {
                report(
                    "W861",
                    at.0,
                    at.1,
                    name.len(),
                    format!("parameter `{name}` of a public function has no doc"),
                );
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
