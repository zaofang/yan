//! `yan new` (18.1): write a package from a template. Templates are 0BSD (R352).

use std::path::{Path, PathBuf};

use clap::ValueEnum;

#[derive(Clone, Copy, ValueEnum)]
pub enum Template {
    /// A program reading standard input.
    Cli,
    /// Functions with contracts and examples, and no capabilities.
    Library,
    /// Public functions an agent may call, each holding only the capabilities its `uses` names.
    AgentTool,
    /// The three-line manifest of a workspace member (RFC 0084 D7); not offered to `--template`.
    #[value(skip)]
    Member,
}

impl Template {
    /// The template's files: path in the package, then text with `{name}` for the package name.
    fn files(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Template::Member => &[
                (
                    "package.yag",
                    "package: Package = Package(name=\"{name}\", version=\"0.1.0\", edition=\"next\", yan=\"1.x\")\n",
                ),
                ("src/lib.yag", "\"\"\"The {name} member.\"\"\"\n"),
            ],
            Template::Cli => &[
                ("package.yag", include_str!("../templates/cli/package.yag")),
                (
                    "src/main.yag",
                    include_str!("../templates/cli/src/main.yag"),
                ),
            ],
            Template::Library => &[
                (
                    "package.yag",
                    include_str!("../templates/library/package.yag"),
                ),
                (
                    "src/lib.yag",
                    include_str!("../templates/library/src/lib.yag"),
                ),
            ],
            Template::AgentTool => &[
                (
                    "package.yag",
                    include_str!("../templates/agent-tool/package.yag"),
                ),
                (
                    "src/main.yag",
                    include_str!("../templates/agent-tool/src/main.yag"),
                ),
            ],
        }
    }
}

/// Write the package `name` (`@org/pkg`, or `pkg` for `@local/pkg`) into `parent/pkg`, which
/// must not exist. Returns that directory.
pub fn create(parent: &Path, name: &str, template: Template) -> Result<PathBuf, String> {
    let (full, pkg) = match name.strip_prefix('@') {
        Some(rest) => match rest.split_once('/') {
            Some((org, pkg)) if is_segment(org) => (name.to_owned(), pkg),
            _ => return Err(format!("`{name}` is not `@org/name` or `name`")),
        },
        None => (format!("@local/{name}"), name),
    };
    if !is_segment(pkg) {
        return Err(format!(
            "`{pkg}` is not a package name: lowercase ASCII letters, digits, `-` and `_`, starting with a letter"
        ));
    }
    let dir = parent.join(pkg);
    if dir.exists() {
        return Err(format!("{} already exists", dir.display()));
    }
    for (path, text) in template.files() {
        let file = dir.join(path);
        let parent = file.parent().expect("a file in the package");
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        std::fs::write(&file, text.replace("{name}", &full))
            .map_err(|e| format!("{}: {e}", file.display()))?;
    }
    Ok(dir)
}

/// Write the member `name` (`@org/pkg`, or `pkg` in the root's org) into `root/pkg` and list it in
/// the root manifest's `members` (RFC 0084 D7). Returns the member's directory.
pub fn create_member(root: &Path, name: &str) -> Result<PathBuf, String> {
    let manifest = root.join("package.yag");
    let text = std::fs::read_to_string(&manifest)
        .map_err(|_| "`--member` needs a package.yag in the current directory".to_owned())?;
    let (m, _) = yan_yag::parse("package.yag", &text, false);
    let root_manifest = m
        .and_then(|m| yan_elab::manifest(&m))
        .ok_or("package.yag declares no `package: Package = Package(...)`")?;
    let full = match name.strip_prefix('@') {
        Some(_) => name.to_owned(),
        None => {
            let org = root_manifest
                .name
                .as_deref()
                .and_then(|n| n.strip_prefix('@')?.split_once('/'))
                .map_or("local", |(org, _)| org);
            format!("@{org}/{name}")
        }
    };
    let pkg = full.rsplit('/').next().expect("a split has a last part");
    let dir = create(root, &full, Template::Member)?;
    let updated = add_member(&text, pkg).ok_or("package.yag has no `Package(...)` call to edit")?;
    std::fs::write(&manifest, updated).map_err(|e| format!("{}: {e}", manifest.display()))?;
    Ok(dir)
}

