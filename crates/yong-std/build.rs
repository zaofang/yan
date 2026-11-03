//! Generates the std source table and the intrinsic registry (R561) from the `@intrinsic` and
//! `@capability` declarations in `lib/<tier>/`, and rejects a reference to a higher tier (R562). The sources are canonical Xin, one signature per line, so a line scan suffices;
//! yong-elab's selftest checks the result against the parser.

use std::fmt::Write as _;
use std::path::Path;
use std::{env, fs};

#[path = "src/intrinsics.rs"]
#[allow(dead_code)]
mod intrinsics;

#[path = "src/tiers.rs"]
mod tiers;

use tiers::Tier;

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

/// A file under `lib/<tier>/`: its module path below `std.`, its tier, path and source.
struct File {
    module: String,
    tier: Tier,
    path: String,
    src: String,
}

fn files(dir: &Path, root: &Path, tier: Tier, out: &mut Vec<File>) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            files(&p, root, tier, out);
            continue;
        }
        println!("cargo::rerun-if-changed={}", p.display());
        let rel = p.strip_prefix(root).unwrap().with_extension("");
        out.push(File {
            module: rel.to_str().unwrap().replace('/', "."),
            tier,
            path: p.to_str().unwrap().to_owned(),
            src: fs::read_to_string(&p).unwrap(),
        });
    }
}

/// The registry rows of one `.xin` source, whose declarations are one signature per line.
fn scan(module: &str, src: &str, rows: &mut Vec<Row>) {
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

/// The `<op>_<scalar>` family and `exclusive_check` (assumption Y4).
fn scalars(rows: &mut Vec<Row>) {
    const OPS: [&str; 21] = [
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
        "shl",
        "shr",
    ];
    let tys = intrinsics::INTS
        .iter()
        .chain(&intrinsics::FLOATS)
        .chain(&["bigint", "bool"]);
    let names = std::iter::once("exclusive_check".to_owned()).chain(tys.flat_map(|t| {
        OPS.iter()
            .chain(&intrinsics::BITS)
            .map(move |o| format!("{o}_{t}"))
    }));
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
    let lib = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("lib");
    let mut all = Vec::new();
    for tier in Tier::ALL {
        let root = lib.join(tier.dir());
        files(&root, &root, tier, &mut all);
    }
    let refs: Vec<_> = all
        .iter()
        .map(|f| (f.module.as_str(), f.tier, f.src.as_str()))
        .collect();
    let bad = tiers::check(&refs);
    assert!(bad.is_empty(), "std tiers (R562):\n{}", bad.join("\n"));
    let mut xin: Vec<&File> = all.iter().filter(|f| f.path.ends_with(".xin")).collect();
    xin.sort_by(|a, b| a.module.cmp(&b.module));
    let mut sources = String::from("pub const SOURCES: &[(&str, &str)] = &[\n");
    let mut tier_rows = String::from("pub const TIERS: &[Tier] = &[\n");
    for f in &xin {
        writeln!(sources, "    ({:?}, include_str!({:?})),", f.module, f.path).unwrap();
        writeln!(tier_rows, "    Tier::{:?},", f.tier).unwrap();
    }
    let out_dir = Path::new(&env::var("OUT_DIR").unwrap()).to_owned();
    fs::write(
        out_dir.join("sources.rs"),
        sources + "];\n\n" + &tier_rows + "];\n",
    )
    .unwrap();

    let mut rows = Vec::new();
    for f in &xin {
        scan(&f.module, &f.src, &mut rows);
    }
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
    fs::write(out_dir.join("registry.rs"), out).unwrap();
    println!("cargo::rerun-if-changed=src/intrinsics.rs");
}
