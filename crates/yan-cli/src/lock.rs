//! `yan.lock` (19.1, R892 to R896): reading it, E019 for a newer format, E020 under `--locked`.

use std::path::Path;

use yan_diag::{Diagnostic, Span};

/// The newest lock `format` this toolchain reads.
const FORMAT: i64 = 1;

/// The identity a requirement `"@org/name x.y[.z]"` pins: the name and its major (the minor in 0.y.z).
fn identity(name: &str, version: &str) -> (String, String) {
    let mut parts = version.split('.');
    let major = parts.next().unwrap_or("");
    let key = match major {
        "0" => format!("0.{}", parts.next().unwrap_or("0")),
        m => m.to_owned(),
    };
    (name.to_owned(), key)
}

fn requirement(req: &str) -> Option<(String, String)> {
    let (name, version) = req.split_once(' ')?;
    Some(identity(name, version.trim()))
}

fn fail(code: &str, message: String) -> Box<Diagnostic> {
    Box::new(Diagnostic::error(
        code,
        "yan.lock",
        Span::default(),
        "",
        message,
    ))
}

/// Check `yan.lock` of the package at `root` against its manifest requirements. A missing lock
/// is an error only under `locked`; a lock is read, never written.
pub fn check(root: &Path, requirements: &[String], locked: bool) -> Result<(), Box<Diagnostic>> {
    let path = root.join("yan.lock");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return if locked && !requirements.is_empty() {
            Err(fail("E020", "`yan.lock` is missing".into()))
        } else {
            Ok(())
        };
    };
    let table: toml::Table = text
        .parse()
        .map_err(|e| fail("E020", format!("`yan.lock` does not parse: {e}")))?;
    let format = table.get("format").and_then(toml::Value::as_integer);
    match format {
        Some(f) if f > FORMAT => {
            return Err(fail(
                "E019",
                format!(
                    "`yan.lock` has format {f}; yan {} reads up to {FORMAT}",
                    env!("CARGO_PKG_VERSION")
                ),
            ));
        }
        Some(_) => {}
        None => return Err(fail("E020", "`yan.lock` has no integer `format`".into())),
    }
    if !locked {
        return Ok(());
    }
    let pinned: Vec<(String, String)> = table
        .get("package")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| {
            Some(identity(
                p.get("name")?.as_str()?,
                p.get("version")?.as_str()?,
            ))
        })
        .collect();
    for r in requirements {
        match requirement(r) {
            Some(id) if pinned.contains(&id) => {}
            _ => {
                return Err(fail("E020", format!("`yan.lock` does not pin `{r}`")));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(lock: Option<&str>, reqs: &[&str], locked: bool) -> Option<String> {
        let dir = std::env::temp_dir().join(format!("yan-lock-{}", std::process::id()));
        let dir = dir.join(format!(
            "{:x}",
            lock.map_or(0, |l| l.len()) + reqs.len() * 7919
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(dir.join("yan.lock"));
        if let Some(l) = lock {
            std::fs::write(dir.join("yan.lock"), l).unwrap();
        }
        let reqs: Vec<String> = reqs.iter().map(|s| (*s).to_owned()).collect();
        check(&dir, &reqs, locked).err().map(|d| d.code)
    }

    const LOCK: &str = "format = 1\n\n[[package]]\nname = \"@a/b\"\nversion = \"1.4.2\"\n";

    #[test]
    fn newer_format_is_e019_even_unlocked() {
        assert_eq!(
            run(Some("format = 2\n"), &[], false).as_deref(),
            Some("E019")
        );
        assert_eq!(run(Some("format = 1\n"), &[], false), None);
    }

    #[test]
    fn missing_lock_is_e020_only_under_locked_with_dependencies() {
        assert_eq!(run(None, &["@a/b 1.4"], true).as_deref(), Some("E020"));
        assert_eq!(run(None, &["@a/b 1.4"], false), None);
        assert_eq!(run(None, &[], true), None);
    }

    #[test]
    fn locked_requires_each_name_and_major_pinned() {
        assert_eq!(run(Some(LOCK), &["@a/b 1.0"], true), None);
        assert_eq!(
            run(Some(LOCK), &["@a/b 2.0"], true).as_deref(),
            Some("E020")
        );
        assert_eq!(
            run(Some(LOCK), &["@a/c 1.0"], true).as_deref(),
            Some("E020")
        );
    }
}
