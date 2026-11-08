//! The content-addressed archive store (19.4, R944): `archives/<sha256>.zip` and the unpacked
//! `src/<sha256>/` under the workspace `.yan/store` (or the configured `store`), `vendor/`, or
//! the user store, searched in that order.

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use yan_diag::Diagnostic;

use crate::lock::fail;

/// The store directories in lookup order.
pub fn roots(root: &Path, configured: Option<&str>) -> Vec<PathBuf> {
    let user = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".cache")))
        .map(|c| c.join("yan/store"));
    [
        Some(root.join(configured.unwrap_or(".yan/store"))),
        Some(root.join("vendor")),
        user,
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn hex(digest: &str) -> &str {
    digest.trim_start_matches("sha256:")
}

pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The first store holding the archive for `digest`, re-verified against it (E020 on a mismatch).
pub fn find(
    roots: &[PathBuf],
    digest: &str,
) -> Result<Option<(PathBuf, Vec<u8>)>, Box<Diagnostic>> {
    let want = hex(digest);
    for r in roots {
        let path = r.join("archives").join(format!("{want}.zip"));
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        if sha256(&bytes) != want {
            return Err(fail(
                "E020",
                format!("`{}` does not hash to `sha256:{want}`", path.display()),
            ));
        }
        return Ok(Some((r.clone(), bytes)));
    }
    Ok(None)
}

fn e022(message: String) -> Box<Diagnostic> {
    fail("E022", message)
}

/// Unpack `bytes` into `<store>/src/<digest>`, through a temporary directory and a rename.
/// Rejects links, absolute and `..` paths and duplicate paths (E022).
pub fn unpack(store: &Path, digest: &str, bytes: &[u8]) -> Result<PathBuf, Box<Diagnostic>> {
    let dest = store.join("src").join(hex(digest));
    if dest.is_dir() {
        return Ok(dest);
    }
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| {
        e022(format!(
            "`sha256:{}` is not a readable archive: {e}",
            hex(digest)
        ))
    })?;
    // The reader collapses entries that share a name, so compare with the declared count.
    if declared_entries(bytes).is_some_and(|n| n != zip.len()) {
        return Err(e022(format!(
            "`sha256:{}` has a duplicate path",
            hex(digest)
        )));
    }
    let tmp = store
        .join("src")
        .join(format!(".tmp-{}-{}", hex(digest), std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let io = |e: std::io::Error| fail("E020", format!("store `{}`: {e}", store.display()));
    std::fs::create_dir_all(&tmp).map_err(io)?;
    let result = extract(&mut zip, &tmp).and_then(|()| {
        match std::fs::rename(&tmp, &dest) {
            // A concurrent unpack of the same digest produced identical content.
            Err(_) if dest.is_dir() => Ok(()),
            r => r.map_err(io),
        }
    });
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    result.map(|()| dest)
}

/// The entry count in the end-of-central-directory record (`None` for zip64 or no record).
fn declared_entries(bytes: &[u8]) -> Option<usize> {
    let at = bytes
        .windows(4)
        .rposition(|w| w == [0x50, 0x4b, 0x05, 0x06])?;
    let n = u16::from_le_bytes(bytes.get(at + 10..at + 12)?.try_into().ok()?);
    (n != u16::MAX).then_some(usize::from(n))
}

fn extract(
    zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
    into: &Path,
) -> Result<(), Box<Diagnostic>> {
    let mut seen = HashSet::new();
    for i in 0..zip.len() {
        let mut f = zip
            .by_index(i)
            .map_err(|e| e022(format!("archive entry {i}: {e}")))?;
        let name = String::from_utf8_lossy(f.name_raw()).into_owned();
        let link = f.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000);
        let path = Path::new(&name);
        let unsafe_path = name.starts_with('/')
            || name.contains('\\')
            || path
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)));
        if link {
            return Err(e022(format!("archive entry `{name}` is a symbolic link")));
        }
        if unsafe_path {
            return Err(e022(format!(
                "archive entry `{name}` is absolute or contains `..`"
            )));
        }
        if !seen.insert(name.trim_end_matches('/').to_owned()) {
            return Err(e022(format!("archive path `{name}` appears twice")));
        }
        let out = into.join(path);
        let io = |e: std::io::Error| e022(format!("archive entry `{name}`: {e}"));
        if f.is_dir() {
            std::fs::create_dir_all(&out).map_err(io)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        let mut data = Vec::new();
        f.read_to_end(&mut data).map_err(io)?;
        std::fs::write(&out, data).map_err(io)?;
    }
    Ok(())
}

