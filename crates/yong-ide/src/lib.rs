//! The analysis API (22.2, R560) over the query database: diagnostics, hover, inlay hints and
//! view-as. `yong-cli` and `yong mcp` use it, and the language server will.

use std::path::Path;

use serde::Serialize;
use yong_core::ast as y;
use yong_db::{Db, Front, Loaded, SourceFile};
use yong_diag::{Diagnostic, Span};
use yong_names::Surface;

pub use yong_db::Database;

/// Every diagnostic of the module, or a tool failure (an unsupported construct, no front end).
pub fn diagnostics(db: &dyn Db, file: SourceFile) -> Result<Vec<Diagnostic>, String> {
    yong_db::diagnostics(db, file).clone()
}

/// The diagnostics reported in item `name` of the module.
pub fn item_diagnostics(
    db: &dyn Db,
    file: SourceFile,
    name: &str,
) -> Result<Vec<Diagnostic>, String> {
    match yong_db::items(db, file).iter().find(|i| i.key(db) == name) {
        Some(&i) => Ok(yong_db::item_diagnostics(db, i).clone()),
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
    let s = yong_db::surface(file.path(db))?;
    if let Ok(Loaded::Ready(m, _)) = yong_db::load(db, file) {
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
    match yong_db::front(db, file).as_ref().ok()? {
        Front::Hir(h, _) => h
            .items
            .iter()
            .find(|it| contains(it.name_span, line, col))
            .map(|it| Hover {
                span: it.name_span,
                text: match s {
                    Surface::Xin => header(&yong_xin::print(&single(it))),
                    _ => source_line(src, it.span.line).trim().to_owned(),
                },
            }),
        Front::Yong(m) => m.items.iter().find_map(|it| match &it.kind {
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
    /// A Wen function's inferred `uses` (7, 15.6).
    Uses,
    /// A Wen function's inferred `fail` (15.6).
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
    let Some(s) = yong_db::surface(file.path(db)) else {
        return Vec::new();
    };
    let Ok(Loaded::Ready(m, _)) = yong_db::load(db, file) else {
        return Vec::new();
    };
    let src = file.text(db);
    let mut out = Vec::new();
    if s != Surface::Yong {
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
    if s == Surface::Wen {
        wen_signatures(db, file, m, &mut out);
    }
    out.sort_by_key(|i| (i.line, i.col));
    out.dedup();
    out
}

/// Inferred `uses` and `fail` of Wen functions that do not write them, after the signature.
fn wen_signatures(db: &dyn Db, file: SourceFile, m: &y::Module, out: &mut Vec<Inlay>) {
    let s = Surface::Wen;
    for it in &yong_db::wen_syntax(db, file).0.items {
        let yong_wen::ast::ItemKind::Fn(f) = &it.kind else {
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

/// The module printed in another surface (16.4): any input as Yong, Wen as Xin.
pub fn view(db: &dyn Db, file: SourceFile, as_: Surface) -> Result<String, ViewError> {
    match (as_, yong_db::surface(file.path(db))) {
        (Surface::Yong, _) => match yong_db::load(db, file).as_ref().map_err(|m| tool(m))? {
            Loaded::Ready(m, _) => Ok(yong_core::print(m)),
            Loaded::Stopped(d) => Err(ViewError::Diagnostics(d.clone())),
        },
        // Wen's HIR printed by the Xin printer: the Xin projection of law 3 (16.5).
        (Surface::Xin, Some(Surface::Wen)) => {
            match yong_db::front(db, file).as_ref().map_err(|m| tool(m))? {
                Front::Hir(h, _) => yong_xin::resugar(h.clone())
                    .map(|x| yong_xin::print(&x))
                    .map_err(|what| ViewError::Outside(what.to_owned())),
                Front::Stopped(d) => Err(ViewError::Diagnostics(d.clone())),
                Front::Yong(_) => unreachable!("a .wen file has a Wen front end"),
            }
        }
        // print(resugar(raise(elaborate(X)))) (16.4, R559).
        (Surface::Xin, _) => match yong_db::load(db, file).as_ref().map_err(|m| tool(m))? {
            Loaded::Ready(m, _) => {
                let h = yong_hir::raise::raise(m, &yong_elab::StdInfo).map_err(
                    |yong_hir::raise::Unraised(what)| unsupported(&format!("raising {what}")),
                )?;
                yong_xin::resugar(h)
                    .map(|x| yong_xin::print(&x))
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

/// `src` in canonical form (18.4): print(parse(src)), for the surfaces with a printer.
pub fn canonical(path: &Path, src: &str) -> Result<String, ViewError> {
    let name = path.display().to_string();
    let (text, diags) = match yong_db::surface(path) {
        Some(Surface::Xin) => {
            let (m, d) = yong_xin::parse(&name, src, false);
            let text = m
                .filter(|_| !yong_db::has_error(&d))
                .map(|m| yong_xin::print(&m));
            (text, d)
        }
        Some(Surface::Wen) => {
            let (m, d) = yong_wen::parse(&name, src);
            ((!yong_db::has_error(&d)).then(|| yong_wen::print(&m)), d)
        }
        _ => return Err(ViewError::Tool(format!("{name}: no printer for this file"))),
    };
    text.ok_or(ViewError::Diagnostics(diags))
}

pub fn surface_name(s: Surface) -> &'static str {
    match s {
        Surface::Yong => "yong",
        Surface::Xin => "xin",
        Surface::Wen => "wen",
    }
}

pub fn surface_title(s: Surface) -> &'static str {
    match s {
        Surface::Yong => "Yong",
        Surface::Xin => "Xin",
        Surface::Wen => "Wen",
    }
}

fn tool(msg: &str) -> ViewError {
    ViewError::Tool(msg.to_owned())
}

fn unsupported(what: &str) -> ViewError {
    ViewError::Tool(format!("not supported yet: {what}"))
}

/// A Yong type in a surface's spelling (16.3).
pub fn show_type(t: &y::Type, s: Surface) -> String {
    if s == Surface::Yong {
        return yong_core::type_text(t);
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
                ("Unit", Surface::Xin, []) => "None".to_owned(),
                ("Option", Surface::Xin, [a]) => format!("{a} | None"),
                _ => {
                    let head = yong_names::type_name(&head, Surface::Yong, s).unwrap_or(&head);
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
                Surface::Xin => format!("tuple[{}]", ts.join(", ")),
                _ => format!("({})", ts.join(", ")),
            }
        }
        _ => yong_core::type_text(t),
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

fn single(it: &yong_hir::Item) -> yong_hir::Module {
    yong_hir::Module {
        header: Vec::new(),
        attrs: Vec::new(),
        doc: None,
        items: vec![it.clone()],
    }
}

/// Signatures, docstrings, contracts and examples: each function's canonical text up to its
/// first body statement, and every other item whole. A budget in tokens (four bytes each)
/// keeps items in module order while they fit.
pub fn sig(m: &yong_hir::Module, budget: Option<usize>) -> String {
    let mut out = Vec::new();
    let mut used = 0;
    for it in &m.items {
        let text = yong_xin::print(&single(it));
        let text = match &it.kind {
            yong_hir::ItemKind::Func(_) | yong_hir::ItemKind::Lemma(_) => header(&text),
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
        let file = db.file(Path::new("m.xin"), SRC);
        let labels: Vec<_> = inlays(&db, file)
            .into_iter()
            .map(|i| (i.line, i.col, i.label))
            .collect();
        assert_eq!(labels, [(2, 6, ": int".to_owned())]);
    }

    #[test]
    fn wen_inlays_show_inferred_uses() {
        let src = "sq(x: int) -> int = x * x\n\nmain() =\n  let a = sq(2)\n  io.print(\"\\(a)\")\n";
        let mut db = Database::new(false, false);
        let file = db.file(Path::new("m.wen"), src);
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
        let file = db.file(Path::new("m.xin"), SRC);
        assert_eq!(hover(&db, file, 2, 5).unwrap().text, "y: int");
        assert_eq!(
            hover(&db, file, 1, 5).unwrap().text,
            "def f(x: int) -> int:"
        );
        assert!(hover(&db, file, 4, 1).is_none());
    }
}
