//! The query database (22.2): salsa queries over source files, keyed by file and by item hash.
//! Each driver (`yong check`, `run`, `hash`, `patch`, `mcp`) asks it for a module's front end,
//! its elaborated Yong, its diagnostics and its items. Std's Xin sources are parsed here too,
//! since the elaborator depends on no surface crate (R560).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Once;

use yong_core::ast as y;
use yong_core::hash::Hashed;
use yong_diag::{Diagnostic, Severity};

/// Each std module: its path below `std.`, its file name, its source and its HIR.
pub fn std_modules() -> Vec<(&'static str, String, &'static str, yong_hir::Module)> {
    yong_std::SOURCES
        .iter()
        .zip(yong_std::TIERS)
        .map(|(&(module, src), tier)| {
            let file = format!("{}/{}.xin", tier.dir(), module.replace('.', "/"));
            let (m, diags) = yong_xin::parse(&file, src, true);
            assert!(diags.is_empty(), "std.{module}: {diags:?}");
            (module, file, src, m.expect("std parses"))
        })
        .collect()
}

/// A source file: its path and its text.
#[salsa::input(debug)]
pub struct SourceFile {
    #[returns(ref)]
    pub path: PathBuf,
    #[returns(ref)]
    pub text: String,
}

/// How every file is checked: `--strict` (14.4) and `--level release` (8.5).
#[salsa::input(singleton, debug)]
pub struct Settings {
    #[returns(copy)]
    pub strict: bool,
    #[returns(copy)]
    pub release: bool,
}

/// An item of a module, keyed by its file, its name and its salted hash (3.6): an edited item
/// is a new key, an unchanged one keeps its cached results.
#[salsa::interned(debug)]
pub struct Item<'db> {
    #[returns(copy)]
    pub file: SourceFile,
    #[returns(ref)]
    pub key: String,
    #[returns(ref)]
    pub hash: String,
}

pub use salsa::Database as Db;

#[salsa::db]
#[derive(Clone, Default)]
pub struct Database {
    storage: salsa::Storage<Self>,
    files: HashMap<PathBuf, SourceFile>,
}

#[salsa::db]
impl salsa::Database for Database {}

impl Database {
    pub fn new(strict: bool, release: bool) -> Self {
        static STD: Once = Once::new();
        STD.call_once(|| yong_elab::install_std(std_modules));
        let db = Self::default();
        Settings::new(&db, strict, release);
        db
    }

    /// The file at `path` with text `text`: the existing input, updated if the text changed.
    pub fn file(&mut self, path: &Path, text: &str) -> SourceFile {
        use salsa::Setter;
        if let Some(&f) = self.files.get(path) {
            if f.text(self) != text {
                f.set_text(self).to(text.to_owned());
            }
            return f;
        }
        let f = SourceFile::new(self, path.to_owned(), text.to_owned());
        self.files.insert(path.to_owned(), f);
        f
    }

    /// The file at `path`, read from disk.
    pub fn open(&mut self, path: &Path) -> Result<SourceFile, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(self.file(path, &text))
    }
}

/// A module's front end: parsed and, for Xin and Wen, lowered to the HIR.
#[derive(Clone, Debug, PartialEq)]
pub enum Front {
    Yong(y::Module),
    /// The HIR and the front end's warnings.
    Hir(yong_hir::Module, Vec<Diagnostic>),
    /// Front-end errors.
    Stopped(Vec<Diagnostic>),
    /// Wen the lowering does not handle yet; the front end's diagnostics.
    Unsupported(String, Vec<Diagnostic>),
}

/// A module ready to check or run: parsed and, for Xin and Wen, elaborated to Yong.
#[derive(Clone, Debug, PartialEq)]
pub enum Loaded {
    Ready(y::Module, Vec<Diagnostic>),
    /// Front-end diagnostics that stop before the Yong checker.
    Stopped(Vec<Diagnostic>),
    /// A construct the elaborator does not lower yet; the front end's diagnostics.
    Unsupported(String, Vec<Diagnostic>),
}

pub fn has_error(diags: &[Diagnostic]) -> bool {
    diags.iter().any(|d| d.severity == Severity::Error)
}

