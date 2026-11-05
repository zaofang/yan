//! Std names with a Python spelling that differs from the Yan one (17.8), and std names
//! with no mapping, which export rejects by name instead of emitting code that fails later.

pub enum Method {
    /// `recv.new(args)`
    Rename(&'static str),
    /// `func(recv, args)`, `func` defined in `yan_rt.py`
    Shim(&'static str),
    /// Rejected, with the reason.
    No(&'static str),
}

pub fn method(name: &str) -> Option<Method> {
    use Method::*;
    Some(match name {
        "is_digit" => Rename("isdigit"),
        "is_alpha" => Rename("isalpha"),
        "is_alnum" => Rename("isalnum"),
        "is_upper" => Rename("isupper"),
        "is_lower" => Rename("islower"),
        "is_space" => Rename("isspace"),
        "sort_unstable" => Rename("sort"),
        "copy" => Shim("copy"),
        "parse_int" => Shim("parse_int"),
        "parse_float" => Shim("parse_float"),
        "to_str" => Shim("str"),
        "byte_len" => Shim("byte_len"),
        "char_count" => Shim("len"),
        "chars" => Shim("chars"),
        "iter" => Shim("iter_of"),
        "index" => Shim("index_of"),
        "slice_exact" => Shim("slice_exact"),
        "bisect_left" => Shim("bisect_left"),
        "bisect_right" => Shim("bisect_right"),
        "insort" => Shim("insort"),
        "grapheme_count" => No("grapheme segmentation"),
        _ => return None,
    })
}

/// `math.NAME` members defined by the shim; every other `math` member is rejected.
pub const MATH: &[&str] = &[
    "pi",
    "e",
    "inf",
    "nan",
    "is_inf",
    "is_finite",
    "is_nan",
    "sqrt",
    "log",
    "log2",
    "log10",
    "exp",
    "sin",
    "cos",
    "tan",
    "acos",
    "atan2",
    "hypot",
    "pow",
    "powr",
    "tgamma",
    "abs",
    "floor",
    "ceil",
    "trunc",
    "fmod",
    "remainder",
    "copysign",
    "frexp",
    "ldexp",
    "next_up",
    "next_down",
];

/// Std modules and types that have no Python mapping yet.
const UNMAPPED: &[&str] = &[
    "bits",
    "regex",
    "encoding",
    "deque",
    "heap",
    "sorted_dict",
    "sorted_set",
    "cli",
    "proc",
    "seeded",
    "Path",
    "WindowsPath",
    "Duration",
    "Instant",
    "PlainDate",
    "ZonedDateTime",
    "TimeZone",
];

/// The diagnostic for a reference to `name` (a bare name, or the base of `name.member`).
pub fn reject_name(name: &str, member: Option<&str>) -> Option<String> {
    if name == "math" {
        let m = member?;
        return (!MATH.contains(&m)).then(|| format!("std {name}.{m}"));
    }
    UNMAPPED.contains(&name).then(|| format!("std {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn math_members_are_defined_by_the_shim() {
        let shim = include_str!("yan_rt.py");
        let body = &shim[shim.find("math = SimpleNamespace(").unwrap()..];
        for m in MATH {
            assert!(body.contains(&format!("    {m}=")), "{m} missing from shim");
        }
    }

    #[test]
    fn unmapped_std_is_named() {
        assert_eq!(
            reject_name("math", Some("sinpi")).unwrap(),
            "std math.sinpi"
        );
        assert_eq!(reject_name("bits", Some("popcount")).unwrap(), "std bits");
        assert!(reject_name("math", Some("sqrt")).is_none());
        assert!(reject_name("xs", Some("len")).is_none());
    }
}
