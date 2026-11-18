//! The layout model (RFC 0084 D1, D2, D4): which directories of a package hold modules, the module
//! path of each file, and the layout errors. One function maps a path to its place, so no tool derives
//! a module name from a file stem again.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use yan_diag::{Diagnostic, Span};

use crate::graph::{Package, owner};

/// The source roots of a package (D2). A source root is not a namespace: `src/a.yn` is `a`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Root {
    Src,
    Bin,
    Ffi,
    Ub,
    Tests,
    Examples,
    Benches,
}

impl Root {
    /// `tests/`, `examples/` and `benches/` see the package as a dependent does (D8).
    pub fn is_black_box(self) -> bool {
        matches!(self, Root::Tests | Root::Examples | Root::Benches)
    }

    /// The roots whose modules are elaborated with the package's names in scope.
    pub fn is_package_code(self) -> bool {
        self == Root::Src || self.is_black_box()
    }
}

const ROOTS: [(&str, Root); 7] = [
    ("src", Root::Src),
    ("bin", Root::Bin),
    ("ffi", Root::Ffi),
    ("ub", Root::Ub),
    ("tests", Root::Tests),
    ("examples", Root::Examples),
    ("benches", Root::Benches),
];

/// Top-level module names an own module may not take (D4); `ffi` and `ub` are the foreign roots.
const RESERVED: [&str; 5] = ["std", "core", "alloc", "ffi", "ub"];

const DEVICES: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// A file's place in its package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Located {
    /// The directory holding `package.yag`.
    pub package: PathBuf,
    pub root: Root,
    /// Directory path under the source root, then the stem. Under `bin/`, `ffi/`, `ub/`, `tests/`,
    /// `examples/` and `benches/` the root's name leads the path (`ffi.libc`), except under `src/`.
    pub module: Vec<String>,
}

/// The place of `path` in the package that owns it, or `None` outside every package and outside the
/// source roots of its package.
pub fn locate(packages: &[Package], path: &Path) -> Option<Located> {
    let pkg = owner(packages, path)?;
    place(&pkg.root, path)
}

fn place(root: &Path, path: &Path) -> Option<Located> {
    let rel = path.strip_prefix(root).ok()?;
    let mut parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let (name, kind) = {
        let first = parts.first()?;
        ROOTS.iter().find(|(n, _)| n == first).copied()?
    };
    parts.remove(0);
    let file = parts.pop()?;
    let stem = file
        .strip_suffix(".yn")
        .or_else(|| file.strip_suffix(".yag"))?;
    parts.push(stem.to_owned());
    if kind != Root::Src {
        parts.insert(0, name.to_owned());
    }
    Some(Located {
        package: root.to_owned(),
        root: kind,
        module: parts,
    })
}

/// One module of a package: its place and its file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    pub root: Root,
    pub path: Vec<String>,
    pub file: PathBuf,
}

/// A layout error, reported on the file it names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub file: PathBuf,
    pub message: String,
}

/// Every module of the package rooted at `root`, in path order, and the layout errors.
pub fn scan(root: &Path) -> (Vec<Module>, Vec<Issue>) {
    let mut files = Vec::new();
    for (name, _) in ROOTS {
        collect(&root.join(name), &mut files);
    }
    files.sort();
    let mut modules: BTreeMap<(Root, Vec<String>), Vec<PathBuf>> = BTreeMap::new();
    let mut issues = Vec::new();
    for f in files {
        let Some(at) = place(root, &f) else { continue };
        for (i, seg) in at.module.iter().enumerate() {
            if let Some(why) = bad_name(seg) {
                issues.push(Issue {
                    file: f.clone(),
                    message: format!("`{seg}` cannot name a module or directory: {why}"),
                });
                break;
            }
            if i == 0 && at.root == Root::Src && RESERVED.contains(&seg.as_str()) {
                issues.push(Issue {
                    file: f.clone(),
                    message: format!(
                        "`{seg}` is a reserved root name and cannot be a top-level module"
                    ),
                });
                break;
            }
        }
        modules.entry((at.root, at.module)).or_default().push(f);
    }
    let mut out = Vec::new();
    for ((kind, path), files) in modules {
        if let [first, rest @ ..] = &files[..] {
            for f in rest {
                issues.push(Issue {
                    file: f.clone(),
                    message: format!(
                        "module `{}` is also defined by `{}`; one path has one file",
                        path.join("."),
                        first.display()
                    ),
                });
            }
            out.push(Module {
                root: kind,
                path,
                file: first.clone(),
            });
        }
    }
    (out, issues)
}

