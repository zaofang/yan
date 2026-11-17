//! `yan metadata --json`: the resolved package graph for editors and dependency bots, so outsiders
//! never parse `package.yd` (RFC 0084 D13).

use std::path::Path;
use std::process::ExitCode;

use serde_json::{Value, json};
use yan_db::{Package, Role, dependency_roots};

use crate::graph::around_dir;

pub fn run(dir: &Path, _json: bool) -> ExitCode {
    let packages = around_dir(dir);
    let Some(top) = packages.iter().find(|p| p.role == Role::Root) else {
        eprintln!("yan: no package.yag at or above {}", dir.display());
        return ExitCode::FAILURE;
    };
    let out = json!({
        "format": 1,
        "root": top.name,
        "packages": packages.iter().map(|p| package(&packages, p)).collect::<Vec<_>>(),
    });
    println!("{}", serde_json::to_string_pretty(&out).expect("json"));
    ExitCode::SUCCESS
}

fn package(all: &[Package], p: &Package) -> Value {
    let rel = |f: &Path| f.strip_prefix(&p.root).unwrap_or(f).display().to_string();
    let role = match p.role {
        Role::Root => "root",
        Role::Member => "member",
        Role::Nested => "nested",
        Role::Locked => "locked",
    };
    json!({
        "name": p.name,
        "version": p.manifest.version,
        "role": role,
        "root": p.root.display().to_string(),
        "capabilities": p.manifest.capabilities,
        "dependencies": dependency_roots(all, p).into_iter().map(|(d, found)| json!({
            "local": d.local,
            "kind": d.kind.field(),
            "requirement": d.requirement,
            "resolved": found.map(|f| json!({"name": f.name, "version": f.manifest.version})),
        })).collect::<Vec<_>>(),
        "modules": p.modules.iter().map(|m| json!({
            "path": m.path.join("."),
            "file": rel(&m.file),
        })).collect::<Vec<_>>(),
    })
}
