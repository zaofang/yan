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
    /// The advisory text of a malicious mark (R947).
    pub malicious: Option<String>,
    pub published: i64,
}

enum Location {
    Dir(PathBuf),
    Http(String),
}

pub struct Index {
    pub source: String,
    location: Location,
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
        let location = if source.contains("://") {
            Location::Http(http_base(source)?)
        } else {
            Location::Dir(dir.clone())
        };
        Ok(Index {
            source: source.clone(),
            location,
            cool_off: self.min_release_age * 86400,
        })
    }
}

/// `https://`, or `http://` on a loopback host, for tests (R946); anything else is E017.
fn http_base(source: &str) -> Result<String, Box<Diagnostic>> {
    let loopback = |rest: &str| {
        let host = match rest.strip_prefix('[') {
            Some(v6) => &rest[..v6.find(']').map_or(0, |i| i + 2)],
            None => rest.split(['/', ':']).next().unwrap_or(""),
        };
        matches!(host, "127.0.0.1" | "[::1]")
    };
    let ok = source.starts_with("https://") || source.strip_prefix("http://").is_some_and(loopback);
    if ok {
        Ok(source.trim_end_matches('/').to_owned())
    } else {
        Err(fail(
            "E017",
            format!("index `{source}`: only `https://` (or loopback `http://`) URLs are supported"),
        ))
    }
}

const LIST_LIMIT: u64 = 1 << 20;
const ARCHIVE_LIMIT: u64 = 256 << 20;

fn host_of(url: &str) -> &str {
    url.split("://")
        .nth(1)
        .unwrap_or("")
        .split('/')
        .next()
        .unwrap_or("")
}

/// GET `url`: `None` on 404. Redirects are followed by hand, only to the same host and, from
/// `https://`, only over `https://`.
fn get(url: &str, limit: u64) -> Result<Option<Vec<u8>>, Box<Diagnostic>> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_connect(Some(std::time::Duration::from_secs(10)))
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .build()
        .into();
    let refuse = |why: String| fail("E017", format!("`{url}`: {why}"));
    let mut at = url.to_owned();
    for _ in 0..5 {
        let mut resp = agent.get(&at).call().map_err(|e| refuse(e.to_string()))?;
        let status = resp.status().as_u16();
        match status {
            404 => return Ok(None),
            200..=299 => {
                return resp
                    .body_mut()
                    .with_config()
                    .limit(limit)
                    .read_to_vec()
                    .map(Some)
                    .map_err(|e| refuse(e.to_string()));
            }
            301 | 302 | 303 | 307 | 308 => {
                let to = resp
                    .headers()
                    .get("location")
                    .and_then(|l| l.to_str().ok())
                    .ok_or_else(|| refuse("redirect without a location".into()))?;
                let same_host = host_of(to) == host_of(&at);
                let downgrade = at.starts_with("https://") && !to.starts_with("https://");
                if !to.contains("://") || !same_host || downgrade {
                    return Err(refuse(format!("redirect to `{to}` leaves the index host")));
                }
                at = to.to_owned();
            }
            _ => return Err(refuse(format!("HTTP {status}"))),
        }
    }
    Err(refuse("too many redirects".into()))
}

impl Index {
    fn read(&self, name: &str, file: &str, limit: u64) -> Result<Option<Vec<u8>>, Box<Diagnostic>> {
        match &self.location {
            Location::Dir(dir) => Ok(std::fs::read(dir.join(name).join("@v").join(file)).ok()),
            Location::Http(base) => get(&format!("{base}/{name}/@v/{file}"), limit),
        }
    }

