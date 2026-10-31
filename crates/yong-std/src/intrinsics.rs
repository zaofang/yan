//! Intrinsic signatures, spelled `<op>_<scalar>` (assumption Y4). `bigint` arithmetic never
//! overflows, so its ops have no `_checked` suffix.

pub const INTS: [&str; 10] = [
    "i8", "i16", "i32", "i64", "i128", "u8", "u16", "u32", "u64", "Addr",
];
pub const FLOATS: [&str; 2] = ["f32", "f64"];

pub struct Sig {
    pub arity: usize,
    pub ret: String,
}

fn sig(arity: usize, ret: &str) -> Option<Sig> {
    Some(Sig {
        arity,
        ret: ret.to_owned(),
    })
}

pub fn lookup(name: &str) -> Option<Sig> {
    if name == "exclusive_check" {
        return sig(2, "Unit");
    }
    let (op, ty) = name.rsplit_once('_')?;
    let cmp = matches!(op, "eq" | "ne" | "lt" | "le" | "gt" | "ge");
    if INTS.contains(&ty) {
        return match op {
            "add_checked" | "sub_checked" | "mul_checked" | "floordiv" | "mod" | "and" | "or"
            | "xor" => sig(2, ty),
            "div" => sig(2, "f64"),
            _ if cmp => sig(2, "bool"),
            _ => None,
        };
    }
    if FLOATS.contains(&ty) {
        return match op {
            "add" | "sub" | "mul" | "div" => sig(2, ty),
            _ if cmp => sig(2, "bool"),
            _ => None,
        };
    }
    if ty == "bigint" {
        return match op {
            "add" | "sub" | "mul" | "floordiv" | "mod" => sig(2, ty),
            _ if cmp => sig(2, "bool"),
            _ => None,
        };
    }
    match (ty, op) {
        ("bool", "and" | "or" | "eq" | "ne") => sig(2, "bool"),
        ("bool", "not") => sig(1, "bool"),
        _ => None,
    }
}
