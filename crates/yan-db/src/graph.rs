//! The package graph (RFC 0079 D5): every package the compiler can see, built by the driver from the
//! workspace, `yan.lock`, the store, `vendor/` and overrides, and read by the queries that used to
//! walk a module's ancestors.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

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

/// R1018: a package is consented to when the root of its workspace names it in `allow_ffi`; a package
/// without a name has nothing to name.
pub fn consented(packages: &[Package], pkg: &Package) -> bool {
    let Some(name) = &pkg.name else {
        return true;
    };
    packages
        .iter()
        .find(|p| p.role == Role::Root && p.root == pkg.workspace)
        .is_none_or(|root| root.manifest.allow_ffi.contains(name))
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
