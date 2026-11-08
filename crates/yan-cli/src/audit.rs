//! `yan audit` (18.1): the claims a reviewer takes on trust: `unsafe fn` preconditions (6),
//! `unsafe impl`, extern blocks and their `safe`, `uses none` and `@access`/`@stack` claims (11.2,
//! 12, R627). Diffing lockfiles, `--stats` and `--policy` are not yet implemented.

use std::path::Path;

use yan_core::ast::{Contract, FnDef, Item, ItemKind, XItem};
use yan_db::{Database, Loaded};

use crate::test;

pub struct Finding {
    pub kind: &'static str,
    /// `module.item`
    pub item: String,
    pub detail: String,
}

impl Finding {
    pub fn line(&self) -> String {
        if self.detail.is_empty() {
            format!("{} {}", self.kind, self.item)
        } else {
            format!("{} {}: {}", self.kind, self.item, self.detail)
        }
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({"kind": self.kind, "item": self.item, "detail": self.detail})
    }
}

/// Attributes that state a claim the checker cannot verify on an extern.
const CLAIM_ATTRS: [&str; 2] = ["access", "stack"];

fn preconditions(f: &FnDef) -> String {
    let pre: Vec<String> = f
        .contracts
        .iter()
        .filter_map(|c| match c {
            Contract::Requires(e) => Some(yan_core::expr_text(e)),
            _ => None,
        })
        .collect();
    if pre.is_empty() {
        "no preconditions".to_owned()
    } else {
        format!("requires {}", pre.join("; requires "))
    }
}

fn unsafe_fn(f: &FnDef, scope: &str, out: &mut Vec<Finding>) {
    if f.is_unsafe {
        out.push(Finding {
            kind: "unsafe_fn",
            item: format!("{scope}{}", f.path.join(".")),
            detail: preconditions(f),
        });
    }
}

fn item_findings(module: &str, it: &Item, key: &str, out: &mut Vec<Finding>) {
    let at = format!("{module}.{key}");
    let mut push = |kind, item: String, detail: String| out.push(Finding { kind, item, detail });
    match &it.kind {
        ItemKind::Fn(f) | ItemKind::Gen(f) => unsafe_fn(f, &format!("{module}."), out),
        ItemKind::Trait(t) => t
            .sigs
            .iter()
            .for_each(|f| unsafe_fn(f, &format!("{at}."), out)),
        ItemKind::Impl(i) => {
            if i.is_unsafe {
                push("unsafe_impl", at.clone(), String::new());
            }
            i.fns
                .iter()
                .for_each(|f| unsafe_fn(f, &format!("{at}."), out));
        }
        ItemKind::Extern(e) => {
            push("extern", at.clone(), format!("abi {} lib {}", e.abi, e.lib));
            for x in &e.items {
                match x {
                    XItem::Fn {
                        safe,
                        name,
                        uses_none,
                        ..
                    } => {
                        let name = format!("{at}.{name}");
                        if *safe {
                            push("extern_safe", name.clone(), "trusted safe signature".into());
                        }
                        if *uses_none {
                            push("uses_none", name, "trusted: pure, deterministic".into());
                        }
                    }
                    XItem::Static {
                        safe: true, name, ..
                    } => push(
                        "extern_safe",
                        format!("{at}.{name}"),
                        "trusted safe static".into(),
                    ),
                    _ => {}
                }
            }
        }
        _ => {}
    }
    for a in it
        .attrs
        .iter()
        .filter(|a| CLAIM_ATTRS.contains(&a.name.as_str()))
    {
        let kind = if matches!(it.kind, ItemKind::Extern(_)) {
            "trusted_claim"
        } else {
            "claim"
        };
        let args = a
            .args
            .as_ref()
            .map_or(String::new(), |v| format!("({} args)", v.len()));
        out.push(Finding {
            kind,
            item: at.clone(),
            detail: format!("@{}{args}", a.name),
        });
    }
}

/// Findings for every module of the package at `root`, in module order. Modules that do not
/// elaborate are reported as `unchecked`.
pub fn audit(root: &Path) -> Result<Vec<Finding>, String> {
    let mut out = Vec::new();
    for (name, file) in test::modules(root)? {
        let mut db = Database::new(false, false);
        let f = db.open(&file)?;
        let m = match yan_db::load(&db, f) {
            Ok(Loaded::Ready(m, _, _)) => m.clone(),
            _ => {
                out.push(Finding {
                    kind: "unchecked",
                    item: name,
                    detail: "module does not elaborate; its claims are not listed".into(),
                });
                continue;
            }
        };
        for (it, h) in m.items.iter().zip(yan_core::hash::hash_module(&m)) {
            item_findings(&name, it, &h.key, &mut out);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn findings(src: &str) -> Vec<String> {
        let (m, d) = yan_core::parse("m.yir", src);
        let m = m.unwrap();
        assert!(d.is_empty(), "{d:?}");
        let mut out = Vec::new();
        for (it, h) in m.items.iter().zip(yan_core::hash::hash_module(&m)) {
            item_findings("m", it, &h.key, &mut out);
        }
        out.iter().map(Finding::line).collect()
    }

    #[test]
    fn lists_unsafe_extern_and_claims() {
        let src = "module m\n\nunsafe fn poke(let p: i64) -> Unit\n    requires ge_i64(let p, let 0)\n{\n    ret ()\n}\n\nunsafe extern \"C\" lib m link \"libm\" {\n    safe fn cos(x: f64) -> f64 uses none\n    unsafe fn printf(fmt: Ptr[u8], ...) -> i32\n}\n";
        let got = findings(src);
        assert!(
            got.iter()
                .any(|l| l.starts_with("unsafe_fn m.poke: requires")),
            "{got:?}"
        );
        assert!(
            got.iter()
                .any(|l| l.starts_with("extern_safe") && l.contains("cos"))
        );
        assert!(
            got.iter()
                .any(|l| l.starts_with("uses_none") && l.contains("cos"))
        );
        assert!(
            !got.iter()
                .any(|l| l.contains("printf") && l.starts_with("extern_safe"))
        );
    }
}
