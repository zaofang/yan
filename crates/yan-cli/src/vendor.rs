//! `yan approve` and `yan vendor` (19.4, R948): local approval bundles under `.yan/bundles/` and
//! the vendored copy of every locked archive.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::deps::Failure;
use crate::lock::fail;
use crate::registry::Registries;
use crate::store;

const BUNDLES: &str = ".yan/bundles";

fn bundle_dirs(root: &Path) -> [PathBuf; 2] {
    [root.join(BUNDLES), root.join("vendor/bundles")]
}

/// The digests (`sha256:...`) of the approval bundles for `name` at `version`.
pub fn approvals_of(root: &Path, name: &str, version: &str) -> Vec<String> {
    let mut found = Vec::new();
    for dir in bundle_dirs(root) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let Ok(bytes) = std::fs::read(entry.path()) else {
                continue;
            };
            let digest = format!("sha256:{}", store::sha256(&bytes));
            let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                continue;
            };
            let is = |k: &str, want: &str| v.get(k).and_then(|x| x.as_str()) == Some(want);
            if is("package", name) && is("version", version) && !found.contains(&digest) {
                found.push(digest);
            }
        }
    }
    found.sort();
    found
}

/// `yan approve @org/name@x.y.z`: write the bundle, then refresh the lock if the package is in it.
pub fn approve(root: &Path, spec: &str) -> Result<String, Failure> {
    let (name, version) = spec
        .rfind('@')
        .filter(|i| *i > 0)
        .map(|i| (&spec[..i], &spec[i + 1..]))
        .ok_or_else(|| Failure::Msg(format!("`{spec}` is not `@org/name@x.y.z`")))?;
    let index = Registries::load(root)?.index(name)?;
    let info = index
        .info(name, version)?
        .ok_or_else(|| fail("E003", format!("`{name}` {version} is not in the index")))?;
    let record = serde_json::json!({
        "package": name,
        "version": version,
        "digest": info.digest,
        "capability_digest": info.capability_digest,
        "publisher": info.publisher,
        "signer": "local",
    });
    let text = serde_json::to_string_pretty(&record).unwrap_or_default() + "\n";
    let digest = store::sha256(text.as_bytes());
    let dir = root.join(BUNDLES);
    std::fs::create_dir_all(&dir)
        .and_then(|()| std::fs::write(dir.join(format!("{digest}.json")), text))
        .map_err(|e| Failure::Msg(format!("cannot write the bundle: {e}")))?;
    if crate::lock::is_locked(root, name) {
        crate::lock::write(root)?;
    }
    Ok(format!(
        "approved {name} {version} (sha256:{digest}, signer local)"
    ))
}

fn write_if_changed(path: &Path, bytes: &[u8]) -> Result<bool, Failure> {
    if std::fs::read(path).is_ok_and(|old| old == bytes) {
        return Ok(false);
    }
    let io = |e: std::io::Error| Failure::Msg(format!("{}: {e}", path.display()));
    std::fs::create_dir_all(path.parent().unwrap_or(Path::new("."))).map_err(io)?;
    std::fs::write(path, bytes).map_err(io)?;
    Ok(true)
}

/// Remove the files of `dir` whose stem is not in `keep`.
fn prune(dir: &Path, keep: &HashSet<String>, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if !keep.contains(stem) && std::fs::remove_file(&path).is_ok() {
            out.push(format!(
                "removed {}",
                path.strip_prefix(dir.parent().unwrap_or(dir))
                    .unwrap_or(&path)
                    .display()
            ));
        }
    }
}

/// `yan vendor`: `vendor/archives/<sha256>.zip` and `vendor/bundles/<sha256>.json` for everything
/// `yan.lock` references, each digest verified while copying; deterministic and idempotent.
pub fn vendor(root: &Path) -> Result<Vec<String>, Failure> {
    let text = std::fs::read_to_string(root.join("yan.lock"))
        .map_err(|_| fail("E020", "`yan.lock` is missing; run `yan lock`".into()))?;
    let table: toml::Table = text
        .parse()
        .map_err(|e| fail("E020", format!("`yan.lock` does not parse: {e}")))?;
    let records = table
        .get("package")
        .and_then(toml::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let configured = crate::registry::configured_store(root);
    let roots = store::roots(root, configured.as_deref());
    let registries = Registries::load(root)?;
    let (mut out, mut archives, mut bundles) = (Vec::new(), HashSet::new(), HashSet::new());
    for p in &records {
        let field = |k: &str| p.get(k).and_then(toml::Value::as_str);
        let (Some(name), Some(version), Some(digest)) =
            (field("name"), field("version"), field("digest"))
        else {
            continue;
        };
        let hex = digest.trim_start_matches("sha256:");
        let bytes = match store::find(&roots, digest)? {
            Some((_, bytes)) => bytes,
            None => {
                let bytes = registries
                    .index(name)?
                    .archive(name, version)?
                    .ok_or_else(|| {
                        fail(
                            "E020",
                            format!("no archive for `{name}` {version} in any store or its index"),
                        )
                    })?;
                if store::sha256(&bytes) != hex {
                    return Err(fail(
                        "E020",
                        format!("the archive of `{name}` {version} does not hash to `{digest}`"),
                    )
                    .into());
                }
                bytes
            }
        };
        archives.insert(hex.to_owned());
        if write_if_changed(
            &root.join("vendor/archives").join(format!("{hex}.zip")),
            &bytes,
        )? {
            out.push(format!("vendored {name} {version}"));
        }
        for b in p
            .get("approvals")
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(b) = b.as_str().map(|b| b.trim_start_matches("sha256:")) else {
                continue;
            };
            let found = bundle_dirs(root)
                .iter()
                .find_map(|d| std::fs::read(d.join(format!("{b}.json"))).ok());
            let Some(body) = found.filter(|body| store::sha256(body) == b) else {
                return Err(fail(
                    "E020",
                    format!("approval bundle `sha256:{b}` of `{name}` is missing or corrupt"),
                )
                .into());
            };
            bundles.insert(b.to_owned());
            write_if_changed(
                &root.join("vendor/bundles").join(format!("{b}.json")),
                &body,
            )?;
        }
    }
    prune(&root.join("vendor/archives"), &archives, &mut out);
    prune(&root.join("vendor/bundles"), &bundles, &mut out);
    if out.is_empty() {
        out.push("vendor is up to date".into());
    }
    Ok(out)
}
