//! Names across surfaces (16.1 to 16.3). The projection table gives each built-in type name its
//! spelling in Yong, Xin and Wen; surfaces and the elaborator translate through it rather than
//! keeping tables of their own.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    Yong,
    Xin,
    Wen,
}

/// One row of the 16.3 table for a type name, with the number of type arguments it takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeName {
    pub yong: &'static str,
    pub xin: &'static str,
    pub wen: &'static str,
    pub arity: usize,
}

impl TypeName {
    pub fn get(&self, s: Surface) -> &'static str {
        match s {
            Surface::Yong => self.yong,
            Surface::Xin => self.xin,
            Surface::Wen => self.wen,
        }
    }
}

const fn row(yong: &'static str, xin: &'static str, wen: &'static str, arity: usize) -> TypeName {
    TypeName {
        yong,
        xin,
        wen,
        arity,
    }
}

/// Built-in type names that differ between surfaces (16.3). `int` and `float` are Xin's and
/// Wen's shared spellings of `i64` and `f64`; Xin also reads `int64` (lenient note N27).
pub const TYPES: &[TypeName] = &[
    row("List", "list", "List", 1),
    row("Map", "dict", "Map", 2),
    row("Set", "set", "Set", 1),
    row("Deque", "deque", "Deque", 1),
    row("Heap", "heap", "Heap", 1),
    row("SortedMap", "sorted_dict", "SortedMap", 2),
    row("SortedSet", "sorted_set", "SortedSet", 1),
    row("Iter", "iter", "Iter", 1),
    row("Soa", "soa", "Soa", 1),
    row("BigInt", "bigint", "BigInt", 0),
    row("Str", "str", "str", 0),
    row("Bytes", "bytes", "Bytes", 0),
    row("ByteArray", "bytearray", "ByteArray", 0),
    row("Path", "Path", "Path", 0),
    row("WindowsPath", "WindowsPath", "WindowsPath", 0),
    row("Duration", "Duration", "Duration", 0),
    row("Instant", "Instant", "Instant", 0),
    row("MonoInstant", "MonoInstant", "MonoInstant", 0),
    row("dec128", "decimal", "dec128", 0),
    row("i8", "int8", "i8", 0),
    row("i16", "int16", "i16", 0),
    row("i32", "int32", "i32", 0),
    row("i64", "int", "int", 0),
    row("i128", "int128", "i128", 0),
    row("u8", "uint8", "u8", 0),
    row("u16", "uint16", "u16", 0),
    row("u32", "uint32", "u32", 0),
    row("u64", "uint64", "u64", 0),
    row("u128", "uint128", "u128", 0),
    row("f16", "float16", "f16", 0),
    row("bf16", "bfloat16", "bf16", 0),
    row("f32", "float32", "f32", 0),
    row("f64", "float", "float", 0),
    row("f128", "float128", "f128", 0),
];

/// The table row a surface spells `name`, if `name` is one of its built-in type names. Wen's
/// `i64` and `f64` are rows too: Wen reads both its spellings.
pub fn type_row(name: &str, s: Surface) -> Option<&'static TypeName> {
    TYPES.iter().find(|r| r.get(s) == name).or(match (s, name) {
        (Surface::Wen, "i64") => type_row("int", Surface::Wen),
        (Surface::Wen, "f64") => type_row("float", Surface::Wen),
        _ => None,
    })
}

/// `name`, a built-in type name of surface `from`, as surface `to` spells it.
pub fn type_name(name: &str, from: Surface, to: Surface) -> Option<&'static str> {
    type_row(name, from).map(|r| r.get(to))
}

#[cfg(test)]
mod selftest {
    use super::*;

    #[test]
    fn spellings_are_unique_per_surface() {
        for s in [Surface::Yong, Surface::Xin, Surface::Wen] {
            for (i, a) in TYPES.iter().enumerate() {
                assert!(
                    TYPES[i + 1..].iter().all(|b| a.get(s) != b.get(s)),
                    "{s:?} spells two rows `{}`",
                    a.get(s)
                );
            }
        }
    }

    #[test]
    fn projections() {
        use Surface::*;
        assert_eq!(type_name("sorted_dict", Xin, Yong), Some("SortedMap"));
        assert_eq!(type_name("i64", Wen, Xin), Some("int"));
        assert_eq!(type_name("f32", Wen, Xin), Some("float32"));
        assert_eq!(type_name("str", Xin, Yong), Some("Str"));
        assert_eq!(type_name("Point", Wen, Xin), None);
    }
}