/// The surface a file is written in, by extension.
pub fn surface(path: &Path) -> Option<yong_names::Surface> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("yong") => Some(yong_names::Surface::Yong),
        Some("xin") => Some(yong_names::Surface::Xin),
        Some("wen") => Some(yong_names::Surface::Wen),
        _ => None,
    }
}

#[salsa::tracked(returns(ref))]
pub fn front(db: &dyn Db, file: SourceFile) -> Result<Front, String> {
    let path = file.path(db);
    let name = path.display().to_string();
    let src = file.text(db);
    Ok(match surface(path) {
        Some(yong_names::Surface::Yong) => match yong_core::parse(&name, src) {
            (Some(m), d) if d.is_empty() => Front::Yong(m),
            (_, d) => Front::Stopped(d),
        },
        Some(yong_names::Surface::Xin) => {
            let (m, diags) = yong_xin::parse(&name, src, Settings::get(db).strict(db));
            match m.filter(|_| !has_error(&diags)) {
                Some(m) => Front::Hir(m, diags),
                None => Front::Stopped(diags),
            }
        }
        Some(yong_names::Surface::Wen) => {
            let (m, diags) = wen_syntax(db, file).clone();
            if has_error(&diags) {
                return Ok(Front::Stopped(diags));
            }
            match yong_wen::lower(&name, src, &m) {
                Err(yong_wen::Unsupported(what)) => Front::Unsupported(what, diags),
                Ok((_, mut errs)) if has_error(&errs) => {
                    errs.extend(diags);
                    Front::Stopped(errs)
                }
                Ok((h, mut errs)) => {
                    errs.extend(diags);
                    Front::Hir(h, errs)
                }
            }
        }
        None => {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            return Err(format!("no front end for .{ext} files yet: {name}"));
        }
    })
}

/// A Wen file's syntax tree and parse diagnostics; tools read what was written from it, since
/// lowering fills in inferred `uses`.
#[salsa::tracked(returns(ref))]
pub fn wen_syntax(db: &dyn Db, file: SourceFile) -> (yong_wen::ast::Module, Vec<Diagnostic>) {
    yong_wen::parse(&file.path(db).display().to_string(), file.text(db))
}

#[salsa::tracked(returns(ref))]
pub fn load(db: &dyn Db, file: SourceFile) -> Result<Loaded, String> {
    load_ordered(db, file, false)
}

/// [`load`] with the module's items reversed before elaborating, for checking law 4 (16.5).
/// Not cached.
pub fn load_reversed(db: &dyn Db, file: SourceFile) -> Result<Loaded, String> {
    load_ordered(db, file, true)
}

fn load_ordered(db: &dyn Db, file: SourceFile, reverse: bool) -> Result<Loaded, String> {
    Ok(match front(db, file).clone()? {
        Front::Yong(mut m) => {
            if reverse {
                m.items.reverse();
            }
            Loaded::Ready(m, Vec::new())
        }
        Front::Hir(mut h, diags) => {
            if reverse {
                h.items.reverse();
            }
            elaborate(db, file, &h, diags)
        }
        Front::Stopped(d) => Loaded::Stopped(d),
        Front::Unsupported(what, d) => Loaded::Unsupported(what, d),
    })
}

/// The shared IR to Yong, for both surfaces; `diags` are the front end's warnings.
fn elaborate(
    db: &dyn Db,
    file: SourceFile,
    m: &yong_hir::Module,
    diags: Vec<Diagnostic>,
) -> Loaded {
    let path = file.path(db);
    let settings = Settings::get(db);
    let wen = surface(path) == Some(yong_names::Surface::Wen);
    let opts = yong_elab::Options {
        // Wen has no lenient reader (14.4).
        strict: wen || settings.strict(db),
        release: settings.release(db),
        wen,
        ffi: is_ffi(path),
    };
    let name = path.display().to_string();
    match yong_elab::elaborate(&name, file.text(db), m, opts) {
        Err(yong_elab::Unsupported(what)) => Loaded::Unsupported(what, diags),
        Ok((y, mut errs)) => {
            let stop = has_error(&errs);
            errs.extend(diags);
            if stop {
                Loaded::Stopped(errs)
            } else {
                Loaded::Ready(y, errs)
            }
        }
    }
}

