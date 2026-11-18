//! The package graph (RFC 0079 D5): every package the compiler can see, built by the driver from the
//! workspace, `yan.lock`, the store, `vendor/` and overrides, and read by the queries that used to
//! walk a module's ancestors.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use yan_diag::{Diagnostic, Span};

use crate::Db;
use crate::closure::{self, Node};
use crate::foreign::ForeignSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The outermost `package.yag` of a workspace.
    Root,
    /// A directory the root lists in `members`.
    Member,
    /// A `package.yag` inside a workspace that the root does not list.
    Nested,
    /// A package `yan.lock` pins, from the store, `vendor/` or an override.
    Locked,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Package {
    pub name: Option<String>,
    pub role: Role,
    /// The source root: the directory holding `package.yag`.
    pub root: PathBuf,
    /// The source root of the workspace root this package belongs to.
    pub workspace: PathBuf,
    pub manifest: yan_elab::Manifest,
    pub foreign: ForeignSet,
    /// Layout errors of the source roots (RFC 0084 D1, D4), found by the driver with the foreign set.
    pub layout: Vec<crate::layout::Issue>,
    /// The modules of the source roots, in path order (RFC 0084 D1).
    pub modules: Vec<crate::layout::Module>,
    /// Requirements as written in `dependencies`, `dev_dependencies` and `tools`.
    pub deps: Vec<String>,
    pub dev_deps: Vec<String>,
    pub tools: Vec<String>,
    /// `[lint] missing-sub-item-docs` of the `yan.toml` at or above the root (R848).
    pub sub_item_lint: bool,
}

/// Every package known to the database. A module outside all of them is a lone script.
#[salsa::input(singleton, debug)]
pub struct PackageGraph {
    #[returns(ref)]
    pub packages: Vec<Package>,
}

/// The package whose source root is the longest prefix of `path`.
pub fn owner<'a>(packages: &'a [Package], path: &Path) -> Option<&'a Package> {
    packages
        .iter()
        .filter(|p| path.starts_with(&p.root))
        .max_by_key(|p| p.root.components().count())
}

/// R1018: a package is consented to when the root of its workspace names it in `allow_ffi`. A
/// package without a name cannot be named there, so it is never consented to.
pub fn consented(packages: &[Package], pkg: &Package) -> bool {
    let Some(name) = &pkg.name else {
        return false;
    };
    packages
        .iter()
        .find(|p| p.role == Role::Root && p.root == pkg.workspace)
        .is_none_or(|root| root.manifest.allow_ffi.contains(name))
}

/// The packages `pkg` names as dependencies, each with the package of the graph it resolves to
/// (RFC 0084 D4, R635): the locked or member package whose name and compatibility major the
/// requirement names. `None` where the lock does not pin it or its source is not on disk.
pub fn dependency_roots<'a>(
    packages: &'a [Package],
    pkg: &'a Package,
) -> Vec<(&'a yan_elab::DepName, Option<&'a Package>)> {
    pkg.manifest
        .dep_names
        .iter()
        .map(|d| {
            let key = closure::requirement(&d.requirement);
            let found = key.as_ref().and_then(|k| {
                packages.iter().find(|p| {
                    p.role != Role::Root
                        && p.name.as_deref() == Some(k.0.as_str())
                        && p.manifest
                            .version
                            .as_deref()
                            .is_some_and(|v| closure::identity(&k.0, v) == *k)
                })
            });
            (d, found)
        })
        .collect()
}

/// E023 and E314 (RFC 0079 D3, D5) for each package of the workspace, on its `package.yag`, once per
/// package rather than per module. The closure runs over every package in the graph, so a member added
/// without `yan lock` or a hand-edited lock is still checked.
#[salsa::tracked(returns(ref))]
pub fn graph_diagnostics(db: &dyn Db, graph: PackageGraph) -> Vec<Diagnostic> {
    let packages = graph.packages(db);
    let at = |p: &Package| p.root.join("package.yag").display().to_string();
    let local = |p: &Package| p.role != Role::Locked;
    let top = Span {
        line: 1,
        col: 1,
        end_line: 1,
        end_col: 1,
    };
    let mut out = Vec::new();
    // Consent is read from the recomputed set, so a lock whose `ffi` array was emptied by hand
    // does not grant it. A locked package is reported on the manifest of its workspace root.
    for p in packages {
        if p.foreign.is_empty() || consented(packages, p) {
            continue;
        }
        if local(p) {
            out.push(Diagnostic::error(
                "E023",
                &at(p),
                top,
                "",
                yan_diag::message("E023.local", &[]),
            ));
        } else {
            out.push(Diagnostic::error(
                "E023",
                &p.workspace.join("package.yag").display().to_string(),
                top,
                "",
                yan_diag::message(
                    "E023.locked",
                    &[("package", p.name.as_deref().unwrap_or("."))],
                ),
            ));
        }
    }
    for p in packages.iter().filter(|p| local(p)) {
        out.extend(crate::layout::diagnostics(&p.layout));
    }
    let nodes: Vec<Node> = packages.iter().map(closure::from_package).collect();
    for (i, message) in closure::dev_cycles(&nodes) {
        if local(&packages[i]) {
            out.push(Diagnostic::error(
                "E334",
                &at(&packages[i]),
                top,
                "",
                message,
            ));
        }
    }
    for p in packages.iter().filter(|p| local(p)) {
        for (d, found) in dependency_roots(packages, p) {
            let Some(member) = found.filter(|m| m.role == Role::Member) else {
                continue;
            };
            let have = member.manifest.version.as_deref().unwrap_or("");
            let need = d.requirement.split_whitespace().nth(1).unwrap_or("");
            if closure::below(have, need) {
                out.push(Diagnostic::error(
                    "E335",
                    &at(p),
                    top,
                    "",
                    yan_diag::message(
                        "E335",
                        &[
                            ("local", &d.local),
                            ("requirement", &d.requirement),
                            ("have", have),
                        ],
                    ),
                ));
            }
        }
    }
    for (i, message) in closure::violations(&nodes) {
        if local(&packages[i]) {
            out.push(Diagnostic::error(
                "E314",
                &at(&packages[i]),
                top,
                "",
                message,
            ));
        }
    }
    out
}

/// Builds the packages around a path: the driver's, since it knows the workspace layout.
pub type GraphBuilder = fn(&Path) -> Vec<Package>;

static BUILDER: OnceLock<GraphBuilder> = OnceLock::new();

/// Install the driver's graph builder; the database calls it for each file no package owns yet.
pub fn install_graph_builder(f: GraphBuilder) {
    let _ = BUILDER.set(f);
}

pub(crate) fn build(path: &Path) -> Vec<Package> {
    BUILDER.get().map_or_else(Vec::new, |f| f(path))
}
