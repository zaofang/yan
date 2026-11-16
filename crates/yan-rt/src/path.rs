//! Lexical paths (R587 to R592): the segments of `Path` and `WindowsPath` text, shared by the
//! interpreter and the native backend.

/// A path split into its anchor (`/`, or a Windows drive and root) and its segments.
#[derive(Clone)]
pub struct Parsed {
    pub drive: String,
    pub root: &'static str,
    pub segs: Vec<String>,
    pub win: bool,
}

impl Parsed {
    /// `win` selects `WindowsPath` over `Path` rules.
    pub fn new(win: bool, s: &str) -> Parsed {
        if win {
            Parsed::windows(s)
        } else {
            Parsed::posix(s)
        }
    }

    pub fn posix(s: &str) -> Parsed {
        Parsed {
            drive: String::new(),
            root: if s.starts_with('/') { "/" } else { "" },
            segs: s
                .split('/')
                .filter(|x| !x.is_empty() && *x != ".")
                .map(str::to_owned)
                .collect(),
            win: false,
        }
    }

    pub fn windows(s: &str) -> Parsed {
        let mk = |drive: String, root, segs| Parsed {
            drive,
            root,
            segs,
            win: true,
        };
        // The verbatim prefix keeps everything after it as written.
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            let (first, tail) = rest.split_once('\\').unwrap_or((rest, ""));
            let segs = if tail.is_empty() {
                Vec::new()
            } else {
                tail.split('\\').map(str::to_owned).collect()
            };
            let root = if rest.contains('\\') { "\\" } else { "" };
            return mk(format!(r"\\?\{first}"), root, segs);
        }
        let s = s.replace('/', "\\");
        let split = |t: &str| -> Vec<String> {
            t.split('\\')
                .filter(|x| !x.is_empty() && *x != ".")
                .map(str::to_owned)
                .collect()
        };
        if let Some(rest) = s.strip_prefix(r"\\") {
            let mut it = rest.splitn(3, '\\');
            let server = it.next().unwrap_or("");
            let share = it.next().unwrap_or("");
            let drive = if share.is_empty() {
                format!(r"\\{server}")
            } else {
                format!(r"\\{server}\{share}")
            };
            return mk(drive, "\\", split(it.next().unwrap_or("")));
        }
        let cs: Vec<char> = s.chars().collect();
        let (drive, rest) = if cs.len() >= 2 && cs[1] == ':' && cs[0].is_ascii_alphabetic() {
            (s[..2].to_owned(), &s[2..])
        } else {
            (String::new(), &s[..])
        };
        let root = if rest.starts_with('\\') { "\\" } else { "" };
        mk(drive, root, split(rest))
    }

    pub fn sep(&self) -> &'static str {
        if self.win { "\\" } else { "/" }
    }

    pub fn anchor(&self) -> String {
        format!("{}{}", self.drive, self.root)
    }

    pub fn text(&self) -> String {
        let t = format!("{}{}", self.anchor(), self.segs.join(self.sep()));
        if t.is_empty() { ".".into() } else { t }
    }

    pub fn parts(&self) -> Vec<String> {
        let a = self.anchor();
        (!a.is_empty())
            .then_some(a)
            .into_iter()
            .chain(self.segs.iter().cloned())
            .collect()
    }

    pub fn fold(&self, x: &str) -> String {
        if self.win {
            x.to_lowercase()
        } else {
            x.to_owned()
        }
    }

    pub fn folded_parts(&self) -> Vec<String> {
        self.parts().iter().map(|x| self.fold(x)).collect()
    }

    pub fn name(&self) -> &str {
        self.segs.last().map_or("", String::as_str)
    }

    pub fn suffix(&self) -> &str {
        let n = self.name();
        match n.rfind('.') {
            Some(i) if i > 0 => &n[i..],
            _ => "",
        }
    }

    pub fn with_segs(&self, segs: Vec<String>) -> Parsed {
        Parsed {
            segs,
            ..self.clone()
        }
    }

    pub fn parent(&self) -> Parsed {
        let mut segs = self.segs.clone();
        segs.pop();
        self.with_segs(segs)
    }

    pub fn join(&self, part: &Parsed) -> Parsed {
        if !part.drive.is_empty() || (!part.root.is_empty() && !self.win) {
            return part.clone();
        }
        if !part.root.is_empty() {
            return Parsed {
                drive: self.drive.clone(),
                ..part.clone()
            };
        }
        self.with_segs(self.segs.iter().chain(&part.segs).cloned().collect())
    }

    pub fn normalize(&self) -> Parsed {
        let mut out: Vec<String> = Vec::new();
        for s in &self.segs {
            if s != ".." {
                out.push(s.clone());
            } else if out.last().is_some_and(|x| x != "..") {
                out.pop();
            } else if self.root.is_empty() {
                out.push(s.clone());
            }
        }
        self.with_segs(out)
    }

    /// The segments after `base`, when `base` has the same anchor and is a prefix by parts.
    pub fn strip(&self, base: &Parsed) -> Option<Vec<String>> {
        let f = |p: &Parsed| p.segs.iter().map(|x| self.fold(x)).collect::<Vec<_>>();
        (self.fold(&self.anchor()) == self.fold(&base.anchor()) && f(self).starts_with(&f(base)))
            .then(|| self.segs[base.segs.len()..].to_vec())
    }
}

