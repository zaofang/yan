//! `yan toolchain`: compare the installed toolchain with the `yan` requirement of the package and
//! its locked dependencies (20.3, RFC 0100 D2). It fetches nothing.

use std::path::Path;

use yan_diag::{Diagnostic, Span};

use crate::graph::around_dir;

pub const INSTALLED: &str = env!("CARGO_PKG_VERSION");

/// `"0.1.x"` as the lowest version it admits: `x` and `*` count as 0.
fn parse(req: &str) -> Option<[u64; 3]> {
    let mut v = [0; 3];
    let mut n = 0;
    for part in req.trim().split('.') {
        if n == 3 {
            return None;
        }
        v[n] = match part {
            "x" | "*" => 0,
            p => p.parse().ok()?,
        };
        n += 1;
    }
    (n > 0).then_some(v)
}

/// The maximum `yan` requirement over the package and its locked dependencies, with the package
/// that holds it. A package that names none requires nothing.
fn required(dir: &Path) -> Result<Option<(String, String)>, String> {
    let mut max: Option<([u64; 3], String, String)> = None;
    for p in around_dir(dir) {
        let Some(req) = p.manifest.yan.clone() else {
            continue;
        };
        let v = parse(&req).ok_or_else(|| {
            format!(
                "`yan = \"{req}\"` of {} is not a version requirement",
                p.name.as_deref().unwrap_or("a package")
            )
        })?;
        if max.as_ref().is_none_or(|(m, ..)| v > *m) {
            max = Some((v, req, p.name.unwrap_or_default()));
        }
    }
    Ok(max.map(|(_, req, name)| (req, name)))
}

/// The report line, or the E025 diagnostic when the installed toolchain is older.
pub fn check(dir: &Path) -> Result<String, Result<Box<Diagnostic>, String>> {
    let installed = parse(INSTALLED).expect("the crate version parses");
    let Some((req, name)) = required(dir).map_err(Err)? else {
        return Ok(format!(
            "installed {INSTALLED}, no package requires a toolchain"
        ));
    };
    if parse(&req).expect("checked above") > installed {
        return Err(Ok(Box::new(Diagnostic::error(
            "E025",
            "package.yag",
            Span::default(),
            "",
            format!(
                "{name} requires yan {req}, installed {INSTALLED}; fetching toolchains is not available yet"
            ),
        ))));
    }
    Ok(format!("installed {INSTALLED}, required {req} by {name}"))
}
