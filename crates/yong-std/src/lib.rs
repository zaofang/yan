//! The standard library: the intrinsic registry (R561) and the `.jian` sources (spec 22.2, R474).

pub mod intrinsics;
pub mod regex;
pub mod registry;

/// `(module, source)` for each `std.<module>`. std.collections declares one submodule per type,
/// since their methods share names.
pub const SOURCES: &[(&str, &str)] = &[
    ("cmp", include_str!("../lib/std/cmp.jian")),
    (
        "collections.deque",
        include_str!("../lib/std/collections/deque.jian"),
    ),
    (
        "collections.heap",
        include_str!("../lib/std/collections/heap.jian"),
    ),
    (
        "collections.sorted_dict",
        include_str!("../lib/std/collections/sorted_dict.jian"),
    ),
    (
        "collections.sorted_set",
        include_str!("../lib/std/collections/sorted_set.jian"),
    ),
    ("dict", include_str!("../lib/std/dict.jian")),
    ("error", include_str!("../lib/std/error.jian")),
    ("fmt", include_str!("../lib/std/fmt.jian")),
    ("fs", include_str!("../lib/std/fs.jian")),
    ("io", include_str!("../lib/std/io.jian")),
    ("iter", include_str!("../lib/std/iter.jian")),
    ("iter.ops", include_str!("../lib/std/iter/ops.jian")),
    ("list", include_str!("../lib/std/list.jian")),
    ("log", include_str!("../lib/std/log.jian")),
    ("math", include_str!("../lib/std/math.jian")),
    ("prelude", include_str!("../lib/std/prelude.jian")),
    ("rand", include_str!("../lib/std/rand.jian")),
    ("regex", include_str!("../lib/std/regex.jian")),
    ("result", include_str!("../lib/std/result.jian")),
    ("set", include_str!("../lib/std/set.jian")),
    ("test", include_str!("../lib/std/test.jian")),
    ("text", include_str!("../lib/std/text.jian")),
];
