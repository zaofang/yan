//! Static package indexes served from a directory (19.3, R636, R895): `registries` from
//! `.yan/config.yag`, `<index>/@org/name/@v/list` and `<ver>.info`.

use std::path::{Path, PathBuf};

use yan_diag::Diagnostic;

use crate::lock::{fail, parts};

/// `min_release_age`'s default in days (19.4).
const DEFAULT_AGE_DAYS: i64 = 7;

pub struct Info {
    pub version: String,
    pub digest: String,
    pub publisher: String,
    pub capability_digest: Option<String>,
    pub requirements: Vec<String>,
    pub yanked: bool,
    pub published: i64,
}

pub struct Index {
    pub source: String,
    dir: PathBuf,
    cool_off: i64,
}

/// Namespace to index and the cool-off, from the workspace's `.yan/config.yag` and then the
/// user's (R943).
pub struct Registries {
    entries: Vec<(String, String, PathBuf)>,
    min_release_age: i64,
}

fn read(base: &Path) -> Result<yan_elab::Config, Box<Diagnostic>> {
    let path = base.join(".yan/config.yag");
    let Ok(src) = std::fs::read_to_string(&path) else {
        return Ok(Default::default());
    };
    let (m, _) = yan_yag::parse("config.yag", &src, false);
    let config = m.map(|m| yan_elab::config(&m)).unwrap_or_default();
    match config.unknown.first() {
        Some(n) => Err(fail(
            "E018",
            format!("`{n}` is not a `Config` field in `{}`", path.display()),
        )),
        None => Ok(config),
    }
}

/// The `store` path of the workspace configuration, else the user's (R943).
pub fn configured_store(root: &Path) -> Option<String> {
    let workspace = read(root).ok()?.store;
    workspace.or_else(|| {
        let home = std::env::var_os("HOME")?;
        read(Path::new(&home)).ok()?.store
    })
}

impl Registries {
    pub fn load(root: &Path) -> Result<Self, Box<Diagnostic>> {
        let workspace = read(root)?;
        let user = match std::env::var_os("HOME") {
            Some(home) => read(Path::new(&home))?,
            None => Default::default(),
        };
        let entries = [
            (root, &workspace),
            (
                Path::new(&std::env::var_os("HOME").unwrap_or_default()),
                &user,
            ),
        ]
        .into_iter()
        .flat_map(|(base, c)| {
            c.registries
                .iter()
                .map(|(ns, to)| (ns.clone(), to.clone(), base.join(to)))
        })
        .collect();
        // A workspace value below the user's (or the default) is ignored.
        let floor = user.min_release_age.unwrap_or(DEFAULT_AGE_DAYS);
        Ok(Self {
            entries,
            min_release_age: floor.max(workspace.min_release_age.unwrap_or(0)),
        })
    }

    /// The index of `name`'s namespace. A mapped namespace never falls back (19.4).
    pub fn index(&self, name: &str) -> Result<Index, Box<Diagnostic>> {
        let ns = name.split('/').next().unwrap_or(name);
        let (_, source, dir) = self.entries.iter().find(|(n, ..)| n == ns).ok_or_else(|| {
            fail(
                "E021",
                format!("namespace `{ns}` has no `registries` mapping in `.yan/config.yag`"),
            )
        })?;
        if source.contains("://") {
            return Err(fail(
                "E017",
                format!("index `{source}`: only directory indexes are supported yet"),
            ));
        }
        Ok(Index {
            source: source.clone(),
            dir: dir.clone(),
            cool_off: self.min_release_age * 86400,
        })
    }
}

impl Index {
    fn package_dir(&self, name: &str) -> PathBuf {
        self.dir.join(name).join("@v")
    }

    /// Every published version, or `None` when the index has no such package.
    pub fn versions(&self, name: &str) -> Option<Vec<String>> {
        let text = std::fs::read_to_string(self.package_dir(name).join("list")).ok()?;
        Some(
            text.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_owned)
                .collect(),
        )
    }

    pub fn info(&self, name: &str, version: &str) -> Option<Info> {
        let text =
            std::fs::read_to_string(self.package_dir(name).join(format!("{version}.info"))).ok()?;
        let v: serde_json::Value = serde_json::from_str(&text).ok()?;
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_owned);
        Some(Info {
            version: version.to_owned(),
            digest: s("digest")?,
            publisher: s("publisher")?,
            capability_digest: s("capability_digest"),
            requirements: v
                .get("requirements")
                .and_then(|r| r.as_array())
                .map(|r| {
                    r.iter()
                        .filter_map(|x| x.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
            yanked: v.get("yanked").and_then(|y| y.as_bool()).unwrap_or(false),
            published: epoch(&s("time")?)?,
        })
    }

    /// The names in `name`'s namespace nearest to it (E003).
    pub fn nearest(&self, name: &str) -> Vec<String> {
        let (ns, local) = name.split_once('/').unwrap_or(("", name));
        let mut near: Vec<(usize, String)> = std::fs::read_dir(self.dir.join(ns))
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .map(|n| (distance(local, &n), n))
            .filter(|(d, _)| *d <= 2)
            .collect();
        near.sort();
        near.into_iter()
            .take(3)
            .map(|(_, n)| format!("{ns}/{n}"))
            .collect()
    }
}

fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != cb)).min(cur + 1).min(row[j] + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

/// Seconds since the epoch of `YYYY-MM-DDTHH:MM:SSZ`.
fn epoch(t: &str) -> Option<i64> {
    let n: Vec<i64> = t
        .trim_end_matches('Z')
        .split(['-', 'T', ':'])
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    let [y, m, d, h, mi, s] = n[..] else {
        return None;
    };
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86400 + h * 3600 + mi * 60 + s)
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Whether `version` satisfies the requirement text `want` (`1`, `1.4`, `1.4.2`): the same
/// major (the minor in 0.y.z) and not below it.
pub fn satisfies(version: &str, want: &str) -> bool {
    crate::lock::identity("", version) == crate::lock::identity("", want)
        && parts(version) >= parts(want)
}

/// The newest version satisfying `want` that is not yanked and, unless `exact`, is older than
/// the cool-off (19.4).
pub fn newest(index: &Index, name: &str, want: &str, exact: bool, now: i64) -> Option<Info> {
    index
        .versions(name)?
        .iter()
        .filter(|v| satisfies(v, want) && !v.contains('-'))
        .filter_map(|v| index.info(name, v))
        .filter(|i| !i.yanked && (exact || i.published + index.cool_off <= now))
        .max_by_key(|i| parts(&i.version))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_matches_known_dates() {
        assert_eq!(epoch("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(epoch("2000-03-01T00:00:00Z"), Some(951_868_800));
    }

    #[test]
    fn satisfies_is_same_major_and_not_below() {
        assert!(satisfies("1.5.0", "1.4"));
        assert!(!satisfies("1.3.9", "1.4"));
        assert!(!satisfies("2.0.0", "1.4"));
        assert!(satisfies("0.3.2", "0.3.1"));
        assert!(!satisfies("0.4.0", "0.3.1"));
    }

    #[test]
    fn distance_counts_edits() {
        assert_eq!(distance("http", "htpp"), 1);
        assert_eq!(distance("http", "http"), 0);
    }
}