/// The unpacked source for `digest`: the archive is found, re-verified and unpacked on first use.
pub fn source(roots: &[PathBuf], digest: &str) -> Result<Option<PathBuf>, Box<Diagnostic>> {
    let Some((store, bytes)) = find(roots, digest)? else {
        return Ok(None);
    };
    unpack(&store, digest, &bytes).map(Some)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Write;

    use super::*;

    pub(crate) fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (n, b) in entries {
            w.start_file(*n, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(b).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("yan-store-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(store: &Path, bytes: &[u8]) -> String {
        let d = sha256(bytes);
        std::fs::create_dir_all(store.join("archives")).unwrap();
        std::fs::write(store.join("archives").join(format!("{d}.zip")), bytes).unwrap();
        format!("sha256:{d}")
    }

    #[test]
    fn unpacks_by_digest_and_prefers_the_workspace_store() {
        let dir = tmp("ok");
        let bytes = zip_of(&[("a/b.txt", b"hi")]);
        let digest = put(&dir.join("vendor"), &bytes);
        let roots = roots(&dir, None);
        let src = source(&roots, &digest).unwrap().unwrap();
        assert_eq!(src, dir.join("vendor/src").join(hex(&digest)));
        assert_eq!(std::fs::read(src.join("a/b.txt")).unwrap(), b"hi");
    }

    #[test]
    fn a_corrupt_archive_is_refused() {
        let dir = tmp("corrupt");
        let digest = put(&dir.join(".yan/store"), &zip_of(&[("a", b"x")]));
        let path = dir
            .join(".yan/store/archives")
            .join(format!("{}.zip", hex(&digest)));
        std::fs::write(path, b"junk").unwrap();
        assert_eq!(find(&roots(&dir, None), &digest).unwrap_err().code, "E020");
    }

    #[test]
    fn unsafe_entries_are_e022() {
        for name in ["../x", "/x", "a/../x"] {
            let dir = tmp("unsafe");
            let digest = put(&dir.join(".yan/store"), &zip_of(&[(name, b"x")]));
            let err = source(&roots(&dir, None), &digest).unwrap_err();
            assert_eq!(err.code, "E022", "{name}");
            assert!(!dir.join("outside").exists());
        }
    }

    #[test]
    fn duplicate_paths_are_e022() {
        let dir = tmp("dup");
        // The writer refuses duplicate names; rename one entry in the bytes (names are not checksummed).
        let mut bytes = zip_of(&[("ab", b"1"), ("ac", b"2")]);
        for i in 0..bytes.len() - 1 {
            if &bytes[i..i + 2] == b"ac" {
                bytes[i + 1] = b'b';
            }
        }
        let digest = put(&dir.join(".yan/store"), &bytes);
        assert_eq!(
            source(&roots(&dir, None), &digest).unwrap_err().code,
            "E022"
        );
    }

    #[test]
    fn symlinks_are_e022() {
        let dir = tmp("link");
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        w.add_symlink("l", "/etc/passwd", zip::write::SimpleFileOptions::default())
            .unwrap();
        let bytes = w.finish().unwrap().into_inner();
        let digest = put(&dir.join(".yan/store"), &bytes);
        assert_eq!(
            source(&roots(&dir, None), &digest).unwrap_err().code,
            "E022"
        );
        assert!(!dir.join(".yan/store/src").join(hex(&digest)).exists());
    }
}