/// `parent` (0), `name` (1), `suffix` (2), `stem` (3), anything else `normalize`.
pub fn op(win: bool, k: i64, s: &str) -> String {
    let p = Parsed::new(win, s);
    match k {
        0 => p.parent().text(),
        1 => p.name().to_owned(),
        2 => p.suffix().to_owned(),
        3 => p.name()[..p.name().len() - p.suffix().len()].to_owned(),
        _ => p.normalize().text(),
    }
}

/// `with_name` (`suffix` false) or `with_suffix`; `None` on a bad name or suffix (panic `value`).
pub fn with(win: bool, suffix: bool, p: &str, n: &str) -> Option<String> {
    let p = Parsed::new(win, p);
    let name = if suffix {
        if !n.is_empty() && (!n.starts_with('.') || n == "." || n.contains(['/', '\\'])) {
            return None;
        }
        format!("{}{n}", &p.name()[..p.name().len() - p.suffix().len()])
    } else {
        n.to_owned()
    };
    if p.name().is_empty() || name.is_empty() || name == "." || name.contains(['/', '\\']) {
        return None;
    }
    let mut segs = p.segs.clone();
    *segs.last_mut().expect("named") = name;
    Some(p.with_segs(segs).text())
}

pub fn join(win: bool, p: &str, q: &str) -> String {
    Parsed::new(win, p).join(&Parsed::new(win, q)).text()
}

pub fn is_absolute(win: bool, p: &str) -> bool {
    let p = Parsed::new(win, p);
    !p.root.is_empty() && (!p.win || !p.drive.is_empty())
}

pub fn starts_with(win: bool, p: &str, b: &str) -> bool {
    Parsed::new(win, p).strip(&Parsed::new(win, b)).is_some()
}

/// The relative path, or `None` when `b` is not a prefix.
pub fn relative_to(win: bool, p: &str, b: &str) -> Option<String> {
    let p = Parsed::new(win, p);
    let segs = p.strip(&Parsed::new(win, b))?;
    let rel = Parsed {
        drive: String::new(),
        root: "",
        segs,
        win: p.win,
    };
    Some(rel.text())
}

pub fn parts(win: bool, p: &str) -> Vec<String> {
    Parsed::new(win, p).parts()
}

/// `drive` (0), `root` (1) or `anchor` of a `WindowsPath`.
pub fn win_part(k: i64, p: &str) -> String {
    let p = Parsed::windows(p);
    match k {
        0 => p.drive,
        1 => p.root.to_owned(),
        _ => p.anchor(),
    }
}

/// The same segments as a `Path`, or `None` when there is a drive or a root.
pub fn to_posix(p: &str) -> Option<String> {
    let p = Parsed::windows(p);
    if !p.drive.is_empty() || !p.root.is_empty() {
        return None;
    }
    Some(Parsed { win: false, ..p }.text())
}

pub fn win_eq(a: &str, b: &str) -> bool {
    Parsed::windows(a).folded_parts() == Parsed::windows(b).folded_parts()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> String {
        Parsed::posix(s).text()
    }

    fn w(s: &str) -> String {
        Parsed::windows(s).text()
    }

    #[test]
    fn construction_normalizes() {
        assert_eq!(t(""), ".");
        assert_eq!(t("a//./b/"), "a/b");
        assert_eq!(t("/"), "/");
        assert_eq!(t("//x/../y"), "/x/../y");
        assert_eq!(Parsed::posix("/a/../../b").normalize().text(), "/b");
        assert_eq!(Parsed::posix("../a/..").normalize().text(), "..");
    }

    #[test]
    fn windows_anchors() {
        assert_eq!(w("C:/a/./b"), r"C:\a\b");
        assert_eq!(w(r"\\srv\share\x"), r"\\srv\share\x");
        assert_eq!(w(r"\\?\C:\a\.\b"), r"\\?\C:\a\.\b");
        assert_eq!(w("C:a"), "C:a");
        let p = Parsed::windows(r"\\srv\share\x");
        assert_eq!((p.drive.as_str(), p.root), (r"\\srv\share", "\\"));
    }
}