/// E024 on `package.yag` for a dependency local name that cannot be a root (D4, R635): a reserved
/// root, an own top-level module, or the name of another dependency. Each names both sides.
pub fn dependency_issues(
    root: &Path,
    manifest: &yan_elab::Manifest,
    modules: &[Module],
) -> Vec<Issue> {
    let file = root.join("package.yag");
    let mut issues = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for d in manifest
        .dep_names
        .iter()
        .filter(|d| d.kind != yan_elab::DepKind::Tool)
    {
        let name = d.local.as_str();
        let why = if RESERVED.contains(&name) {
            Some(format!("`{name}` is a reserved root name"))
        } else if modules
            .iter()
            .any(|m| m.root == Root::Src && m.path.first().is_some_and(|s| s == name))
        {
            Some(format!("the package has a top-level module `{name}`"))
        } else if seen.contains(&name) {
            Some(format!("another dependency is already named `{name}`"))
        } else {
            None
        };
        seen.push(name);
        if let Some(why) = why {
            issues.push(Issue {
                file: file.clone(),
                message: format!(
                    "dependency `{}` is a root named `{name}`, and {why}; rename one side (RFC 0084 D4)",
                    d.requirement
                ),
            });
        }
    }
    issues
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if e.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if p.is_dir() {
            collect(&p, out);
        } else if matches!(p.extension().and_then(|x| x.to_str()), Some("yn" | "yag")) {
            out.push(p);
        }
    }
}

/// Why `path` cannot be the path of a module under `src/`, or `None` (D4).
pub fn path_issue(path: &[String]) -> Option<String> {
    for (i, seg) in path.iter().enumerate() {
        if let Some(why) = bad_name(seg) {
            return Some(yan_diag::message(
                "E024.bad_name",
                &[("name", seg), ("why", why)],
            ));
        }
        if i == 0 && RESERVED.contains(&seg.as_str()) {
            return Some(yan_diag::message("E024.reserved", &[("name", seg)]));
        }
    }
    None
}

/// A value name (13.1) that is not a device name and not too long to print.
fn bad_name(s: &str) -> Option<&'static str> {
    let mut chars = s.chars();
    let valid = chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && !s.ends_with('_')
        && !s.contains("__")
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !valid {
        return Some("a name is lowercase words joined by single underscores");
    }
    if DEVICES.contains(&s) {
        return Some("it is a device name on Windows");
    }
    None
}

/// E024 for each layout error of a package, on the offending file.
pub fn diagnostics(issues: &[Issue]) -> Vec<Diagnostic> {
    let top = Span {
        line: 1,
        col: 1,
        end_line: 1,
        end_col: 1,
    };
    issues
        .iter()
        .map(|i| {
            Diagnostic::error(
                "E024",
                &i.file.display().to_string(),
                top,
                "",
                i.message.clone(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(dir: &Path, rel: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "").unwrap();
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("yan-layout-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_file_path_is_its_module_path() {
        let root = Path::new("/p");
        let at = |s: &str| place(root, Path::new(s)).map(|l| (l.root, l.module.join(".")));
        assert_eq!(at("/p/src/lib.yn"), Some((Root::Src, "lib".into())));
        assert_eq!(
            at("/p/src/billing/invoice.yag"),
            Some((Root::Src, "billing.invoice".into()))
        );
        assert_eq!(at("/p/ffi/libc.yn"), Some((Root::Ffi, "ffi.libc".into())));
        assert_eq!(
            at("/p/tests/support/db.yn"),
            Some((Root::Tests, "tests.support.db".into()))
        );
        assert_eq!(at("/p/stray.yn"), None);
        assert_eq!(at("/p/src/readme.md"), None);
    }

    #[test]
    fn a_file_and_a_directory_of_one_stem_are_a_module_and_its_child() {
        let d = tmp("sibling");
        touch(&d, "src/billing.yn");
        touch(&d, "src/billing/invoice.yag");
        let (mods, issues) = scan(&d);
        assert!(issues.is_empty(), "{issues:?}");
        let paths: Vec<String> = mods.iter().map(|m| m.path.join(".")).collect();
        assert_eq!(paths, ["billing", "billing.invoice"]);
    }

    #[test]
    fn one_path_with_two_extensions_is_an_error() {
        let d = tmp("dup");
        touch(&d, "src/fees.yn");
        touch(&d, "src/fees.yag");
        let (_, issues) = scan(&d);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].message.contains("module `fees`"));
    }

    #[test]
    fn bad_stems_and_reserved_roots_are_errors() {
        let d = tmp("names");
        touch(&d, "src/Fees.yn");
        touch(&d, "src/my-mod.yn");
        touch(&d, "src/std.yn");
        touch(&d, "src/nul.yn");
        touch(&d, "src/ok/std.yn");
        let (_, issues) = scan(&d);
        let bad: Vec<String> = issues
            .iter()
            .map(|i| i.file.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(bad, ["Fees.yn", "my-mod.yn", "nul.yn", "std.yn"]);
    }

    #[test]
    fn hidden_directories_and_other_files_are_not_modules() {
        let d = tmp("hidden");
        touch(&d, "src/.cache/x.yn");
        touch(&d, "src/notes.txt");
        let (mods, issues) = scan(&d);
        assert!(mods.is_empty() && issues.is_empty());
    }
}