/// 12: a module under a directory named `ffi` (conformance assumption A9).
fn is_ffi(path: &Path) -> bool {
    path.parent()
        .is_some_and(|d| d.components().any(|c| c.as_os_str() == "ffi"))
}

/// Every diagnostic of the module: the front end's, the elaborator's and the Yong checker's.
#[salsa::tracked(returns(ref))]
pub fn diagnostics(db: &dyn Db, file: SourceFile) -> Result<Vec<Diagnostic>, String> {
    Ok(match load(db, file).clone()? {
        Loaded::Ready(m, mut d) => {
            let path = file.path(db).display().to_string();
            d.extend(yong_check::check(&path, file.text(db), &m));
            d
        }
        Loaded::Stopped(d) => d,
        // Front-end errors found before the unsupported construct are still real.
        Loaded::Unsupported(_, d) if has_error(&d) => d,
        Loaded::Unsupported(what, _) => return Err(format!("not supported yet: {what}")),
    })
}

/// The item hashes (3.6) of a module that elaborates.
#[salsa::tracked(returns(ref))]
pub fn hashes(db: &dyn Db, file: SourceFile) -> Option<Vec<Hashed>> {
    match load(db, file) {
        Ok(Loaded::Ready(m, _)) => Some(yong_core::hash::hash_module(m)),
        _ => None,
    }
}

/// The module's items, in item order. Each is keyed by its salted hash (3.6) when the module
/// elaborates, and otherwise, while it has errors, by the BLAKE3 hash of its written text.
#[salsa::tracked(returns(ref))]
pub fn items<'db>(db: &'db dyn Db, file: SourceFile) -> Vec<Item<'db>> {
    if let Some(hs) = hashes(db, file) {
        return hs
            .iter()
            .map(|h| Item::new(db, file, h.key.clone(), h.salted.to_string()))
            .collect();
    }
    let Ok(Front::Hir(m, _)) = front(db, file) else {
        return Vec::new();
    };
    let lines: Vec<&str> = file.text(db).lines().collect();
    m.items
        .iter()
        .map(|it| {
            let from = (it.span.line as usize).saturating_sub(1).min(lines.len());
            let to = (it.span.end_line as usize).clamp(from, lines.len());
            let text = lines[from..to].join("\n");
            let hash = format!("b3:{}", blake3::hash(text.as_bytes()).to_hex());
            Item::new(db, file, it.kind.name().to_owned(), hash)
        })
        .collect()
}

/// The diagnostics reported in one item.
#[salsa::tracked(returns(ref))]
pub fn item_diagnostics<'db>(db: &'db dyn Db, item: Item<'db>) -> Vec<Diagnostic> {
    diagnostics(db, item.file(db))
        .iter()
        .flatten()
        .filter(|d| d.item == *item.key(db))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str =
        "def f(x: int) -> int:\n    return x + 1\n\n\ndef g() -> int:\n    return f(1)\n";

    #[test]
    fn an_unchanged_item_keeps_its_key() {
        let mut db = Database::new(false, false);
        let file = db.file(Path::new("m.xin"), SRC);
        let before: Vec<_> = items(&db, file)
            .iter()
            .map(|i| i.hash(&db).clone())
            .collect();
        assert_eq!(before.len(), 2);
        let edited = SRC.replace("f(1)", "f(2)");
        let file = db.file(Path::new("m.xin"), &edited);
        let after: Vec<_> = items(&db, file)
            .iter()
            .map(|i| i.hash(&db).clone())
            .collect();
        assert_eq!(before[0], after[0]);
        assert_ne!(before[1], after[1]);
    }

    #[test]
    fn diagnostics_follow_the_text() {
        let mut db = Database::new(false, false);
        let file = db.file(Path::new("m.xin"), SRC);
        assert_eq!(diagnostics(&db, file).as_ref().unwrap().len(), 0);
        let file = db.file(Path::new("m.xin"), &SRC.replace("f(1)", "f(\"a\")"));
        let d = diagnostics(&db, file).as_ref().unwrap();
        assert!(d.iter().any(|d| d.item == "g"), "{d:?}");
        let g = items(&db, file)[1];
        assert_eq!(item_diagnostics(&db, g).len(), d.len());
    }
}
