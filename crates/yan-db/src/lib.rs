//! The query database (22.2): salsa queries over source files, keyed by file and by item hash.
//! Each driver (`yan check`, `run`, `hash`, `patch`, `mcp`) asks it for a module's front end,
//! its elaborated Yan Core, its diagnostics and its items. Std's Yan Agent sources are parsed here too,
//! since the elaborator depends on no surface crate (R560).

pub mod closure;
mod docs;
mod foreign;
mod graph;
mod layout;
#[cfg(test)]
use foreign::is_ffi_module;
pub use foreign::{ForeignLib, ForeignSet, foreign_set};
pub use graph::{
    GraphBuilder, Package, PackageGraph, Role, consented, dependency_roots, graph_diagnostics,
    install_graph_builder, owner,
};
pub use layout::{
    Issue as LayoutIssue, Located, Module as PackageModule, Root, dependency_issues,
    diagnostics as layout_diagnostics, locate, path_issue, scan as scan_layout,
};

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Once;

use yan_core::ast as y;
use yan_core::hash::Hashed;
use yan_diag::{Diagnostic, Severity, Span};
use yan_surface::TriviaTable;

/// Each std module: its path below `std.`, its file name, its source and its HIR.
pub fn std_modules() -> Vec<(&'static str, String, &'static str, yan_hir::Module)> {
    yan_std::SOURCES
        .iter()
        .zip(yan_std::TIERS)
        .map(|(&(module, src), tier)| {
            let file = format!("{}/{}.yag", tier.dir(), module.replace('.', "/"));
            let (m, diags) = yan_yag::parse(&file, src, true);
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

/// Every source file the database knows, by path: a module reads its siblings through it.
#[salsa::input(singleton, debug)]
pub struct FileIndex {
    #[returns(ref)]
    pub by_path: HashMap<PathBuf, SourceFile>,
}

/// How every file is checked: `--strict` (14.4) and `--level release` (8.5).
#[salsa::input(singleton, debug)]
pub struct Settings {
    #[returns(copy)]
    pub strict: bool,
    #[returns(copy)]
    pub release: bool,
    /// Stage 0 does not run (R676).
    #[returns(copy)]
    pub untrusted: bool,
    /// Public functions carry tool schemas for `yan mcp package` (RFC 0062).
    #[returns(copy)]
    pub tools: bool,
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
    /// Directories the graph builder has already been asked about.
    probed: HashSet<PathBuf>,
}

#[salsa::db]
impl salsa::Database for Database {}

impl Database {
    pub fn new(strict: bool, release: bool) -> Self {
        static STD: Once = Once::new();
        STD.call_once(|| yan_elab::install_std(std_modules));
        let db = Self::default();
        Settings::new(&db, strict, release, false, false);
        PackageGraph::new(&db, Vec::new());
        FileIndex::new(&db, HashMap::new());
        db
    }

    /// Stop stage 0 from running, for a repository the user has not trusted (R676).
    pub fn untrusted(mut self) -> Self {
        use salsa::Setter;
        Settings::get(&self).set_untrusted(&mut self).to(true);
        self
    }

    /// Elaborate public functions as tools (RFC 0062).
    pub fn tools(mut self) -> Self {
        use salsa::Setter;
        Settings::get(&self).set_tools(&mut self).to(true);
        self
    }

    /// E023 and E314 of every workspace package seen so far, on their `package.yag` (RFC 0079 D3).
    pub fn graph_diagnostics(&self) -> Vec<Diagnostic> {
        graph::graph_diagnostics(self, PackageGraph::get(self)).clone()
    }

    /// The file at `path` with text `text`: the existing input, updated if the text changed.
    pub fn file(&mut self, path: &Path, text: &str) -> SourceFile {
        use salsa::Setter;
        self.cover(path);
        if let Some(&f) = self.files.get(path) {
            if f.text(self) != text {
                f.set_text(self).to(text.to_owned());
            }
            return f;
        }
        let f = SourceFile::new(self, path.to_owned(), text.to_owned());
        self.files.insert(path.to_owned(), f);
        let index = FileIndex::get(self);
        let all = self.files.clone();
        index.set_by_path(self).to(all);
        f
    }

    /// Add the packages around `path` to the graph, once per directory.
    fn cover(&mut self, path: &Path) {
        use salsa::Setter;
        let dir = path.parent().unwrap_or(Path::new("")).to_owned();
        if !self.probed.insert(dir) {
            return;
        }
        let found = graph::build(path);
        let graph = PackageGraph::get(self);
        let mut packages = graph.packages(self).clone();
        let before = packages.len();
        for p in found {
            if !packages.iter().any(|q| q.root == p.root) {
                packages.push(p);
            }
        }
        if packages.len() != before {
            let fresh: Vec<PathBuf> = packages[before..]
                .iter()
                .flat_map(|p| p.modules.iter())
                .filter(|m| matches!(m.root, layout::Root::Src | layout::Root::Tests))
                .map(|m| m.file.clone())
                .collect();
            graph.set_packages(self).to(packages);
            for f in fresh {
                let _ = self.open(&f);
            }
        }
    }

    /// The file at `path`, read from disk.
    pub fn open(&mut self, path: &Path) -> Result<SourceFile, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(self.file(path, &text))
    }
}

/// A module's front end: parsed and, for Yan Agent and Yan, lowered to the HIR.
#[derive(Clone, Debug, PartialEq)]
pub enum Front {
    Yir(y::Module),
    /// The HIR and the front end's warnings.
    Hir(yan_hir::Module, Vec<Diagnostic>),
    /// Front-end errors.
    Stopped(Vec<Diagnostic>),
}

/// A module ready to check or run: parsed and, for Yan Agent and Yan, elaborated to Yan Core.
#[derive(Clone, Debug, PartialEq)]
pub enum Loaded {
    Ready(y::Module, TriviaTable, Vec<Diagnostic>),
    /// Front-end diagnostics that stop before the Yan Core checker, E017 among them (R649).
    Stopped(Vec<Diagnostic>),
}

pub fn has_error(diags: &[Diagnostic]) -> bool {
    diags.iter().any(|d| d.severity == Severity::Error)
}

/// The surface a file is written in, by extension.
pub fn surface(path: &Path) -> Option<yan_names::Surface> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("yir") => Some(yan_names::Surface::Yir),
        Some("yag") => Some(yan_names::Surface::Yag),
        Some("yn") => Some(yan_names::Surface::Yn),
        _ => None,
    }
}