    /// Every published version, or `None` when the index has no such package.
    pub fn versions(&self, name: &str) -> Result<Option<Vec<String>>, Box<Diagnostic>> {
        Ok(self.read(name, "list", LIST_LIMIT)?.map(|b| {
            String::from_utf8_lossy(&b)
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_owned)
                .collect()
        }))
    }

    /// The archive bytes of `version`, or `None` when the index has none.
    pub fn archive(&self, name: &str, version: &str) -> Result<Option<Vec<u8>>, Box<Diagnostic>> {
        self.read(name, &format!("{version}.zip"), ARCHIVE_LIMIT)
    }

    pub fn info(&self, name: &str, version: &str) -> Result<Option<Info>, Box<Diagnostic>> {
        let Some(text) = self.read(name, &format!("{version}.info"), LIST_LIMIT)? else {
            return Ok(None);
        };
        Ok(parse_info(&text, version))
    }

    /// The names in `name`'s namespace nearest to it (E003).
    pub fn nearest(&self, name: &str) -> Vec<String> {
        let (ns, local) = name.split_once('/').unwrap_or(("", name));
        // An HTTP index has no search snapshot yet (R645), so nothing is nearest.
        let Location::Dir(dir) = &self.location else {
            return Vec::new();
        };
        let mut near: Vec<(usize, String)> = std::fs::read_dir(dir.join(ns))
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

fn parse_info(text: &[u8], version: &str) -> Option<Info> {
    let v: serde_json::Value = serde_json::from_slice(text).ok()?;
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
        malicious: v
            .get("malicious")
            .and_then(|m| m.as_bool())
            .filter(|m| *m)
            .map(|_| s("advisory").unwrap_or_default()),
        published: epoch(&s("time")?)?,
    })
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
pub fn newest(
    index: &Index,
    name: &str,
    want: &str,
    exact: bool,
    now: i64,
) -> Result<Option<Info>, Box<Diagnostic>> {
    let mut best: Option<Info> = None;
    for v in index.versions(name)?.iter().flatten() {
        if !satisfies(v, want) || v.contains('-') {
            continue;
        }
        let Some(i) = index.info(name, v)? else {
            continue;
        };
        let newer = best
            .as_ref()
            .is_none_or(|b| parts(&i.version) > parts(&b.version));
        if !i.yanked
            && i.malicious.is_none()
            && (exact || i.published + index.cool_off <= now)
            && newer
        {
            best = Some(i);
        }
    }
    Ok(best)
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

    /// A loopback server answering each request with the next canned response.
    fn serve(responses: Vec<String>) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for r in responses {
                let (mut c, _) = listener.accept().unwrap();
                let mut buf = [0; 2048];
                let _ = c.read(&mut buf);
                c.write_all(r.as_bytes()).unwrap();
            }
        });
        format!("http://{addr}")
    }

    fn ok(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn http_index(base: &str) -> Index {
        Index {
            source: base.into(),
            location: Location::Http(http_base(base).unwrap()),
            cool_off: 0,
        }
    }

    #[test]
    fn an_http_index_serves_list_and_info_and_404_is_absent() {
        let info = r#"{"digest":"sha256:ab","publisher":"p","yanked":false,"time":"2020-01-01T00:00:00Z"}"#;
        let base = serve(vec![
            ok("1.0.0\n1.1.0\n"),
            ok(info),
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
        ]);
        let index = http_index(&base);
        assert_eq!(index.versions("@a/b").unwrap().unwrap(), ["1.0.0", "1.1.0"]);
        assert_eq!(index.info("@a/b", "1.0.0").unwrap().unwrap().publisher, "p");
        assert!(index.versions("@a/none").unwrap().is_none());
    }

    #[test]
    fn a_server_error_an_oversize_body_and_a_foreign_redirect_fail_loudly() {
        let big = "x".repeat(LIST_LIMIT as usize + 1);
        let base = serve(vec![
            "HTTP/1.1 500 Oops\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
            ok(&big),
            "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.2:1/x\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into(),
        ]);
        let index = http_index(&base);
        for _ in 0..3 {
            assert_eq!(index.versions("@a/b").unwrap_err().code, "E017");
        }
    }

    #[test]
    fn only_https_and_loopback_http_are_urls() {
        assert!(http_base("https://example.org/idx/").is_ok());
        assert!(http_base("http://127.0.0.1:8080").is_ok());
        assert!(http_base("http://[::1]:8080").is_ok());
        assert!(http_base("http://example.org").is_err());
        assert!(http_base("ftp://example.org").is_err());
    }
}
