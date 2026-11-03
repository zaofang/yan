//! std.path: lexical `Path` (`/` on every host) and `WindowsPath` text (R587 to R592).

use std::cmp::Ordering;
use std::rc::Rc;

use yan_std::registry::{Id, id::*};

use super::bind::{Bind, bad_args};
use super::native::opt;
use super::{R, Value, panic};
use Value::*;

/// Windows path text; equal, ordered and hashed by its case-folded form.
#[derive(Clone, Debug)]
pub(crate) struct WinPath {
    pub text: Rc<str>,
    folded: Rc<str>,
}

impl PartialEq for WinPath {
    fn eq(&self, o: &Self) -> bool {
        self.folded == o.folded
    }
}

impl WinPath {
    pub fn folded(&self) -> &str {
        &self.folded
    }
}

/// A path split into its anchor (`/`, or a Windows drive and root) and its segments.
#[derive(Clone)]
struct Parsed {
    drive: String,
    root: &'static str,
    segs: Vec<String>,
    win: bool,
}

impl Parsed {
    fn posix(s: &str) -> Parsed {
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

    fn windows(s: &str) -> Parsed {
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

    fn sep(&self) -> &'static str {
        if self.win { "\\" } else { "/" }
    }

    fn anchor(&self) -> String {
        format!("{}{}", self.drive, self.root)
    }

    fn text(&self) -> String {
        let t = format!("{}{}", self.anchor(), self.segs.join(self.sep()));
        if t.is_empty() { ".".into() } else { t }
    }

    fn value(&self) -> Value {
        let t = self.text();
        if self.win {
            WindowsPath(Rc::new(WinPath {
                folded: t.to_lowercase().into(),
                text: t.into(),
            }))
        } else {
            Path(t.into())
        }
    }

    fn parts(&self) -> Vec<String> {
        let a = self.anchor();
        (!a.is_empty())
            .then_some(a)
            .into_iter()
            .chain(self.segs.iter().cloned())
            .collect()
    }

    fn fold(&self, x: &str) -> String {
        if self.win {
            x.to_lowercase()
        } else {
            x.to_owned()
        }
    }

    fn folded_parts(&self) -> Vec<String> {
        self.parts().iter().map(|x| self.fold(x)).collect()
    }

    fn name(&self) -> &str {
        self.segs.last().map_or("", String::as_str)
    }

    fn suffix(&self) -> &str {
        let n = self.name();
        match n.rfind('.') {
            Some(i) if i > 0 => &n[i..],
            _ => "",
        }
    }

    fn with_segs(&self, segs: Vec<String>) -> Parsed {
        Parsed {
            segs,
            ..self.clone()
        }
    }

    fn parent(&self) -> Parsed {
        let mut segs = self.segs.clone();
        segs.pop();
        self.with_segs(segs)
    }

    fn join(&self, part: &Parsed) -> Parsed {
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

    fn normalize(&self) -> Parsed {
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
    fn strip(&self, base: &Parsed) -> Option<Vec<String>> {
        let f = |p: &Parsed| p.segs.iter().map(|x| self.fold(x)).collect::<Vec<_>>();
        (self.fold(&self.anchor()) == self.fold(&base.anchor()) && f(self).starts_with(&f(base)))
            .then(|| self.segs[base.segs.len()..].to_vec())
    }
}

fn parsed(v: &Value) -> Parsed {
    match v {
        Path(s) => Parsed::posix(s),
        WindowsPath(w) => Parsed::windows(&w.text),
        Str(s) => Parsed::posix(s),
        _ => unreachable!("a path"),
    }
}

/// `show(p)`: the path text.
pub(super) fn text(v: &Value) -> &str {
    match v {
        Path(s) => s,
        WindowsPath(w) => &w.text,
        _ => unreachable!("a path"),
    }
}

/// Paths order by their parts; Windows parts compare case-folded.
pub(super) fn order(a: &Value, b: &Value) -> Ordering {
    parsed(a).folded_parts().cmp(&parsed(b).folded_parts())
}

fn with_name(p: &Parsed, n: &str) -> R<Value> {
    if p.name().is_empty() || n.is_empty() || n == "." || n.contains(['/', '\\']) {
        return panic("value");
    }
    let mut segs = p.segs.clone();
    *segs.last_mut().expect("named") = n.to_owned();
    Ok(p.with_segs(segs).value())
}

fn with_suffix(p: &Parsed, s: &str) -> R<Value> {
    if !s.is_empty() && (!s.starts_with('.') || s == "." || s.contains(['/', '\\'])) {
        return panic("value");
    }
    let n = p.name();
    let stem = &n[..n.len() - p.suffix().len()];
    with_name(p, &format!("{stem}{s}"))
}

fn strs(xs: Vec<String>) -> Value {
    List(xs.into_iter().map(|x| Str(x.into())).collect())
}

macro_rules! f {
    ($id:ident, |$a:ident| { $($pat:pat => $e:expr),+ $(,)? }) => {
        (
            $id,
            Bind::Plain(|$a| {
                Ok(match &mut *$a {
                    $($pat => $e,)+
                    #[allow(unreachable_patterns)]
                    _ => return bad_args($id),
                })
            }),
        )
    };
}

/// The binds shared by `Path` and `WindowsPath`, one table per type.
macro_rules! common {
    ($T:ident, $text:ident, $parent:ident, $name:ident, $suffix:ident, $stem:ident, $with_name:ident,
     $with_suffix:ident, $parts:ident, $abs:ident, $rel:ident, $starts:ident, $norm:ident) => {
        [
            f!($text, |a| { [p @ $T(_)] => Str(text(p).into()) }),
            f!($parent, |a| { [p @ $T(_)] => parsed(p).parent().value() }),
            f!($name, |a| { [p @ $T(_)] => Str(parsed(p).name().into()) }),
            f!($suffix, |a| { [p @ $T(_)] => Str(parsed(p).suffix().into()) }),
            f!($stem, |a| {
                [p @ $T(_)] => {
                    let p = parsed(p);
                    let n = p.name();
                    Str(n[..n.len() - p.suffix().len()].into())
                }
            }),
            f!($with_name, |a| { [p @ $T(_), Str(n)] => with_name(&parsed(p), n)? }),
            f!($with_suffix, |a| { [p @ $T(_), Str(s)] => with_suffix(&parsed(p), s)? }),
            f!($parts, |a| { [p @ $T(_)] => strs(parsed(p).parts()) }),
            f!($abs, |a| {
                [p @ $T(_)] => {
                    let p = parsed(p);
                    Bool(!p.root.is_empty() && (!p.win || !p.drive.is_empty()))
                }
            }),
            f!($rel, |a| {
                [p @ $T(_), b @ $T(_)] => {
                    let p = parsed(p);
                    opt(p.strip(&parsed(b)).map(|segs| {
                        Parsed { drive: String::new(), root: "", segs, win: p.win }.value()
                    }))
                }
            }),
            f!($starts, |a| { [p @ $T(_), b @ $T(_)] => Bool(parsed(p).strip(&parsed(b)).is_some()) }),
            f!($norm, |a| { [p @ $T(_)] => parsed(p).normalize().value() }),
        ]
    };
}

static POSIX: [(Id, Bind); 12] = common!(
    Path,
    STD_PATH_TEXT,
    STD_PATH_PARENT,
    STD_PATH_NAME,
    STD_PATH_SUFFIX,
    STD_PATH_STEM,
    STD_PATH_WITH_NAME,
    STD_PATH_WITH_SUFFIX,
    STD_PATH_PARTS,
    STD_PATH_IS_ABSOLUTE,
    STD_PATH_RELATIVE_TO,
    STD_PATH_STARTS_WITH,
    STD_PATH_NORMALIZE
);

static WINDOWS: [(Id, Bind); 12] = common!(
    WindowsPath,
    STD_PATH_WINDOWS_TEXT,
    STD_PATH_WINDOWS_PARENT,
    STD_PATH_WINDOWS_NAME,
    STD_PATH_WINDOWS_SUFFIX,
    STD_PATH_WINDOWS_STEM,
    STD_PATH_WINDOWS_WITH_NAME,
    STD_PATH_WINDOWS_WITH_SUFFIX,
    STD_PATH_WINDOWS_PARTS,
    STD_PATH_WINDOWS_IS_ABSOLUTE,
    STD_PATH_WINDOWS_RELATIVE_TO,
    STD_PATH_WINDOWS_STARTS_WITH,
    STD_PATH_WINDOWS_NORMALIZE
);

static OTHER: &[(Id, Bind)] = &[
    f!(STD_PRELUDE_PATH, |a| { [Str(s)] => Parsed::posix(s).value() }),
    f!(STD_PRELUDE_WINDOWSPATH, |a| { [Str(s)] => Parsed::windows(s).value() }),
    f!(STD_TEXT_AS_PATH, |a| { [Str(s)] => Parsed::posix(s).value() }),
    f!(STD_PATH_AS_PATH, |a| { [p @ Path(_)] => p.clone() }),
    f!(STD_PATH_JOIN, |a| { [p @ Path(_), q @ (Path(_) | Str(_))] => parsed(p).join(&parsed(q)).value() }),
    f!(STD_PATH_WINDOWS_JOIN, |a| {
        [p @ WindowsPath(_), Str(q)] => parsed(p).join(&Parsed::windows(q)).value(),
    }),
    f!(STD_PATH_WINDOWS_DRIVE, |a| { [p @ WindowsPath(_)] => Str(parsed(p).drive.into()) }),
    f!(STD_PATH_WINDOWS_ROOT, |a| { [p @ WindowsPath(_)] => Str(parsed(p).root.into()) }),
    f!(STD_PATH_WINDOWS_ANCHOR, |a| { [p @ WindowsPath(_)] => Str(parsed(p).anchor().into()) }),
    f!(STD_PATH_WINDOWS_TO_POSIX, |a| {
        [p @ WindowsPath(_)] => {
            let p = parsed(p);
            opt((p.drive.is_empty() && p.root.is_empty())
                .then(|| Parsed { win: false, ..p }.value()))
        }
    }),
];

pub(super) fn binds() -> impl Iterator<Item = &'static (Id, Bind)> {
    POSIX.iter().chain(&WINDOWS).chain(OTHER)
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
