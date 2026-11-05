//! The analysis API (22.2, R560) over the query database: diagnostics, hover, inlay hints and
//! view-as. `yan-cli` and `yan mcp` use it, and the language server will.

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;
use yan_core::ast as y;
use yan_db::{Db, Front, Loaded, SourceFile};
use yan_diag::{Diagnostic, Span};
use yan_names::Surface;

pub use yan_db::Database;

/// Every diagnostic of the module, or a tool failure (an unsupported construct, no front end).
/// Each fix is kept only once it is verified (R292).
pub fn diagnostics(db: &dyn Db, file: SourceFile) -> Result<Vec<Diagnostic>, String> {
    let mut diags = yan_db::diagnostics(db, file).clone()?;
    verify_fixes(db, file, &mut diags);
    yan_diag::habit::annotate(file.text(db), &mut diags);
    unique_quotes(db, file, &mut diags);
    Ok(diags)
}

/// Grows each quote until it occurs once in its item (R293).
fn unique_quotes(db: &dyn Db, file: SourceFile, diags: &mut [Diagnostic]) {
    let src = file.text(db);
    let lines: Vec<&str> = src.lines().collect();
    let items: Vec<Span> = match yan_db::front(db, file) {
        Ok(Front::Hir(m, _)) => m.items.iter().map(|i| i.span).collect(),
        Ok(Front::Yir(m)) => m.items.iter().map(|i| i.span).collect(),
        _ => Vec::new(),
    };
    for d in diags {
        let Some(&line) = lines.get((d.span.line as usize).wrapping_sub(1)) else {
            continue;
        };
        let from = (d.span.col as usize).saturating_sub(1);
        let to = from + d.quote.len();
        if d.quote.is_empty() || line.get(from..to) != Some(d.quote.as_str()) {
            continue;
        }
        let item = match items
            .iter()
            .find(|s| (s.line..=s.end_line).contains(&d.span.line))
        {
            Some(s) => {
                lines[s.line as usize - 1..(s.end_line as usize).min(lines.len())].join("\n")
            }
            None => src.to_owned(),
        };
        d.quote = yan_diag::unique_quote(line, from, to, &item).to_owned();
    }
}

/// Applies each fix to a copy of the file and re-checks it. A fix that removes its diagnostic
/// and adds none is marked `verified: true`; any other fix is dropped (R292).
fn verify_fixes(db: &dyn Db, file: SourceFile, diags: &mut [Diagnostic]) {
    let settings = yan_db::Settings::get(db);
    let (strict, release) = (settings.strict(db), settings.release(db));
    let codes: Vec<String> = diags.iter().map(|d| d.code.clone()).collect();
    for (i, d) in diags.iter_mut().enumerate() {
        let mut allowed = codes.clone();
        allowed.remove(i);
        let fixes = std::mem::take(&mut d.fixes);
        for mut fix in fixes {
            let Some(text) = apply_edits(file.text(db), &fix["edits"]) else {
                continue;
            };
            let mut fresh = Database::new(strict, release);
            let f = fresh.file(file.path(db), &text);
            let Ok(after) = yan_db::diagnostics(&fresh, f) else {
                continue;
            };
            let mut left = allowed.clone();
            let adds_none = after.iter().all(|a| {
                left.iter()
                    .position(|c| *c == a.code)
                    .map(|k| left.swap_remove(k))
                    .is_some()
            });
            if adds_none {
                fix["verified"] = serde_json::Value::Bool(true);
                d.fixes.push(fix);
            }
        }
    }
}