/// `manifest` with `member` listed in `members`, adding the field when the manifest has none.
fn add_member(manifest: &str, member: &str) -> Option<String> {
    if let Some(at) = manifest.find("members=[") {
        let open = at + "members=[".len();
        let close = open + manifest[open..].find(']')?;
        let sep = if manifest[open..close].trim().is_empty() {
            ""
        } else {
            ", "
        };
        return Some(format!(
            "{}{sep}\"{member}\"{}",
            &manifest[..close],
            &manifest[close..]
        ));
    }
    let end = manifest.trim_end().strip_suffix(')')?.trim_end();
    let comma = if end.ends_with(',') || end.ends_with('(') {
        ""
    } else {
        ","
    };
    Some(format!("{end}{comma}\n    members=[\"{member}\"],\n)\n"))
}

fn is_segment(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yan-new-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn every_template_checks_and_passes_its_tests() {
        for t in Template::value_variants() {
            let tag = t.to_possible_value().unwrap().get_name().to_owned();
            let parent = scratch(&tag);
            let dir = create(&parent, "@acme/demo", *t).unwrap();
            let manifest = std::fs::read_to_string(dir.join("package.yag")).unwrap();
            let (m, _) = yan_yag::parse("package.yag", &manifest, true);
            let m = yan_elab::manifest(&m.unwrap()).expect("a Package(...)");
            assert_eq!(m.name.as_deref(), Some("@acme/demo"));
            let report = test::run(&dir, None, false, None, None).unwrap();
            assert!(!report.failed(false), "{tag}: {:?}", report.lines());
            assert!(!report.tests.is_empty(), "{tag} has no tests");
            std::fs::remove_dir_all(&parent).unwrap();
        }
    }

    #[test]
    fn names() {
        let parent = scratch("names");
        let dir = create(&parent, "tool_1", Template::Cli).unwrap();
        assert_eq!(dir, parent.join("tool_1"));
        let manifest = std::fs::read_to_string(dir.join("package.yag")).unwrap();
        assert!(manifest.contains("name=\"@local/tool_1\""));
        for bad in ["Tool", "1tool", "@acme", "@Acme/x", "a/b", ""] {
            assert!(create(&parent, bad, Template::Cli).is_err(), "{bad}");
        }
        assert!(
            create(&parent, "tool_1", Template::Cli)
                .unwrap_err()
                .contains("already exists")
        );
        std::fs::remove_dir_all(&parent).unwrap();
    }

    #[test]
    fn a_member_is_listed_in_the_root_and_checks() {
        let root = scratch("member");
        let manifest = "package: Package = Package(name=\"@acme/root\", version=\"0.1.0\", edition=\"next\", yan=\"1.x\")\n";
        std::fs::write(root.join("package.yag"), manifest).unwrap();
        let dir = create_member(&root, "core").unwrap();
        create_member(&root, "@acme/web").unwrap();
        assert_eq!(dir, root.join("core"));
        let text = std::fs::read_to_string(root.join("package.yag")).unwrap();
        assert!(text.contains("members=[\"core\", \"web\"]"), "{text}");
        let member = std::fs::read_to_string(dir.join("package.yag")).unwrap();
        assert!(member.contains("name=\"@acme/core\""));
        assert!(
            create_member(&root, "core")
                .unwrap_err()
                .contains("already exists")
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_member_needs_a_manifest_here() {
        let root = scratch("nomember");
        assert!(
            create_member(&root, "core")
                .unwrap_err()
                .contains("package.yag")
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
