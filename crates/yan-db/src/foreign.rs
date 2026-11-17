//! A package's foreign set (RFC 0079 D4): the `ffi` modules and extern libraries it holds, computed
//! from source. The one producer of this fact; resolution, publish and the compiler all call it.

use std::path::{Path, PathBuf};

/// An extern library and the object it links (`lib zlib` with `link "z"`; `link` defaults to `name`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ForeignLib {
    pub name: String,
    pub link: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ForeignSet {
    /// Module paths relative to the package root, `/`-separated and sorted.
    pub modules: Vec<String>,
    pub libs: Vec<ForeignLib>,
}

impl ForeignSet {
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty() && self.libs.is_empty()
    }
}

/// The directories whose modules are `ffi` modules (12): only `<root>/ffi/` and `<root>/ub/`, so a
/// nested or vendored directory of that name does not count.
const FFI_DIRS: [&str; 2] = ["ffi", "ub"];

/// 12: a module is an `ffi` module when it sits under `<root>/ffi/` or `<root>/ub/` of its package.
/// The root is the nearest directory above the module with a `package.yag`; without one, the module's
/// own directory must be named `ffi` or `ub` (conformance assumption A9).
pub fn is_ffi_module(path: &Path) -> bool {
    let Some(dir) = path.parent() else {
        return false;
    };
    match dir.ancestors().find(|d| d.join("package.yag").is_file()) {
        Some(root) => dir.strip_prefix(root).is_ok_and(|rel| {
            rel.components()
                .next()
                .is_some_and(|c| FFI_DIRS.iter().any(|d| c.as_os_str() == *d))
        }),
        None => dir
            .file_name()
            .is_some_and(|n| FFI_DIRS.iter().any(|d| n == *d)),
    }
}

/// The foreign set of the package rooted at `root`. Unreadable or unparsable modules still count as
/// modules; their libraries cannot be read, and the module's own check reports the error.
pub fn foreign_set(root: &Path) -> ForeignSet {
    let mut files = Vec::new();
    for d in FFI_DIRS {
        collect(&root.join(d), &mut files);
    }
    files.sort();
    let mut set = ForeignSet::default();
    for f in files {
        let rel = f.strip_prefix(root).unwrap_or(&f);
        set.modules.push(
            rel.components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/"),
        );
        set.libs.extend(libs_of(&f));
    }
    set.libs.sort();
    set.libs.dedup();
    set
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect(&p, out);
        } else if p.extension().is_some_and(|x| x == "yn") {
            out.push(p);
        }
    }
}

fn libs_of(file: &Path) -> Vec<ForeignLib> {
    let Ok(src) = std::fs::read_to_string(file) else {
        return Vec::new();
    };
    let name = file.display().to_string();
    let (ast, _) = yan_yn::parse(&name, &src);
    let Ok((hir, _)) = yan_yn::lower(&name, &src, &ast) else {
        return Vec::new();
    };
    hir.items
        .iter()
        .filter_map(|it| match &it.kind {
            yan_hir::ItemKind::Extern(x) => Some(ForeignLib {
                name: x.lib.clone(),
                link: x.link.clone().unwrap_or_else(|| x.lib.clone()),
            }),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("yan-foreign-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn modules_and_libraries_are_read_from_the_root_ffi_dirs() {
        let d = tmp("set");
        std::fs::create_dir_all(d.join("ffi")).unwrap();
        std::fs::create_dir_all(d.join("src").join("ffi")).unwrap();
        std::fs::write(d.join("package.yag"), "").unwrap();
        std::fs::write(
            d.join("ffi").join("z.yn"),
            "unsafe extern \"C\" lib zlib link \"z\"\n  safe f() -> c_int\n",
        )
        .unwrap();
        std::fs::write(
            d.join("src").join("ffi").join("n.yn"),
            "unsafe extern \"C\" lib nested link \"n\"\n  safe f() -> c_int\n",
        )
        .unwrap();
        let s = foreign_set(&d);
        assert_eq!(s.modules, ["ffi/z.yn"]);
        assert_eq!(
            s.libs,
            [ForeignLib {
                name: "zlib".into(),
                link: "z".into()
            }]
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_package_without_ffi_is_empty() {
        let d = tmp("empty");
        assert!(foreign_set(&d).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
