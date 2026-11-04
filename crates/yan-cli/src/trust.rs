//! `yan trust` (21.4, R676): the directories whose code `yan mcp` may run. The record is one
//! canonical path per line in `$YAN_HOME/trusted`, else `$XDG_CONFIG_HOME/yan/trusted`, else
//! `~/.config/yan/trusted`; a trusted directory covers everything beneath it.

use std::path::{Path, PathBuf};

fn store() -> Option<PathBuf> {
    let var = |k| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    var("YAN_HOME")
        .or_else(|| var("XDG_CONFIG_HOME").map(|c| c.join("yan")))
        .or_else(|| var("HOME").map(|h| h.join(".config/yan")))
        .map(|d| d.join("trusted"))
}

fn canonical(dir: &Path) -> Result<PathBuf, String> {
    dir.canonicalize()
        .map_err(|e| format!("{}: {e}", dir.display()))
}

pub fn is_trusted(dir: &Path) -> bool {
    let (Some(store), Ok(dir)) = (store(), canonical(dir)) else {
        return false;
    };
    std::fs::read_to_string(store)
        .unwrap_or_default()
        .lines()
        .any(|l| dir.starts_with(l))
}

/// Records `dir` as trusted; its canonical path.
pub fn add(dir: &Path) -> Result<PathBuf, String> {
    let dir = canonical(dir)?;
    let store = store().ok_or("no home directory to record trust in; set YAN_HOME")?;
    let mut text = std::fs::read_to_string(&store).unwrap_or_default();
    if !text.lines().any(|l| Path::new(l) == dir) {
        let line = dir.to_str().ok_or("the path is not UTF-8")?;
        text.push_str(line);
        text.push('\n');
        if let Some(parent) = store.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&store, text).map_err(|e| format!("{}: {e}", store.display()))?;
    }
    Ok(dir)
}