#[salsa::tracked(returns(ref))]
pub fn front(db: &dyn Db, file: SourceFile) -> Result<Front, String> {
    let path = file.path(db);
    let name = path.display().to_string();
    let src = file.text(db);
    Ok(match surface(path) {
        Some(yan_names::Surface::Yir) => match yan_core::parse(&name, src) {
            (Some(m), d) if d.is_empty() => Front::Yir(m),
            (_, d) => Front::Stopped(d),
        },
        Some(yan_names::Surface::Yag) => {
            let (m, diags) = yan_yag::parse(&name, src, Settings::get(db).strict(db));
            match m.filter(|_| !has_error(&diags)) {
                Some(m) => Front::Hir(m, diags),
                None => Front::Stopped(diags),
            }
        }
        Some(yan_names::Surface::Yn) => {
            let (m, diags) = yn_syntax(db, file).clone();
            if has_error(&diags) {
                return Ok(Front::Stopped(diags));
            }
            let packages = PackageGraph::get(db).packages(db);
            let placed = layout::locate(packages, path).filter(|l| l.root.is_package_code());
            let modules: Vec<String> = owner(packages, path)
                .map(|p| {
                    visible_modules(packages, p, path)
                        .into_iter()
                        .map(|(n, _)| n)
                        .collect()
                })
                .unwrap_or_default();
            let ctx = placed.map(|l| (l.module[..l.module.len() - 1].to_vec(), modules));
            let pkg = ctx
                .as_ref()
                .map(|(dir, modules)| yan_yn::PackageCtx { dir, modules });
            match yan_yn::lower_in(&name, src, &m, pkg.as_ref()) {
                Err(u) => Front::Stopped(unsupported(&name, src, &u.what, u.at, None, diags)),
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

/// A Yan file's syntax tree and parse diagnostics; tools read what was written from it, since
/// lowering fills in inferred `uses`.
#[salsa::tracked(returns(ref))]
pub fn yn_syntax(db: &dyn Db, file: SourceFile) -> (yan_yn::ast::Module, Vec<Diagnostic>) {
    yan_yn::parse(&file.path(db).display().to_string(), file.text(db))
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
        Front::Yir(mut m) => {
            if reverse {
                m.items.reverse();
            }
            Loaded::Ready(m, TriviaTable::default(), Vec::new())
        }
        Front::Hir(mut h, diags) => {
            if reverse {
                h.items.reverse();
            }
            elaborate(db, file, &h, diags)
        }
        Front::Stopped(d) => Loaded::Stopped(d),
    })
}

/// The shared IR to Yan Core, for both surfaces; `diags` are the front end's warnings.
fn elaborate(db: &dyn Db, file: SourceFile, m: &yan_hir::Module, diags: Vec<Diagnostic>) -> Loaded {
    let path = file.path(db);
    let settings = Settings::get(db);
    let yn = surface(path) == Some(yan_names::Surface::Yn);
    let packages = PackageGraph::get(db).packages(db);
    let pkg = owner(packages, path);
    let opts = yan_elab::Options {
        // Yan has no lenient reader (14.4).
        strict: yn || settings.strict(db),
        release: settings.release(db),
        yn,
        ffi: pkg.map_or_else(
            || foreign::lone_ffi(path),
            |p| foreign::in_ffi_dir(&p.root, path),
        ),
        untrusted: settings.untrusted(db),
        tools: settings.tools(db),
    };
    let name = path.display().to_string();
    let modules = pkg
        .map(|p| visible_modules(packages, p, path))
        .unwrap_or_default();
    let deps = pkg
        .map(|p| dependency_scopes(packages, p, path))
        .unwrap_or_default();
    let index = FileIndex::get(db).by_path(db);
    let sibling = |dotted: &str| {
        let (_, f) = modules.iter().find(|(n, _)| n == dotted)?;
        let sf = *index.get(f)?;
        match front(db, sf) {
            Ok(Front::Hir(h, _)) => Some(h.clone()),
            _ => None,
        }
    };
    let own = layout::locate(packages, path)
        .filter(|l| l.root.is_package_code())
        .map(|l| l.module.join("."));
    let env = own.map(|this| yan_elab::PackageEnv {
        this,
        modules: modules.iter().map(|(n, _)| n.clone()).collect(),
        deps: deps
            .iter()
            .map(|(local, mods)| yan_elab::DepScope {
                local: local.clone(),
                modules: mods.iter().map(|(rel, _)| rel.clone()).collect(),
            })
            .collect(),
        load: &sibling,
    });
    if let Some(env) = &env {
        // A module that names one the front end stopped on stops with that module's errors.
        for used in yan_elab::referenced(m, &env.modules) {
            let Some((_, f)) = modules.iter().find(|(n, _)| *n == used) else {
                continue;
            };
            if let Some(sf) = index.get(f)
                && let Ok(Front::Stopped(d)) = front(db, *sf)
            {
                return Loaded::Stopped(d.clone());
            }
        }
    }
    match yan_elab::elaborate_pkg(
        &name,
        file.text(db),
        m,
        opts,
        package_manifest(packages, path),
        env.as_ref(),
    ) {
        Err(u) => Loaded::Stopped(unsupported(
            &name,
            file.text(db),
            &u.what,
            u.at,
            u.code,
            diags,
        )),
        Ok((y, table, mut errs)) => {
            let stop = has_error(&errs);
            errs.extend(diags);
            if let Some(sf) = surface(path) {
                errs.extend(docs::lint(
                    &name,
                    file.text(db),
                    m,
                    sf,
                    pkg.map_or_else(|| lone_sub_item_lint(path), |p| p.sub_item_lint),
                ));
            }
            if stop {
                Loaded::Stopped(errs)
            } else {
                Loaded::Ready(y, table, errs)
            }
        }
    }
}

/// The modules of other packages that `file` names, directly or through each other, so a build
/// of `file` can report what those modules' own manifests and ceilings reject (RFC 0084 D12
/// step 3).
pub fn dependency_modules(db: &dyn Db, file: SourceFile) -> Vec<SourceFile> {
    let packages = PackageGraph::get(db).packages(db);
    let index = FileIndex::get(db).by_path(db);
    let Some(home) = owner(packages, file.path(db)) else {
        return Vec::new();
    };
    let mut seen: Vec<SourceFile> = vec![file];
    let mut out = Vec::new();
    let mut work = vec![file];
    while let Some(f) = work.pop() {
        let path = f.path(db);
        let Some(pkg) = owner(packages, path) else {
            continue;
        };
        let Ok(Front::Hir(h, _)) = front(db, f) else {
            continue;
        };
        let modules = visible_modules(packages, pkg, path);
        let names: Vec<String> = modules.iter().map(|(n, _)| n.clone()).collect();
        for used in yan_elab::referenced(h, &names) {
            let Some(sf) = modules
                .iter()
                .find(|(n, _)| *n == used)
                .and_then(|(_, p)| index.get(p))
            else {
                continue;
            };
            if seen.contains(sf) {
                continue;
            }
            seen.push(*sf);
            work.push(*sf);
            if owner(packages, sf.path(db)).is_some_and(|o| o.root != home.root) {
                out.push(*sf);
            }
        }
    }
    out
}

/// The modules of the package's `src/` root as (dotted path, file).
fn source_modules(p: &Package) -> Vec<(String, PathBuf)> {
    p.modules
        .iter()
        .filter(|m| m.root == Root::Src)
        .map(|m| (m.path.join("."), m.file.clone()))
        .collect()
}

/// The dependencies `pkg` reaches by local name from the file at `path`, each with its modules
/// as (path relative to its own root, file). Dev dependencies are visible outside `src/` only
/// (R636). A dependency the lock does not resolve to a package on disk is absent.
fn dependency_scopes(
    packages: &[Package],
    pkg: &Package,
    path: &Path,
) -> Vec<(String, Vec<(String, PathBuf)>)> {
    let root = layout::locate(packages, path).map(|l| l.root);
    let in_src = root.is_none_or(|r| r == Root::Src);
    let mut out: Vec<(String, Vec<(String, PathBuf)>)> = Vec::new();
    // A black-box file sees its own package exactly as a dependent does (D8).
    if root.is_some_and(Root::is_black_box)
        && let Some(local) = pkg.name.as_deref().and_then(|n| n.rsplit('/').next())
    {
        out.push((local.to_owned(), source_modules(pkg)));
    }
    for (d, found) in dependency_roots(packages, pkg) {
        let wanted = match d.kind {
            yan_elab::DepKind::Normal => true,
            yan_elab::DepKind::Dev => !in_src && root != Some(Root::Examples),
            yan_elab::DepKind::Tool => false,
        };
        if let Some(dep) = found.filter(|_| wanted)
            && !out.iter().any(|(l, _)| *l == d.local)
        {
            out.push((d.local.clone(), source_modules(dep)));
        }
    }
    out
}

/// Every module the file at `path` can name: its package's own, and each dependency's under the
/// dependency's local name (`http` for its `lib`, `http.client` for its `client`).
fn visible_modules(packages: &[Package], pkg: &Package, path: &Path) -> Vec<(String, PathBuf)> {
    let root = layout::locate(packages, path).map(|l| l.root);
    // A black-box file reaches the package only through its local name (D8).
    let mut all = if root.is_some_and(Root::is_black_box) {
        Vec::new()
    } else {
        source_modules(pkg)
    };
    if root == Some(Root::Tests) {
        all.extend(
            pkg.modules
                .iter()
                .filter(|m| m.root == Root::Tests)
                .map(|m| (m.path.join("."), m.file.clone())),
        );
    }
    for (local, mods) in dependency_scopes(packages, pkg, path) {
        all.extend(mods.into_iter().map(|(rel, f)| {
            let global = if rel == "lib" {
                local.clone()
            } else {
                format!("{local}.{rel}")
            };
            (global, f)
        }));
    }
    all
}

/// W861 for a script outside any package: the nearest `yan.toml` above it (R848). Kept as a
/// directory walk because the `yan.toml` of a lone script belongs to no package in the graph.
fn lone_sub_item_lint(path: &Path) -> bool {
    let Some(dir) = path.parent() else {
        return false;
    };
    dir.ancestors()
        .map(|d| d.join("yan.toml"))
        .find(|p| p.is_file())
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| t.parse::<toml::Table>().ok())
        .and_then(|t| t.get("lint")?.get("missing-sub-item-docs")?.as_bool())
        .unwrap_or(false)
}

/// The manifest of the package that owns `path` (19.1). A `package.yag` itself carries its own.
fn package_manifest(packages: &[Package], path: &Path) -> Option<yan_elab::Manifest> {
    if path.file_name().is_some_and(|n| n == "package.yag") {
        return None;
    }
    let pkg = owner(packages, path)?;
    let mut manifest = pkg.manifest.clone();
    manifest.ffi_unconsented = !consented(packages, pkg);
    Some(manifest)
}

/// Every diagnostic of the module: the front end's, the elaborator's and the Yan Core checker's.
#[salsa::tracked(returns(ref))]
pub fn diagnostics(db: &dyn Db, file: SourceFile) -> Result<Vec<Diagnostic>, String> {
    Ok(match load(db, file).clone()? {
        Loaded::Ready(m, _, mut d) => {
            let path = file.path(db).display().to_string();
            d.extend(
                yan_check::check_in(&path, file.text(db), &m, Settings::get(db).release(db))
                    .into_iter()
                    .map(|mut x| {
                        // An error in an item copied from a module this one names, which that module's
                        // own check reports on its file.
                        if x.span.line >= yan_elab::PULLED {
                            x.span = Span {
                                line: 1,
                                col: 1,
                                end_line: 1,
                                end_col: 1,
                            };
                            x.message =
                                format!("in an item of a module this module names: {}", x.message);
                            x.quote.clear();
                        }
                        x
                    }),
            );
            // One cause, one diagnostic: an unconsumed `@linear` binding is E412, not also W812.
            let unconsumed: Vec<_> = d
                .iter()
                .filter(|x| x.code == "E412")
                .map(|x| (x.span.line, x.span.col))
                .collect();
            d.retain(|x| !(x.code == "W812" && unconsumed.contains(&(x.span.line, x.span.col))));
            d
        }
        Loaded::Stopped(d) => d,
    })
}

/// E017 for a construct found where lowering cannot go on with the next item, such as a type
/// declaration other items depend on; at line 1 when no span is known (R649).
fn unsupported(
    file: &str,
    src: &str,
    what: &str,
    at: Option<Span>,
    code: Option<&'static str>,
    mut diags: Vec<Diagnostic>,
) -> Vec<Diagnostic> {
    let at = at.unwrap_or(Span {
        line: 1,
        col: 1,
        end_line: 1,
        end_col: 1,
    });
    let quote = src
        .lines()
        .nth(at.line.saturating_sub(1) as usize)
        .and_then(|l| {
            let to = if at.end_line == at.line {
                at.end_col as usize
            } else {
                l.len() + 1
            };
            l.get(at.col.saturating_sub(1) as usize..to.saturating_sub(1))
        })
        .unwrap_or("");
    let (code, message) = match code {
        Some(code) => (code, what.to_owned()),
        None => ("E017", yan_elab::unsupported_message(what)),
    };
    diags.push(Diagnostic::error(code, file, at, quote, message));
    diags
}

/// The item hashes (3.6) of a module that elaborates.
#[salsa::tracked(returns(ref))]
pub fn hashes(db: &dyn Db, file: SourceFile) -> Option<Vec<Hashed>> {
    match load(db, file) {
        Ok(Loaded::Ready(m, _, _)) => Some(yan_core::hash::hash_module(m)),
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
        let file = db.file(Path::new("m.yag"), SRC);
        let before: Vec<_> = items(&db, file)
            .iter()
            .map(|i| i.hash(&db).clone())
            .collect();
        assert_eq!(before.len(), 2);
        let edited = SRC.replace("f(1)", "f(2)");
        let file = db.file(Path::new("m.yag"), &edited);
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
        let file = db.file(Path::new("m.yag"), SRC);
        assert_eq!(diagnostics(&db, file).as_ref().unwrap().len(), 0);
        let file = db.file(Path::new("m.yag"), &SRC.replace("f(1)", "f(\"a\")"));
        let d = diagnostics(&db, file).as_ref().unwrap();
        assert!(d.iter().any(|d| d.item == "g"), "{d:?}");
        let g = items(&db, file)[1];
        assert_eq!(item_diagnostics(&db, g).len(), d.len());
    }

    #[test]
    fn ffi_is_the_package_ffi_directory() {
        let dir = std::env::temp_dir().join(format!("yan-ffi-{}", std::process::id()));
        let pkg = dir.join("ffi").join("pkg");
        std::fs::create_dir_all(pkg.join("src")).unwrap();
        std::fs::create_dir_all(pkg.join("ffi")).unwrap();
        std::fs::write(pkg.join("package.yag"), "").unwrap();
        std::fs::create_dir_all(pkg.join("src").join("ffi")).unwrap();
        assert!(!is_ffi_module(&pkg.join("src").join("a.yn")));
        assert!(!is_ffi_module(&pkg.join("src").join("ffi").join("a.yn")));
        assert!(is_ffi_module(&pkg.join("ffi").join("a.yn")));
        assert!(is_ffi_module(Path::new("conformance/yn/pass/ffi/a.yn")));
        assert!(is_ffi_module(Path::new("conformance/ub/a.yn")));
        assert!(!is_ffi_module(Path::new("conformance/yn/fail/unsafe/a.yn")));
    }

    #[cfg(unix)]
    #[test]
    fn embed_does_not_follow_a_symlink_out_of_the_root() {
        let dir = std::env::temp_dir().join(format!("yan-embed-{}", std::process::id()));
        let root = dir.join("pkg");
        // A directory left by an earlier run whose pid was reused would make `symlink` fail.
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("yan.toml"), "").unwrap();
        std::fs::write(dir.join("secret.txt"), "s").unwrap();
        std::fs::write(root.join("ok.txt"), "o").unwrap();
        std::os::unix::fs::symlink(dir.join("secret.txt"), root.join("out")).unwrap();
        std::os::unix::fs::symlink(root.join("ok.txt"), root.join("in")).unwrap();
        let mut db = Database::new(false, false);
        let codes = |db: &mut Database, target: &str| -> Vec<String> {
            let src = format!("let t: str = #embed(\"{target}\")\n");
            let f = db.file(&root.join("m.yn"), &src);
            diagnostics(db, f)
                .as_ref()
                .unwrap()
                .iter()
                .map(|d| d.code.clone())
                .collect()
        };
        assert_eq!(codes(&mut db, "out"), ["E267"]);
        assert!(codes(&mut db, "in").is_empty());
    }
}

#[cfg(test)]
mod selftest {
    use super::*;

    fn codes(path: &str, src: &str) -> Vec<String> {
        let mut db = Database::new(false, false);
        let f = db.file(Path::new(path), src);
        diagnostics(&db, f)
            .as_ref()
            .expect("supported")
            .iter()
            .map(|d| d.code.clone())
            .collect()
    }

    #[test]
    fn a_var_method_on_a_let_parameter_is_e403() {
        let src = "def add(dst: list[int], x: int):\n    dst.append(x)\n";
        assert_eq!(codes("t.yag", src), ["E403"]);
        let ok = "def add(var dst: list[int], x: int):\n    dst.append(x)\n";
        assert!(codes("t.yag", ok).is_empty());
    }

    #[test]
    fn a_var_method_on_a_temporary_is_e403() {
        let src = "def f():\n    d = {\"a\": [1]}\n    d.setdefault(\"b\", []).append(2)\n";
        assert_eq!(codes("t.yag", src), ["E403"]);
    }

    #[test]
    fn a_sink_parameter_may_change_in_place() {
        let src = "def grow(take xs: list[int]) -> list[int]:\n    xs.append(9)\n    return xs\n";
        assert!(codes("t.yag", src).is_empty());
    }

    #[test]
    fn a_yn_let_stays_immutable() {
        let src = "main() =\n  let total = 0\n  total += 1\n";
        assert_eq!(codes("t.yn", src), ["E403"]);
        let ok = "main() =\n  var total = 0\n  total += 1\n";
        assert!(codes("t.yn", ok).is_empty());
    }

    const ARGS: &str = "def f(\n    #: the x\n    x: int,\n    y: list[int],\n) -> int:\n    \"\"\"Does f.\n\n    Args:\n        x: also x\n        y (list[str]): the y\n        q: nothing\n    \"\"\"\n    return x + len(y)\n";

    #[test]
    fn args_section_diagnostics() {
        assert_eq!(codes("t.yag", ARGS), ["E280", "E281", "W860"]);
    }

    #[test]
    fn missing_sub_item_docs_are_opt_in() {
        let dir = std::env::temp_dir().join(format!("yan-w861-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = "pub def f(x: int) -> int:\n    \"\"\"Does f.\"\"\"\n    return x\n";
        let path = dir.join("m.yag");
        assert!(!codes(path.to_str().unwrap(), src).contains(&"W861".to_owned()));
        std::fs::write(
            dir.join("yan.toml"),
            "[lint]\nmissing-sub-item-docs = true\n",
        )
        .unwrap();
        assert!(codes(path.to_str().unwrap(), src).contains(&"W861".to_owned()));
        std::fs::remove_dir_all(&dir).ok();
    }
}
