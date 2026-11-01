//! Every `@intrinsic` in yong-std's sources has a native arm here, and every native arm is
//! declared, as an intrinsic or as a capability operation (R474).

use std::collections::BTreeSet;
use yong_jian::ast::ItemKind;

const NATIVE: [&str; 7] = [
    include_str!("lib.rs"),
    include_str!("native.rs"),
    include_str!("prelude.rs"),
    include_str!("regex.rs"),
    include_str!("collections.rs"),
    include_str!("math.rs"),
    include_str!("iter.rs"),
];

fn native_paths() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for src in NATIVE {
        for chunk in src.split('"').skip(1).step_by(2) {
            let segs: Vec<&str> = chunk.split('.').collect();
            let word = |s: &str| {
                !s.is_empty()
                    && s.starts_with(|c: char| c.is_ascii_lowercase())
                    && s.bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            };
            // Three segments, or four for a submodule (std.collections.deque.len).
            if matches!(segs.len(), 3 | 4) && segs[0] == "std" && segs[1..].iter().all(|s| word(s))
            {
                out.insert(chunk.to_string());
            }
        }
    }
    out
}

fn declared() -> (BTreeSet<String>, BTreeSet<String>) {
    let (mut intrinsics, mut caps) = (BTreeSet::new(), BTreeSet::new());
    for (module, src) in yong_std::SOURCES {
        let (ast, diags) = yong_jian::parse(&format!("std/{module}.jian"), src, true);
        assert!(diags.is_empty(), "std.{module}: {diags:?}");
        for item in ast.unwrap().items {
            let has = |d: &str| item.decorators.iter().any(|x| x.path == [d]);
            match &item.kind {
                ItemKind::Func(f) if has("intrinsic") => {
                    intrinsics.insert(format!("std.{module}.{}", f.name));
                }
                ItemKind::Trait(t) if has("capability") => {
                    caps.extend(
                        t.sigs
                            .iter()
                            .map(|(_, f)| format!("std.{module}.{}", f.name)),
                    );
                }
                _ => {}
            }
        }
    }
    (intrinsics, caps)
}

#[test]
fn intrinsics_match_native_arms() {
    let native = native_paths();
    let (intrinsics, caps) = declared();
    let missing: Vec<_> = intrinsics.difference(&native).collect();
    assert!(
        missing.is_empty(),
        "@intrinsic without a native arm: {missing:?}"
    );
    let undeclared: Vec<_> = native
        .iter()
        .filter(|p| !intrinsics.contains(*p) && !caps.contains(*p))
        .collect();
    assert!(
        undeclared.is_empty(),
        "native arm without a declaration: {undeclared:?}"
    );
}
