//! Std tiers (17.1, R562): a module's tier is the `lib/` directory its files live in, and a module
//! may refer only to modules of its own tier or a lower one. build.rs runs [`check`] on every
//! file under `lib/`.

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    Core,
    Alloc,
    Std,
}

impl Tier {
    /// Lowest first.
    pub const ALL: [Tier; 3] = [Tier::Core, Tier::Alloc, Tier::Std];

    pub fn dir(self) -> &'static str {
        match self {
            Tier::Core => "core",
            Tier::Alloc => "alloc",
            Tier::Std => "std",
        }
    }
}

/// The modules `src` names by a `std.<module>` path, each resolved to the longest known module.
fn refs<'a>(src: &str, modules: &[(&'a str, Tier)]) -> Vec<(&'a str, Tier)> {
    let mut out = Vec::new();
    for (i, _) in src.match_indices("std.") {
        let ident = |c: char| c.is_alphanumeric() || c == '_';
        if src[..i].chars().next_back().is_some_and(ident) {
            continue;
        }
        let path: String = src[i + 4..]
            .chars()
            .take_while(|&c| ident(c) || c == '.')
            .collect();
        let found = modules
            .iter()
            .filter(|(m, _)| path == *m || path.starts_with(&format!("{m}.")))
            .max_by_key(|(m, _)| m.len());
        out.extend(found);
    }
    out
}

/// Each reference from a module to a module of a higher tier, as a message. `files` holds
/// `(module, tier, source)`, one entry per file.
pub fn check(files: &[(&str, Tier, &str)]) -> Vec<String> {
    let modules: Vec<(&str, Tier)> = files.iter().map(|&(m, t, _)| (m, t)).collect();
    let mut out = Vec::new();
    for &(module, tier, src) in files {
        for (to, to_tier) in refs(src, &modules) {
            if to_tier > tier {
                out.push(format!(
                    "std.{module} ({}) refers to std.{to} ({}), a higher tier",
                    tier.dir(),
                    to_tier.dir()
                ));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_reference_to_a_higher_tier() {
        let files = [
            ("fs", Tier::Std, "pub trait Fs: ..."),
            ("iter", Tier::Core, "def f() -> std.iter.ops.X: ..."),
            ("iter.ops", Tier::Core, ""),
            ("list", Tier::Alloc, "def g(x: std.fs.Fs) -> mystd.fs: ..."),
            ("test", Tier::Std, "def h() -> std.fs.Fs: ..."),
        ];
        assert_eq!(
            check(&files),
            ["std.list (alloc) refers to std.fs (std), a higher tier"]
        );
    }
}
