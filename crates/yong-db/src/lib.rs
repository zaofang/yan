//! The query database (22.2). For now it only loads std: the elaborator depends on no surface
//! crate (R560), so std's Jian sources are parsed here and handed to `yong_elab::install_std`.

/// Each std module: its path below `std.`, its file name, its source and its HIR.
pub fn std_modules() -> Vec<(&'static str, String, &'static str, yong_hir::Module)> {
    yong_std::SOURCES
        .iter()
        .map(|&(module, src)| {
            let file = format!("std/{}.jian", module.replace('.', "/"));
            let (m, diags) = yong_jian::parse(&file, src, true);
            assert!(diags.is_empty(), "std.{module}: {diags:?}");
            (module, file, src, m.expect("std parses"))
        })
        .collect()
}
