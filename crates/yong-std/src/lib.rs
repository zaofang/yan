//! The standard library: the intrinsic table and the `.jian` sources (spec 22.2, R474).

pub mod intrinsics;

/// `(module, source)` for each `std.<module>`.
pub const SOURCES: &[(&str, &str)] = &[
    ("cmp", include_str!("../lib/std/cmp.jian")),
    ("dict", include_str!("../lib/std/dict.jian")),
    ("fmt", include_str!("../lib/std/fmt.jian")),
    ("fs", include_str!("../lib/std/fs.jian")),
    ("io", include_str!("../lib/std/io.jian")),
    ("list", include_str!("../lib/std/list.jian")),
    ("log", include_str!("../lib/std/log.jian")),
    ("math", include_str!("../lib/std/math.jian")),
    ("prelude", include_str!("../lib/std/prelude.jian")),
    ("rand", include_str!("../lib/std/rand.jian")),
    ("result", include_str!("../lib/std/result.jian")),
    ("set", include_str!("../lib/std/set.jian")),
    ("test", include_str!("../lib/std/test.jian")),
    ("text", include_str!("../lib/std/text.jian")),
];