/// `src` with each single-line edit applied, or `None` when an edit's span does not hold its
/// `search` text.
fn apply_edits(src: &str, edits: &serde_json::Value) -> Option<String> {
    let mut lines: Vec<String> = src.split('\n').map(str::to_owned).collect();
    let mut edits: Vec<_> = edits.as_array()?.iter().collect();
    edits.sort_by_key(|e| (e["span"]["line"].as_u64(), e["span"]["col"].as_u64()));
    for e in edits.into_iter().rev() {
        let span = &e["span"];
        if span["line"] != span["end_line"] {
            return None;
        }
        let line = lines.get_mut((span["line"].as_u64()? as usize).checked_sub(1)?)?;
        let from = (span["col"].as_u64()? as usize).checked_sub(1)?;
        let to = (span["end_col"].as_u64()? as usize).checked_sub(1)?;
        if line.get(from..to)? != e["search"].as_str()? {
            return None;
        }
        line.replace_range(from..to, e["text"].as_str()?);
    }
    Some(lines.join("\n"))
}

/// The diagnostics reported in item `name` of the module.
pub fn item_diagnostics(
    db: &dyn Db,
    file: SourceFile,
    name: &str,
) -> Result<Vec<Diagnostic>, String> {
    match yan_db::items(db, file).iter().find(|i| i.key(db) == name) {
        Some(&i) => Ok(yan_db::item_diagnostics(db, i).clone()),
        None => Ok(diagnostics(db, file)?
            .into_iter()
            .filter(|d| d.item == name)
            .collect()),
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Hover {
    pub span: Span,
    pub text: String,
}

/// What is at 1-based `line` and `col`: a local binding's type, or an item's signature.
pub fn hover(db: &dyn Db, file: SourceFile, line: u32, col: u32) -> Option<Hover> {
    let s = yan_db::surface(file.path(db))?;
    if let Ok(Loaded::Ready(m, _, _)) = yan_db::load(db, file) {
        let mut found = None;
        for_each_let(m, &mut |name, at, ty| {
            if contains(at, line, col) {
                found = Some(Hover {
                    span: at,
                    text: format!("{name}: {}", show_type(ty, s)),
                });
            }
        });
        if found.is_some() {
            return found;
        }
    }
    let src = file.text(db);
    match yan_db::front(db, file).as_ref().ok()? {
        Front::Hir(h, _) => h
            .items
            .iter()
            .find(|it| contains(it.name_span, line, col))
            .map(|it| Hover {
                span: it.name_span,
                text: match s {
                    Surface::Yag => header(&yan_yag::print(&single(it))),
                    _ => source_line(src, it.span.line).trim().to_owned(),
                },
            }),
        Front::Yir(m) => m.items.iter().find_map(|it| match &it.kind {
            y::ItemKind::Fn(f) | y::ItemKind::Gen(f) if contains(f.name_span, line, col) => {
                Some(Hover {
                    span: f.name_span,
                    text: source_line(src, it.span.line)
                        .trim_end()
                        .trim_end_matches('{')
                        .trim()
                        .to_owned(),
                })
            }
            _ => None,
        }),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InlayKind {
    /// The inferred type of a local binding written without one.
    Type,
    /// A Yan function's inferred `uses` (7, 15.6).
    Uses,
    /// A Yan function's inferred `fail` (15.6).
    Fail,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Inlay {
    pub line: u32,
    pub col: u32,
    pub kind: InlayKind,
    pub label: String,
}

/// Inlay hints for what the source leaves to inference, in source order.
pub fn inlays(db: &dyn Db, file: SourceFile) -> Vec<Inlay> {
    let Some(s) = yan_db::surface(file.path(db)) else {
        return Vec::new();
    };
    let Ok(Loaded::Ready(m, _, _)) = yan_db::load(db, file) else {
        return Vec::new();
    };
    let src = file.text(db);
    let mut out = Vec::new();
    if s != Surface::Yir {
        for_each_let(m, &mut |_, at, ty| {
            let rest = source_line(src, at.end_line)
                .get(at.end_col.saturating_sub(1) as usize..)
                .unwrap_or("");
            if at.line == at.end_line && at.line > 0 && !rest.trim_start().starts_with(':') {
                out.push(Inlay {
                    line: at.end_line,
                    col: at.end_col,
                    kind: InlayKind::Type,
                    label: format!(": {}", show_type(ty, s)),
                });
            }
        });
    }
    if s == Surface::Yn {
        yn_signatures(db, file, m, &mut out);
    }
    out.sort_by_key(|i| (i.line, i.col));
    out.dedup();
    out
}

/// Inferred `uses` and `fail` of Yan functions that do not write them, after the signature.
fn yn_signatures(db: &dyn Db, file: SourceFile, m: &y::Module, out: &mut Vec<Inlay>) {
    let s = Surface::Yn;
    for it in &yan_db::yn_syntax(db, file).0.items {
        let yan_yn::ast::ItemKind::Fn(f) = &it.kind else {
            continue;
        };
        let sig = &f.sig;
        let Some(def) = m.items.iter().find_map(|i| match &i.kind {
            y::ItemKind::Fn(d) if d.path.last() == Some(&sig.name) => Some(d),
            _ => None,
        }) else {
            continue;
        };
        let (line, col) = (sig.span.end_line, sig.span.end_col);
        let caps: Vec<&str> = def
            .params
            .iter()
            .filter_map(|p| match p {
                y::Param::Cap { name, .. } => Some(name.as_str()),
                y::Param::Value { .. } => None,
            })
            .collect();
        if sig.uses.is_none() && !caps.is_empty() {
            out.push(Inlay {
                line,
                col,
                kind: InlayKind::Uses,
                label: format!(" uses {}", caps.join(", ")),
            });
        }
        if let (Some(t), None) = (&def.fail, &sig.fail) {
            out.push(Inlay {
                line,
                col,
                kind: InlayKind::Fail,
                label: format!(" fail {}", show_type(t, s)),
            });
        }
    }
}

/// Why a view was not printed.
#[derive(Debug)]
pub enum ViewError {
    /// The module has errors.
    Diagnostics(Vec<Diagnostic>),
    /// The module is outside the target surface's subset.
    Outside(String),
    /// A failure of the tool: no printer, no front end, an unsupported construct.
    Tool(String),
}

/// The module printed in another surface (16.4): any input as Yan Core, Yan as Yan Agent.
/// How [`view`] uses the spelling marks of elaboration (RFC 0034).
#[derive(Clone, Copy, Debug, Default)]
pub struct Marks {
    /// A mark that raise did not use is an error (D6).
    pub strict: bool,
    /// Raise with an empty table: the bare check of D7.
    pub bare: bool,
}

pub fn view(
    db: &dyn Db,
    file: SourceFile,
    as_: Surface,
    marks: Marks,
) -> Result<String, ViewError> {
    match (as_, yan_db::surface(file.path(db))) {
        (Surface::Yir, _) => match yan_db::load(db, file).as_ref().map_err(|m| tool(m))? {
            Loaded::Ready(m, _, _) => Ok(yan_core::print(m)),
            Loaded::Stopped(d) => Err(ViewError::Diagnostics(d.clone())),
        },
        // Yan's HIR printed by the Yan Agent printer: the Yan Agent projection of law 3 (16.5).
        (Surface::Yag, Some(Surface::Yn)) => {
            match yan_db::front(db, file).as_ref().map_err(|m| tool(m))? {
                Front::Hir(h, _) => yan_yag::resugar(h.clone())
                    .map(|x| yan_yag::print(&x))
                    .map_err(|what| ViewError::Outside(what.to_owned())),
                Front::Stopped(d) => Err(ViewError::Diagnostics(d.clone())),
                Front::Yir(_) => unreachable!("a .yn file has a Yan front end"),
            }
        }
        // print(resugar(raise(elaborate(X)))) (16.4, R559).
        (Surface::Yag, _) => match yan_db::load(db, file).as_ref().map_err(|m| tool(m))? {
            Loaded::Ready(m, trivia, _) => {
                let trivia = &if marks.bare {
                    Default::default()
                } else {
                    trivia.clone()
                };
                let raised = yan_hir::raise::raise(m, trivia, &yan_elab::StdInfo).map_err(
                    |yan_hir::raise::Unraised(what)| unsupported(&format!("raising {what}")),
                )?;
                let unused = yan_hir::raise::unconsumed(trivia, &raised);
                if marks.strict && !unused.is_empty() {
                    return Err(ViewError::Diagnostics(unused_spellings(
                        file.path(db),
                        m,
                        &unused,
                    )));
                }
                yan_yag::resugar(raised.module)
                    .map(|x| yan_yag::print(&x))
                    .map_err(|what| ViewError::Outside(what.to_owned()))
            }
            Loaded::Stopped(d) => Err(ViewError::Diagnostics(d.clone())),
        },
        _ => Err(ViewError::Tool(format!(
            "no printer for `{}` yet",
            surface_name(as_)
        ))),
    }
}

/// E279 for each spelling that `raise` did not use (16.5, RFC 0034 D6), at its item.
fn unused_spellings(
    path: &Path,
    m: &y::Module,
    unused: &[yan_hir::raise::Unused],
) -> Vec<Diagnostic> {
    let spans: HashMap<String, Span> = m
        .items
        .iter()
        .zip(yan_core::hash::hash_module(m))
        .map(|(it, h)| (h.identity.to_string(), it.span))
        .collect();
    let file = path.display().to_string();
    unused
        .iter()
        .map(|u| {
            let at = spans.get(&u.item).copied().unwrap_or_default();
            Diagnostic::error(
                "E279",
                &file,
                at,
                "",
                format!("spelling not used by raise: {}", u.what),
            )
            .with_item(&u.item)
        })
        .collect()
}

/// `src` in canonical form (18.4): print(parse(src)), for the surfaces with a printer.
pub fn canonical(path: &Path, src: &str) -> Result<String, ViewError> {
    let name = path.display().to_string();
    let (text, diags) = match yan_db::surface(path) {
        Some(Surface::Yag) => {
            let (m, d) = yan_yag::parse(&name, src, false);
            let text = m
                .filter(|_| !yan_db::has_error(&d))
                .map(|m| yan_yag::print(&m));
            (text, d)
        }
        Some(Surface::Yn) => {
            let (m, d) = yan_yn::parse(&name, src);
            ((!yan_db::has_error(&d)).then(|| yan_yn::print(&m)), d)
        }
        _ => return Err(ViewError::Tool(format!("{name}: no printer for this file"))),
    };
    text.ok_or(ViewError::Diagnostics(diags))
}

pub fn surface_name(s: Surface) -> &'static str {
    match s {
        Surface::Yir => "yir",
        Surface::Yag => "yag",
        Surface::Yn => "yn",
    }
}

pub fn surface_title(s: Surface) -> &'static str {
    match s {
        Surface::Yir => "Yan",
        Surface::Yag => "Yan Agent",
        Surface::Yn => "Yan",
    }
}

fn tool(msg: &str) -> ViewError {
    ViewError::Tool(msg.to_owned())
}

fn unsupported(what: &str) -> ViewError {
    ViewError::Tool(format!("not supported yet: {what}"))
}

/// A Yan Core type in a surface's spelling (16.3).
pub fn show_type(t: &y::Type, s: Surface) -> String {
    if s == Surface::Yir {
        return yan_core::type_text(t);
    }
    match t {
        y::Type::Named(n) => {
            let head = n.path.join(".");
            let args: Vec<String> = n
                .args
                .iter()
                .map(|a| match a {
                    y::TArg::Type(t) => show_type(t, s),
                    y::TArg::Nat(k) => k.clone(),
                })
                .collect();
            match (head.as_str(), s, args.as_slice()) {
                ("Unit", Surface::Yag, []) => "None".to_owned(),
                ("Option", Surface::Yag, [a]) => format!("{a} | None"),
                _ => {
                    let head = yan_names::type_name(&head, Surface::Yir, s).unwrap_or(&head);
                    if args.is_empty() {
                        head.to_owned()
                    } else {
                        format!("{head}[{}]", args.join(", "))
                    }
                }
            }
        }
        y::Type::Tuple(ts) => {
            let ts: Vec<String> = ts.iter().map(|t| show_type(t, s)).collect();
            match s {
                Surface::Yag => format!("tuple[{}]", ts.join(", ")),
                _ => format!("({})", ts.join(", ")),
            }
        }
        _ => yan_core::type_text(t),
    }
}

/// Every local binding the source names: its name, the span of its pattern and its type.
fn for_each_let(m: &y::Module, f: &mut dyn FnMut(&str, Span, &y::Type)) {
    for it in &m.items {
        match &it.kind {
            y::ItemKind::Fn(d) | y::ItemKind::Gen(d) => {
                if let Some(b) = &d.body {
                    block(b, f);
                }
            }
            y::ItemKind::Impl(i) => {
                for d in &i.fns {
                    if let Some(b) = &d.body {
                        block(b, f);
                    }
                }
            }
            _ => {}
        }
    }
}

fn block(b: &y::Block, f: &mut dyn FnMut(&str, Span, &y::Type)) {
    for s in &b.stmts {
        match &s.kind {
            y::StmtKind::Let {
                pat: y::Pat::Bind(name),
                pat_span,
                ty,
                ..
            } if !name.starts_with('_') => f(name, *pat_span, ty),
            y::StmtKind::If { then, els, .. } | y::StmtKind::ComptimeIf { then, els, .. } => {
                block(then, f);
                if let Some(e) = els {
                    block(e, f);
                }
            }
            y::StmtKind::Match { arms, .. } => arms.iter().for_each(|(_, b)| block(b, f)),
            y::StmtKind::Loop { body, .. }
            | y::StmtKind::Jp { body, .. }
            | y::StmtKind::With { body, .. }
            | y::StmtKind::ComptimeFor { body, .. }
            | y::StmtKind::Unsafe(body) => block(body, f),
            y::StmtKind::Par { blocks, .. } => blocks.iter().for_each(|b| block(b, f)),
            _ => {}
        }
    }
}

fn contains(s: Span, line: u32, col: u32) -> bool {
    (s.line, s.col) <= (line, col) && (line, col) < (s.end_line, s.end_col)
}

fn source_line(src: &str, line: u32) -> &str {
    src.lines()
        .nth(line.saturating_sub(1) as usize)
        .unwrap_or("")
}

fn single(it: &yan_hir::Item) -> yan_hir::Module {
    yan_hir::Module {
        header: Vec::new(),
        attrs: Vec::new(),
        doc: None,
        items: vec![it.clone()],
    }
}

/// Signatures, docstrings, contracts and examples: each function's canonical text up to its
/// first body statement, and every other item whole. A budget in tokens (four bytes each)
/// keeps items in module order while they fit.
pub fn sig(m: &yan_hir::Module, budget: Option<usize>) -> String {
    let mut out = Vec::new();
    let mut used = 0;
    for it in &m.items {
        let text = yan_yag::print(&single(it));
        let text = match &it.kind {
            yan_hir::ItemKind::Func(_) | yan_hir::ItemKind::Lemma(_) => header(&text),
            _ => text.trim_end().to_owned(),
        };
        used += text.len() / 4 + 1;
        if budget.is_some_and(|b| used > b) {
            break;
        }
        out.push(text);
    }
    out.join("\n\n")
}

/// The decorator, `def` and continuation lines, then the docstring and contract lines.
fn header(item: &str) -> String {
    let mut lines = item.lines();
    let mut out: Vec<&str> = Vec::new();
    for l in lines.by_ref() {
        out.push(l);
        if !l.starts_with('@') && l.trim_end().ends_with(':') {
            break;
        }
    }
    let mut in_doc = false;
    for l in lines {
        let t = l.trim_start();
        let contract = ["pre ", "post ", "ex ", "decreases "]
            .iter()
            .any(|k| t.starts_with(k));
        if in_doc || t.starts_with("\"\"\"") {
            let opens = t.starts_with("\"\"\"") && !in_doc;
            out.push(l);
            let closes = t.ends_with("\"\"\"") && (!opens || t.len() >= 6);
            in_doc = !closes;
        } else if contract {
            out.push(l);
        } else {
            break;
        }
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const SRC: &str = "def f(x: int) -> int:\n    y = x + 1\n    z: int = y\n    return z\n";

    #[test]
    fn unused_spelling_is_e279_at_its_item() {
        let m = y::Module {
            header: Vec::new(),
            path: y::Path::default(),
            items: Vec::new(),
        };
        let unused = [yan_hir::raise::Unused {
            item: "h".to_owned(),
            what: "Out at h [0]".to_owned(),
        }];
        let d = unused_spellings(Path::new("t.yag"), &m, &unused);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].code, "E279");
        assert_eq!(d[0].file, "t.yag");
        assert_eq!(d[0].message, "spelling not used by raise: Out at h [0]");
    }

    #[test]
    fn only_verified_fixes_are_reported() {
        let src = "def f(name: str, count: int) -> int:\n    _ = name\n    return cont + 1\n\n\
                   def g(nane: str, pane: int) -> int:\n    _ = pane\n    return nam + 1\n";
        let mut db = Database::new(false, false);
        let file = db.file(Path::new("m.yag"), src);
        let fixes: Vec<_> = diagnostics(&db, file)
            .unwrap()
            .iter()
            .map(|d| {
                d.fixes
                    .iter()
                    .map(|f| (f["edits"][0]["text"].clone(), f["verified"].clone()))
                    .collect::<Vec<_>>()
            })
            .collect();
        // `nane + 1` would be a new E201, so g's near name is dropped.
        assert_eq!(fixes, [vec![("count".into(), true.into())], vec![]]);
    }

    #[test]
    fn header_keeps_doc_and_contracts() {
        let f = "@inline\ndef f(x: int) -> int:\n    \"\"\"Doc\n    more.\"\"\"\n    pre x > 0\n    ex f(1) == 2\n    return x + 1\n";
        assert_eq!(
            header(f),
            "@inline\ndef f(x: int) -> int:\n    \"\"\"Doc\n    more.\"\"\"\n    pre x > 0\n    ex f(1) == 2"
        );
    }

    #[test]
    fn inlays_show_only_unwritten_types() {
        let mut db = Database::new(false, false);
        let file = db.file(Path::new("m.yag"), SRC);
        let labels: Vec<_> = inlays(&db, file)
            .into_iter()
            .map(|i| (i.line, i.col, i.label))
            .collect();
        assert_eq!(labels, [(2, 6, ": int".to_owned())]);
    }

    #[test]
    fn yn_inlays_show_inferred_uses() {
        let src = "sq(x: int) -> int = x * x\n\nmain() =\n  let a = sq(2)\n  io.print(\"\\(a)\")\n";
        let mut db = Database::new(false, false);
        let file = db.file(Path::new("m.yn"), src);
        let labels: Vec<_> = inlays(&db, file)
            .into_iter()
            .map(|i| (i.line, i.col, i.label))
            .collect();
        assert_eq!(
            labels,
            [(3, 7, " uses io".to_owned()), (4, 8, ": int".to_owned())]
        );
    }

    #[test]
    fn hover_shows_bindings_and_signatures() {
        let mut db = Database::new(false, false);
        let file = db.file(Path::new("m.yag"), SRC);
        assert_eq!(hover(&db, file, 2, 5).unwrap().text, "y: int");
        assert_eq!(
            hover(&db, file, 1, 5).unwrap().text,
            "def f(x: int) -> int:"
        );
        assert!(hover(&db, file, 4, 1).is_none());
    }
}
