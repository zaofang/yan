//! Generates the intrinsic registry (R561) from the `@intrinsic` and `@capability` declarations
//! in `lib/std`. The sources are canonical Jian, one signature per line, so a line scan suffices;
//! yong-elab's selftest checks the result against the parser.

use std::fmt::Write as _;
use std::path::Path;
use std::{env, fs};

#[path = "src/intrinsics.rs"]
#[allow(dead_code)]
mod intrinsics;

struct Row {
    path: String,
    sig: String,
    min: usize,
    max: usize,
    effect: &'static str,
    fails: bool,
    callback: bool,
}

/// Top-level comma-separated parts of `s`.
fn split_top(s: &str) -> Vec<&str> {
    let (mut out, mut depth, mut from) = (Vec::new(), 0i32, 0);
    for (i, c) in s.char_indices() {
        match c {
            '[' | '(' | '{' => depth += 1,
            ']' | ')' | '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push(s[from..i].trim());
                from = i + 1;
            }
            _ => {}
        }
    }
    out.push(s[from..].trim());
    out.retain(|p| !p.is_empty());
    out
}

/// `def name[..](params) -> ret: ...` as a row.
fn row(path: String, line: &str, cap: bool) -> Row {
    let sig = line.trim().to_owned();
    let open = sig.find('(').expect("a signature has a parameter list");
    let mut depth = 0;
    let close = sig[open..]
        .char_indices()
        .find_map(|(i, c)| {
            match c {
                '(' | '[' => depth += 1,
                ')' | ']' => depth -= 1,
                _ => {}
            }
            (depth == 0).then_some(open + i)
        })
        .expect("balanced parameters");
    let params: Vec<&str> = split_top(&sig[open + 1..close])
        .into_iter()
        .filter(|p| *p != "/")
        .collect();
    let ret = &sig[close..];
    Row {
        path,
        min: params.iter().filter(|p| !p.contains(" = ")).count(),
        max: params.len(),
        effect: if cap {
            "Cap"
        } else if params.iter().any(|p| p.starts_with("var ")) {
            "Mutates"
        } else {
            "Pure"
        },
        fails: ret.contains(" fail "),
        callback: params.iter().any(|p| p.contains("Callable[")),
        sig,
    }
}

fn scan(dir: &Path, root: &Path, rows: &mut Vec<Row>) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            scan(&p, root, rows);
            continue;
        }
        println!("cargo::rerun-if-changed={}", p.display());
        let rel = p.strip_prefix(root).unwrap().with_extension("");
        let module = rel.to_str().unwrap().replace('/', ".");
        let src = fs::read_to_string(&p).unwrap();
        let (mut intrinsic, mut cap) = (false, false);
        for line in src.lines() {
            if line == "@intrinsic" {
                intrinsic = true;
            } else if line == "@capability" {
                cap = true;
            } else if line.starts_with('@') {
            } else if let Some(def) = line.strip_prefix("pub def ") {
                if intrinsic {
                    let name = def.split(['[', '(']).next().unwrap();
                    rows.push(row(format!("std.{module}.{name}"), line, false));
                }
                intrinsic = false;
                cap = false;
            } else if line.starts_with("pub trait ") {
                intrinsic = false;
            } else if let Some(def) = line.strip_prefix("    def ").filter(|_| cap) {
                let name = def.split(['[', '(']).next().unwrap();
                rows.push(row(format!("std.{module}.{name}"), line, true));
            } else if !line.starts_with(' ') && !line.is_empty() {
                intrinsic = false;
                cap = false;
            }
        }
    }
}

/// The `<op>_<scalar>` family and `exclusive_check` (assumption Y4).
fn scalars(rows: &mut Vec<Row>) {
    const OPS: [&str; 19] = [
        "add_checked",
        "sub_checked",
        "mul_checked",
        "floordiv",
        "mod",
        "and",
        "or",
        "xor",
        "div",
        "add",
        "sub",
        "mul",
        "eq",
        "ne",
        "lt",
        "le",
        "gt",
        "ge",
        "not",
    ];
    let tys = intrinsics::INTS
        .iter()
        .chain(&intrinsics::FLOATS)
        .chain(&["bigint", "bool"]);
    let names = std::iter::once("exclusive_check".to_owned())
        .chain(tys.flat_map(|t| OPS.iter().map(move |o| format!("{o}_{t}"))));
    for name in names {
        if let Some(s) = intrinsics::lookup(&name) {
            rows.push(Row {
                sig: format!("{name}: {} args -> {}", s.arity, s.ret),
                path: name,
                min: s.arity,
                max: s.arity,
                effect: "Pure",
                fails: false,
                callback: false,
            });
        }
    }
}

fn main() {
    let root = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("lib/std");
    let mut rows = Vec::new();
    scan(&root, &root, &mut rows);
    scalars(&mut rows);
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    let mut out = String::new();
    writeln!(out, "pub static ENTRIES: [Entry; {}] = [", rows.len()).unwrap();
    for r in &rows {
        writeln!(
            out,
            "    Entry {{ path: {:?}, sig: {:?}, min_args: {}, max_args: {}, effect: Effect::{}, fails: {}, callback: {} }},",
            r.path, r.sig, r.min, r.max, r.effect, r.fails, r.callback
        )
        .unwrap();
    }
    writeln!(
        out,
        "];\n\n/// Ids by path, `.` spelled `_` and upper-cased.\npub mod id {{\n    use super::Id;"
    )
    .unwrap();
    for (i, r) in rows.iter().enumerate() {
        let c = r.path.replace('.', "_").to_uppercase();
        writeln!(out, "    pub const {c}: Id = Id({i});").unwrap();
    }
    writeln!(out, "}}").unwrap();
    let dst = Path::new(&env::var("OUT_DIR").unwrap()).join("registry.rs");
    fs::write(dst, out).unwrap();
    println!("cargo::rerun-if-changed=src/intrinsics.rs");
}
