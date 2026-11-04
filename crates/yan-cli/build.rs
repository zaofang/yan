//! Embeds the first `fail/` conformance case per code and surface for `yan explain` (R299).

use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::{Path, PathBuf};

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            files(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut first: BTreeMap<(String, &str), (String, PathBuf, String)> = BTreeMap::new();
    for surface in ["yag", "yn", "yir"] {
        let mut paths = Vec::new();
        files(&root.join(surface).join("fail"), &mut paths);
        for p in paths {
            let Ok(text) = std::fs::read_to_string(&p) else {
                continue;
            };
            let header = |key: &str| {
                text.lines()
                    .filter_map(|l| l.strip_prefix("# ").or_else(|| l.strip_prefix("-- ")))
                    .filter_map(|l| l.strip_prefix(key))
                    .map(str::trim)
                    .collect::<Vec<_>>()
            };
            let flags = header("flags:").join(" ");
            for e in header("expect:") {
                let code = e.split_whitespace().next().unwrap_or("").to_owned();
                let rel = p.strip_prefix(root.parent().unwrap()).unwrap();
                first.entry((code, surface)).or_insert((
                    rel.display().to_string(),
                    p.clone(),
                    flags.clone(),
                ));
            }
        }
    }
    let mut out = String::from("pub const CASES: &[(&str, &str, &str, &str)] = &[\n");
    for ((code, _), (rel, abs, flags)) in first {
        writeln!(
            out,
            "    ({code:?}, {rel:?}, {flags:?}, include_str!({:?})),",
            abs.canonicalize().unwrap()
        )
        .unwrap();
    }
    out.push_str("];\n");
    let dest = Path::new(&std::env::var("OUT_DIR").unwrap()).join("explain_cases.rs");
    std::fs::write(dest, out).unwrap();
}
