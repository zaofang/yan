//! The standard library: the intrinsic registry (R561) and the `.xin` sources (spec 22.2, R474).

pub mod intrinsics;
pub mod regex;
pub mod registry;

/// `(module, source)` for each `std.<module>`. std.collections declares one submodule per type,
/// since their methods share names.
pub const SOURCES: &[(&str, &str)] = &[
    ("cmp", include_str!("../lib/std/cmp.xin")),
    (
        "collections.deque",
        include_str!("../lib/std/collections/deque.xin"),
    ),
    (
        "collections.heap",
        include_str!("../lib/std/collections/heap.xin"),
    ),
    (
        "collections.sorted_dict",
        include_str!("../lib/std/collections/sorted_dict.xin"),
    ),
    (
        "collections.sorted_set",
        include_str!("../lib/std/collections/sorted_set.xin"),
    ),
    ("dict", include_str!("../lib/std/dict.xin")),
    ("error", include_str!("../lib/std/error.xin")),
    ("fmt", include_str!("../lib/std/fmt.xin")),
    ("fs", include_str!("../lib/std/fs.xin")),
    ("io", include_str!("../lib/std/io.xin")),
    ("iter", include_str!("../lib/std/iter.xin")),
    ("iter.ops", include_str!("../lib/std/iter/ops.xin")),
    ("list", include_str!("../lib/std/list.xin")),
    ("log", include_str!("../lib/std/log.xin")),
    ("math", include_str!("../lib/std/math.xin")),
    ("prelude", include_str!("../lib/std/prelude.xin")),
    ("rand", include_str!("../lib/std/rand.xin")),
    ("regex", include_str!("../lib/std/regex.xin")),
    ("result", include_str!("../lib/std/result.xin")),
    ("set", include_str!("../lib/std/set.xin")),
    ("test", include_str!("../lib/std/test.xin")),
    ("text", include_str!("../lib/std/text.xin")),
];
